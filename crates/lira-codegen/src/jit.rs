//! In-memory JIT execution: `lira jit file.li`.
//!
//! The whole program is compiled to machine code in this process and run
//! immediately. `liblira_rt` is already linked into the compiler binary, so the
//! JIT only has to tell Cranelift where each `lira_rt_*` symbol lives.

use cranelift_jit::{JITBuilder, JITModule};

use lirac::ast::Program;
use lirac::sema::SemanticTables;

use crate::error::{CodegenError, CodegenResult};
use crate::lower::Lowerer;

/// Declare the runtime symbols and build the JIT's symbol table from the same
/// list, so the two cannot drift apart.
///
/// The declarations are deliberately signature-free: nothing in Rust calls
/// these, the addresses are only handed to Cranelift, which then calls them
/// through the signatures in [`crate::runtime`]. `lira_rt_boot` is the one
/// exception and is declared properly below.
macro_rules! runtime_symbols {
    ($($name:ident),* $(,)?) => {
        extern "C" {
            $(fn $name();)*
        }

        fn runtime_symbols() -> Vec<(&'static str, *const u8)> {
            vec![$((stringify!($name), $name as *const u8)),*]
        }
    };
}

runtime_symbols!(
    lira_rt_abort,
    lira_rt_alloc,
    lira_rt_array_get,
    lira_rt_array_len,
    lira_rt_array_new,
    lira_rt_array_pop,
    lira_rt_array_push,
    lira_rt_array_set,
    lira_rt_base64_decode,
    lira_rt_base64_decode_url,
    lira_rt_base64_encode,
    lira_rt_base64_encode_url,
    lira_rt_bool_to_str,
    lira_rt_chan_close,
    lira_rt_chan_new,
    lira_rt_chan_recv,
    lira_rt_chan_send,
    lira_rt_chdir,
    lira_rt_copy,
    lira_rt_dns_lookup,
    lira_rt_env_all,
    lira_rt_env_args,
    lira_rt_env_exe,
    lira_rt_env_get,
    lira_rt_env_has,
    lira_rt_env_home_dir,
    lira_rt_env_keys,
    lira_rt_env_remove,
    lira_rt_env_set,
    lira_rt_env_temp_dir,
    lira_rt_fiber_id,
    lira_rt_file_close,
    lira_rt_file_exists,
    lira_rt_file_open,
    lira_rt_file_read,
    lira_rt_file_seek,
    lira_rt_file_size,
    lira_rt_file_write,
    lira_rt_float_to_str,
    lira_rt_getcwd,
    lira_rt_idiv,
    lira_rt_imod,
    lira_rt_int_to_str,
    lira_rt_ipow,
    lira_rt_is_dir,
    lira_rt_is_file,
    lira_rt_listdir,
    lira_rt_math_acos,
    lira_rt_math_asin,
    lira_rt_math_atan,
    lira_rt_math_atan2,
    lira_rt_math_cos,
    lira_rt_math_cosh,
    lira_rt_math_exp,
    lira_rt_math_ln,
    lira_rt_math_log10,
    lira_rt_math_log2,
    lira_rt_math_pow,
    lira_rt_math_round,
    lira_rt_math_sin,
    lira_rt_math_sinh,
    lira_rt_math_tan,
    lira_rt_math_tanh,
    lira_rt_md5,
    lira_rt_mkdir,
    lira_rt_mkdir_all,
    lira_rt_print_bool,
    lira_rt_print_float,
    lira_rt_print_int,
    lira_rt_print_str,
    lira_rt_println_bool,
    lira_rt_println_float,
    lira_rt_println_int,
    lira_rt_println_str,
    lira_rt_random,
    lira_rt_random_int,
    lira_rt_remove,
    lira_rt_remove_all,
    lira_rt_rename,
    lira_rt_rmdir,
    lira_rt_set_args,
    lira_rt_sha1,
    lira_rt_sha256,
    lira_rt_sha512,
    lira_rt_sleep,
    lira_rt_spawn,
    lira_rt_str_char_code,
    lira_rt_str_cmp,
    lira_rt_str_concat,
    lira_rt_str_eq,
    lira_rt_str_from_char_code,
    lira_rt_str_index_of,
    lira_rt_str_len,
    lira_rt_str_new,
    lira_rt_str_split,
    lira_rt_str_substring,
    lira_rt_str_to_lower,
    lira_rt_str_to_upper,
    lira_rt_str_trim,
    lira_rt_str_trim_end,
    lira_rt_str_trim_start,
    lira_rt_tcp_close,
    lira_rt_tcp_connect,
    lira_rt_tcp_read,
    lira_rt_tcp_write,
    lira_rt_time_format_iso,
    lira_rt_time_micros,
    lira_rt_time_ms,
    lira_rt_time_nanos,
    lira_rt_time_parse_iso,
    lira_rt_time_secs,
    lira_rt_time_timezone_offset,
    lira_rt_url_decode,
    lira_rt_url_encode,
    lira_rt_uuid_is_valid,
    lira_rt_uuid_nil,
    lira_rt_uuid_v4,
    lira_rt_uuid_v7,
    lira_rt_yield,
);

extern "C" {
    /// Runs `entry` as fiber 0 and returns once no fiber can make progress.
    fn lira_rt_boot(entry: extern "C" fn(*mut u8), env: *mut u8) -> i32;
}

/// Compile `program` in memory and run it. Returns the process exit code.
pub fn run(program: &Program, sema: &SemanticTables) -> CodegenResult<i32> {
    let mut builder = JITBuilder::new(cranelift_module::default_libcall_names())
        .map_err(|e| CodegenError::internal(e.to_string()))?;
    for (name, address) in runtime_symbols() {
        builder.symbol(name, address);
    }
    // `lira_rt_boot` is called from Rust, but generated code may reference it
    // too; registering it keeps both paths on the same implementation.
    builder.symbol("lira_rt_boot", lira_rt_boot as *const u8);

    let mut module = JITModule::new(builder);
    let entry_id = {
        let mut lowerer = Lowerer::new(&mut module, program, sema)?;
        lowerer.lower_program(program)?
    };

    module
        .finalize_definitions()
        .map_err(|e| CodegenError::internal(e.to_string()))?;
    let entry = module.get_finalized_function(entry_id);

    // SAFETY: `entry` points at code Cranelift just compiled and finalised in
    // this process, with the `void(*)(void*)` signature `lower_entry` gives it.
    let exit_code = unsafe {
        let entry: extern "C" fn(*mut u8) = std::mem::transmute(entry);
        lira_rt_boot(entry, std::ptr::null_mut())
    };

    // The program has returned and nothing holds a pointer into its code, so the
    // JIT's executable mappings can go.
    unsafe { module.free_memory() };
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn jit_symbol_table_covers_every_declared_runtime_call() {
        let registered: HashSet<&str> = runtime_symbols().into_iter().map(|(n, _)| n).collect();
        for name in crate::runtime::symbol_names() {
            // `lira_rt_boot` is registered separately, outside the macro list.
            if name == "lira_rt_boot" {
                continue;
            }
            assert!(
                registered.contains(name),
                "runtime symbol `{}` is declared for codegen but has no JIT address",
                name
            );
        }
    }

    #[test]
    fn runtime_addresses_resolve() {
        for (name, address) in runtime_symbols() {
            assert!(!address.is_null(), "`{}` resolved to a null address", name);
        }
    }
}
