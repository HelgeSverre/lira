//! Ahead-of-time compilation: `lira build file.li -o app`.
//!
//! Produces a standalone native executable. Cranelift emits an object file, a
//! small generated `main` hands the entry point to the fiber scheduler, and the
//! system C compiler links the result against `liblira_rt` — which is embedded
//! in this binary, so no separate runtime install is needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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
/// Rust's exact regex implementation and its dependencies. This archive is
/// emitted by the workspace dependency and copied by `build.rs`, so AOT
/// executables do not depend on Cargo's target directory at runtime.
const NATIVE_RUNTIME_ARCHIVE: &[u8] = include_bytes!(env!("LIRA_NATIVE_RUNTIME_ARCHIVE"));

/// Compile `program` to a native executable at `output`.
pub fn build(program: &Program, sema: &SemanticTables, output: &Path) -> CodegenResult<()> {
    if cfg!(target_os = "windows") {
        return Err(CodegenError::unsupported(
            "the native backend does not support Windows",
        ));
    }
    let object = compile_object(program, sema)?;

    let dir = tempdir()?;
    let result = (|| {
        let object_path = dir.join("lira_program.o");
        let archive_path = dir.join("liblira_rt.a");
        let native_archive_path = dir.join("liblira_native_runtime.a");
        std::fs::write(&object_path, &object).map_err(|e| {
            CodegenError::link(format!("could not write {}: {}", object_path.display(), e))
        })?;
        std::fs::write(&archive_path, RUNTIME_ARCHIVE).map_err(|e| {
            CodegenError::link(format!("could not write {}: {}", archive_path.display(), e))
        })?;
        std::fs::write(&native_archive_path, NATIVE_RUNTIME_ARCHIVE).map_err(|e| {
            CodegenError::link(format!(
                "could not write {}: {}",
                native_archive_path.display(),
                e
            ))
        })?;

        link(&object_path, &native_archive_path, &archive_path, output)
    })();

    cleanup_tempdir(&dir, result)
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
fn link(
    object: &Path,
    native_archive: &Path,
    runtime_archive: &Path,
    output: &Path,
) -> CodegenResult<()> {
    let linker = std::env::var("LIRA_CC")
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| "cc".to_string());

    let mut command = Command::new(&linker);
    // Keep the object before the archives, and the C runtime after the Rust
    // archive: regex objects reference lira_rt_str_new/array helpers that the
    // second archive must satisfy in the same linker pass.
    command
        .arg(object)
        .arg(native_archive)
        .arg(runtime_archive)
        .arg("-o")
        .arg(output);
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
    // `create_dir` is intentionally exclusive: a pre-existing path must
    // never be reused for object/archive output. The counter handles
    // concurrent builds in one process; the timestamp and pid separate
    // processes without relying on a predictable single name.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id();
    create_tempdir(&std::env::temp_dir(), pid, nonce, &COUNTER)
}

fn create_tempdir(
    root: &Path,
    pid: u32,
    nonce: u128,
    counter: &AtomicU64,
) -> CodegenResult<PathBuf> {
    for _ in 0..128 {
        let sequence = counter.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(format!("lira-build-{pid}-{nonce:032x}-{sequence:016x}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CodegenError::link(format!(
                    "could not create {}: {}",
                    dir.display(),
                    error
                )))
            }
        }
    }
    Err(CodegenError::link(
        "could not allocate a unique native build directory after 128 attempts".to_owned(),
    ))
}

/// Remove the scratch directory owned by this build, retaining the build
/// result when cleanup succeeds and reporting both failures when it does not.
fn cleanup_tempdir(dir: &Path, result: CodegenResult<()>) -> CodegenResult<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => result,
        Err(cleanup_error) => match result {
            Ok(()) => Err(CodegenError::link(format!(
                "native build completed but could not remove scratch directory {}: {}",
                dir.display(),
                cleanup_error
            ))),
            Err(primary) => Err(append_cleanup_error(primary, dir, cleanup_error)),
        },
    }
}

