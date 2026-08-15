//! Ahead-of-time compilation: `lira build file.li -o app`.
//!
//! Produces a standalone native executable. Cranelift emits an object file, a
//! small generated `main` hands the entry point to the fiber scheduler, and the
//! system C compiler links the result against `liblira_rt` — which is embedded
//! in this binary, so no separate runtime install is needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use lirac::ast::Program;
use lirac::sema::SemanticTables;

use crate::error::{CodegenError, CodegenResult};
use crate::lower::Lowerer;
use crate::runtime;

/// The runtime archive, compiled by this crate's build script and carried
/// inside the compiler so `lira build` needs nothing else on the machine.
const RUNTIME_ARCHIVE: &[u8] = include_bytes!(env!("LIRA_RT_ARCHIVE"));

/// Compile `program` to a native executable at `output`.
pub fn build(program: &Program, sema: &SemanticTables, output: &Path) -> CodegenResult<()> {
    let object = compile_object(program, sema)?;

    let dir = tempdir()?;
    let object_path = dir.join("lira_program.o");
    let archive_path = dir.join("liblira_rt.a");
    std::fs::write(&object_path, &object).map_err(|e| {
        CodegenError::link(format!("could not write {}: {}", object_path.display(), e))
    })?;
    std::fs::write(&archive_path, RUNTIME_ARCHIVE).map_err(|e| {
        CodegenError::link(format!("could not write {}: {}", archive_path.display(), e))
    })?;

    let result = link(&object_path, &archive_path, output);
    // Best-effort cleanup: a failed link is worth reporting, a failed rmdir is not.
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Compile `program` to an object file image, without linking.
pub fn compile_object(program: &Program, sema: &SemanticTables) -> CodegenResult<Vec<u8>> {
    let mut flag_builder = settings::builder();
    // Native executables are position-independent on every platform we target.
    flag_builder
        .set("is_pic", "true")
        .map_err(|e| CodegenError::internal(e.to_string()))?;
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| CodegenError::internal(e.to_string()))?;

    let isa = cranelift_native::builder()
        .map_err(|e| CodegenError::unsupported(format!("unsupported host: {}", e)))?
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError::internal(e.to_string()))?;

    let builder = ObjectBuilder::new(
        isa,
        "lira_program",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CodegenError::internal(e.to_string()))?;
    let mut module = ObjectModule::new(builder);

    let entry_id = {
        let mut lowerer = Lowerer::new(&mut module, program, sema)?;
        lowerer.lower_program(program)?
    };
    emit_c_main(&mut module, entry_id)?;

    let product = module.finish();
    product
        .emit()
        .map_err(|e| CodegenError::internal(format!("could not emit object file: {}", e)))
}

/// Emit the C `main` that boots the scheduler.
///
/// Native code cannot simply run the entry point on the process stack: a fiber
/// that blocks on a channel has to switch stacks, and the scheduler needs a
/// stack of its own to switch back to. So `main` is a two-line function that
/// hands the entry point to `lira_rt_boot` and returns whatever it reports.
fn emit_c_main(module: &mut ObjectModule, entry_id: FuncId) -> CodegenResult<()> {
    let pointer_ty = module.target_config().pointer_type();
    let call_conv = module.isa().default_call_conv();

    // `int main(int argc, char **argv)` — the arguments are forwarded to the
    // runtime so `env_args` has something to report.
    let mut main_sig = Signature::new(call_conv);
    main_sig.params.push(AbiParam::new(types::I32));
    main_sig.params.push(AbiParam::new(pointer_ty));
    main_sig.returns.push(AbiParam::new(types::I32));
    let main_id = module
        .declare_function("main", Linkage::Export, &main_sig)
        .map_err(|e| CodegenError::internal(e.to_string()))?;

    let boot_sig = runtime::signature("lira_rt_boot", call_conv, pointer_ty)?;
    let boot_id = module
        .declare_function("lira_rt_boot", Linkage::Import, &boot_sig)
        .map_err(|e| CodegenError::internal(e.to_string()))?;

    let set_args_sig = runtime::signature("lira_rt_set_args", call_conv, pointer_ty)?;
    let set_args_id = module
        .declare_function("lira_rt_set_args", Linkage::Import, &set_args_sig)
        .map_err(|e| CodegenError::internal(e.to_string()))?;

    let frontend_config = module.target_config();
    let mut ctx = module.make_context();
    ctx.func.signature = main_sig;
    let mut builder_ctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        let argc = builder.block_params(block)[0];
        let argv = builder.block_params(block)[1];

        let set_args_ref = module.declare_func_in_func(set_args_id, builder.func);
        builder.ins().call(set_args_ref, &[argc, argv]);

        let entry_ref = module.declare_func_in_func(entry_id, builder.func);
        let boot_ref = module.declare_func_in_func(boot_id, builder.func);
        let entry_addr = builder.ins().func_addr(pointer_ty, entry_ref);
        let null_env = builder.ins().iconst(pointer_ty, 0);
        let call = builder.ins().call(boot_ref, &[entry_addr, null_env]);
        let status = builder.inst_results(call)[0];
        builder.ins().return_(&[status]);

        builder.seal_all_blocks();
        builder.finalize(frontend_config);
    }
    module
        .define_function(main_id, &mut ctx)
        .map_err(|e| CodegenError::internal(format!("main: {}", e)))?;
    module.clear_context(&mut ctx);
    Ok(())
}

/// Link the object and the runtime archive into an executable.
fn link(object: &Path, archive: &Path, output: &Path) -> CodegenResult<()> {
    let linker = std::env::var("LIRA_CC")
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| "cc".to_string());

    let mut command = Command::new(&linker);
    command.arg(object).arg(archive).arg("-o").arg(output);
    if cfg!(target_os = "linux") {
        // The runtime uses libm for float formatting and pthreads for nothing
        // yet, but glibc wants them named explicitly on older toolchains.
        command.arg("-lm").arg("-lpthread");
    } else {
        command.arg("-lm");
    }

    let result = command.output().map_err(|e| {
        CodegenError::link(format!(
            "could not run the linker `{}`: {}. Set LIRA_CC to a working C compiler.",
            linker, e
        ))
    })?;

    if !result.status.success() {
        return Err(CodegenError::link(format!(
            "linking failed ({}):\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    Ok(())
}

/// A private scratch directory for the object file and runtime archive.
fn tempdir() -> CodegenResult<PathBuf> {
    // Enough uniqueness for concurrent builds in one process and across
    // processes, without pulling in a dependency for it.
    let unique = format!(
        "lira-build-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir)
        .map_err(|e| CodegenError::link(format!("could not create {}: {}", dir.display(), e)))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_archive_is_embedded() {
        // An empty archive would link but leave every runtime call undefined.
        assert!(RUNTIME_ARCHIVE.len() > 1024);
        assert!(RUNTIME_ARCHIVE.starts_with(b"!<arch>\n"));
    }
}