fn append_cleanup_error(
    primary: CodegenError,
    dir: &Path,
    cleanup_error: std::io::Error,
) -> CodegenError {
    let detail = format!(
        "additionally could not remove scratch directory {}: {}",
        dir.display(),
        cleanup_error
    );
    match primary {
        CodegenError::Unsupported { message, span } => CodegenError::Unsupported {
            message: format!("{message}; {detail}"),
            span,
        },
        CodegenError::Internal(message) => CodegenError::Internal(format!("{message}; {detail}")),
        CodegenError::Link(message) => CodegenError::Link(format!("{message}; {detail}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_archive_is_embedded() {
        // An empty archive would link but leave every runtime call undefined.
        assert!(RUNTIME_ARCHIVE.len() > 1024);
        assert!(RUNTIME_ARCHIVE.starts_with(b"!<arch>\n"));
        assert!(NATIVE_RUNTIME_ARCHIVE.len() > 1024);
        assert!(NATIVE_RUNTIME_ARCHIVE.starts_with(b"!<arch>\n"));
    }

    #[test]
    fn native_build_directories_are_exclusive_under_concurrency() {
        let threads: Vec<_> = (0..16).map(|_| std::thread::spawn(tempdir)).collect();
        let dirs: Vec<PathBuf> = threads
            .into_iter()
            .map(|thread| {
                thread
                    .join()
                    .expect("tempdir thread completed")
                    .expect("tempdir created")
            })
            .collect();
        let mut names: Vec<_> = dirs.iter().map(|path| path.as_os_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), dirs.len());
        for dir in dirs {
            assert!(std::fs::create_dir(&dir).is_err());
            std::fs::remove_dir(&dir).expect("remove exclusive tempdir");
        }
    }

    #[test]
    fn native_build_directories_skip_preexisting_candidates() {
        let root = tempdir().expect("test root created");
        let counter = AtomicU64::new(0);
        let nonce = 0x1234_u128;
        let occupied = root.join(format!(
            "lira-build-7-{nonce:032x}-{sequence:016x}",
            sequence = 0
        ));
        std::fs::create_dir(&occupied).expect("pre-existing candidate created");

        let created = create_tempdir(&root, 7, nonce, &counter).expect("next candidate created");
        assert_ne!(created, occupied);
        assert!(occupied.is_dir(), "pre-existing directory was not reused");
        assert!(created.is_dir());

        std::fs::remove_dir_all(&root).expect("test directories removed");
    }

    #[test]
    fn native_build_directories_are_distinct_sequentially() {
        let root = tempdir().expect("test root created");
        let counter = AtomicU64::new(0);
        let dirs: Vec<_> = (0..16)
            .map(|_| create_tempdir(&root, 11, 0x55, &counter).expect("candidate created"))
            .collect();
        let mut unique = dirs.iter().collect::<Vec<_>>();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), dirs.len());

        std::fs::remove_dir_all(&root).expect("test directories removed");
    }

    #[test]
    fn cleanup_preserves_primary_error_and_reports_cleanup_failure() {
        let root = tempdir().expect("test root created");
        let primary_dir = root.join("primary");
        std::fs::create_dir(&primary_dir).expect("primary directory created");
        let primary = cleanup_tempdir(&primary_dir, Err(CodegenError::internal("linker failed")))
            .expect_err("primary error must remain an error");
        assert!(matches!(primary, CodegenError::Internal(message) if message == "linker failed"));
        assert!(!primary_dir.exists());

        let cleanup_dir = root.join("cleanup-failure");
        std::fs::create_dir(&cleanup_dir).expect("cleanup directory created");
        std::fs::remove_dir(&cleanup_dir).expect("simulate cleanup race");
        let cleanup = cleanup_tempdir(&cleanup_dir, Ok(())).expect_err("cleanup failure reported");
        assert!(cleanup
            .to_string()
            .contains("could not remove scratch directory"));

        std::fs::remove_dir_all(&root).expect("test root removed");
    }
}
