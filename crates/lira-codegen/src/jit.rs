//! In-memory JIT lowering and trusted in-process execution.
//!
//! The whole program is compiled to machine code in this process and run
//! immediately. `liblira_rt` is already linked into the compiler binary, so the
//! JIT only has to tell Cranelift where each `lira_rt_*` symbol lives.

use cranelift_jit::{JITBuilder, JITModule};
use std::sync::{Mutex, MutexGuard};

use lirac::ast::Program;
use lirac::sema::SemanticTables;

use crate::error::{CodegenError, CodegenResult};
use crate::lower::Lowerer;

// The bundled native runtime owns process-global scheduler and collector
// state. Compilation itself may proceed concurrently, but executing two JIT
// modules at once would race those structures and let one module unregister
// the other's global roots.
static RUNTIME_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn lock_runtime() -> MutexGuard<'static, ()> {
    RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
            let mut symbols = vec![$((stringify!($name), $name as *const u8)),*];
            symbols.extend(rust_runtime_symbols());
            symbols
        }
    };
}

runtime_symbols!(
    lira_rt_abort,
    lira_rt_alloc,
    lira_rt_any_binary,
    lira_rt_any_bit_not,
    lira_rt_any_box_array,
    lira_rt_any_box_array_typed,
    lira_rt_any_box_bool,
    lira_rt_any_box_float,
    lira_rt_any_box_int,
    lira_rt_any_box_map,
    lira_rt_any_box_map_typed,
    lira_rt_any_box_object,
    lira_rt_any_box_object_typed,
    lira_rt_any_box_function,
    lira_rt_any_box_function_typed,
    lira_rt_any_box_channel,
    lira_rt_any_box_channel_typed,
    lira_rt_any_box_fiber,
    lira_rt_any_box_interface,
    lira_rt_any_box_ref,
    lira_rt_any_box_optional,
    lira_rt_any_box_string,
    lira_rt_any_copy,
    lira_rt_any_compare,
    lira_rt_any_from_slot,
    lira_rt_any_index,
    lira_rt_any_array_at,
    lira_rt_any_object_at,
    lira_rt_any_set,
    lira_rt_any_is,
    lira_rt_any_is_typed,
    lira_rt_any_len,
    lira_rt_any_neg,
    lira_rt_any_null,
    lira_rt_any_to_string,
    lira_rt_any_truthy,
    lira_rt_any_object_len,
    lira_rt_any_object_key_at,
    lira_rt_any_unbox_bool,
    lira_rt_any_unbox_float,
    lira_rt_any_unbox_int,
    lira_rt_any_unbox_ref,
    lira_rt_any_unbox_string,
    lira_rt_any_unbox_array,
    lira_rt_any_unbox_map,
    lira_rt_any_unbox_function,
    lira_rt_any_unbox_function_typed,
    lira_rt_any_unbox_interface,
    lira_rt_any_unbox_channel,
    lira_rt_any_unbox_channel_typed,
    lira_rt_any_unbox_object_typed,
    lira_rt_any_unbox_optional,
    lira_rt_any_cast_int,
    lira_rt_any_cast_float,
    lira_rt_any_cast_bool,
    lira_rt_any_push,
    lira_rt_any_pop,
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
    lira_rt_chan_try_recv,
    lira_rt_chan_try_send,
    lira_rt_collect,
    lira_rt_copy_ctx_free,
    lira_rt_copy_ctx_insert,
    lira_rt_copy_ctx_lookup,
    lira_rt_copy_ctx_new,
    lira_gc_register_root_slot,
    lira_gc_unregister_all_root_slots,
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
    lira_rt_interface_is,
    lira_rt_interface_method_slot,
    lira_rt_interface_new,
    lira_rt_interface_payload,
    lira_rt_interface_spec,
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
    lira_rt_math_fmod,
    lira_rt_math_ln,
    lira_rt_math_log10,
    lira_rt_math_log2,
    lira_rt_math_pow,
    lira_rt_math_round,
    lira_rt_math_sin,
    lira_rt_math_sinh,
    lira_rt_math_tan,
    lira_rt_math_tanh,
    lira_rt_map_get,
    lira_rt_map_has,
    lira_rt_map_keys,
    lira_rt_map_len,
    lira_rt_map_new,
    lira_rt_map_set,
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
    lira_rt_select,
    lira_rt_select_block,
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
    lira_rt_str_index,
    lira_rt_str_index_of,
    lira_rt_str_len,
    lira_rt_str_new,
    lira_rt_str_split,
    lira_rt_str_substring,
    lira_rt_str_to_lower,
    lira_rt_str_to_int,
    lira_rt_str_to_upper,
    lira_rt_str_trim,
    lira_rt_str_trim_end,
    lira_rt_str_trim_start,
    lira_rt_tcp_close,
    lira_rt_tcp_connect,
    lira_rt_tcp_read,
    lira_rt_tcp_write,
    lira_rt_time_components,
    lira_rt_time_format,
    lira_rt_time_format_iso,
    lira_rt_time_from_components,
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

fn rust_runtime_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        (
            "lira_rt_http_get",
            lira_native_runtime::lira_rt_http_get as *const u8,
        ),
        (
            "lira_rt_http_post",
            lira_native_runtime::lira_rt_http_post as *const u8,
        ),
        (
            "lira_rt_http_request",
            lira_native_runtime::lira_rt_http_request as *const u8,
        ),
        (
            "lira_rt_json_parse",
            lira_native_runtime::lira_rt_json_parse as *const u8,
        ),
        (
            "lira_rt_json_stringify",
            lira_native_runtime::lira_rt_json_stringify as *const u8,
        ),
        (
            "lira_rt_json_stringify_pretty",
            lira_native_runtime::lira_rt_json_stringify_pretty as *const u8,
        ),
        (
            "lira_rt_regex_captures",
            lira_native_runtime::lira_rt_regex_captures as *const u8,
        ),
        (
            "lira_rt_regex_find",
            lira_native_runtime::lira_rt_regex_find as *const u8,
        ),
        (
            "lira_rt_regex_find_all",
            lira_native_runtime::lira_rt_regex_find_all as *const u8,
        ),
        (
            "lira_rt_regex_is_valid",
            lira_native_runtime::lira_rt_regex_is_valid as *const u8,
        ),
        (
            "lira_rt_regex_match",
            lira_native_runtime::lira_rt_regex_match as *const u8,
        ),
        (
            "lira_rt_regex_replace",
            lira_native_runtime::lira_rt_regex_replace as *const u8,
        ),
        (
            "lira_rt_regex_replace_all",
            lira_native_runtime::lira_rt_regex_replace_all as *const u8,
        ),
        (
            "lira_rt_regex_split",
            lira_native_runtime::lira_rt_regex_split as *const u8,
        ),
    ]
}

extern "C" {
    /// Runs `entry` as fiber 0 and returns once no fiber can make progress.
    fn lira_rt_boot(entry: extern "C" fn(*mut u8), env: *mut u8) -> i32;
    /// Checks that no try-allocation path leaked an uncommitted reservation.
    fn lira_gc_validate_no_reservations() -> i32;
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
    let _runtime_guard = lock_runtime();

    // SAFETY: `entry` points at code Cranelift just compiled and finalised in
    // this process, with the `void(*)(void*)` signature `lower_entry` gives it.
    let exit_code = unsafe {
        let entry: extern "C" fn(*mut u8) = std::mem::transmute(entry);
        lira_rt_boot(entry, std::ptr::null_mut())
    };

    // Global cells point into this temporary JITModule. Drop those root slots
    // before releasing its mappings, then reclaim values owned only by them.
    let reservations_valid = unsafe {
        lira_gc_unregister_all_root_slots();
        lira_rt_collect();
        lira_gc_validate_no_reservations() != 0
    };

    // The program has returned and nothing holds a pointer into its code, so the
    // JIT's executable mappings can go.
    unsafe { module.free_memory() };
    if exit_code == 0 && !reservations_valid {
        return Err(CodegenError::internal(
            "native runtime leaked an allocation reservation",
        ));
    }
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::ffi::OsString;

    struct SelectSeedRestore(Option<OsString>);

    impl Drop for SelectSeedRestore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(seed) => std::env::set_var("LIRA_SELECT_SEED", seed),
                None => std::env::remove_var("LIRA_SELECT_SEED"),
            }
        }
    }

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

    #[test]
    fn sequential_jit_runs_reload_and_reset_the_select_seed() {
        let _restore = SelectSeedRestore(std::env::var_os("LIRA_SELECT_SEED"));
        let dir = std::env::temp_dir().join(format!("lira-jit-select-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create JIT select scratch directory");
        let source_path = dir.join("program.li");

        let first_wins = r#"
            fn main() {
                let ch: Channel<int> = chan(1)
                send(ch, 4)
                select {
                    first = <-ch => { if first != 4 { println(1 / 0) } }
                    second = <-ch => println(1 / 0)
                }
            }
        "#;
        let second_wins = r#"
            fn main() {
                let ch: Channel<int> = chan(1)
                send(ch, 4)
                select {
                    first = <-ch => println(1 / 0)
                    second = <-ch => { if second != 4 { println(1 / 0) } }
                }
            }
        "#;

        for (seed, source) in [("1", first_wins), ("2", second_wins), ("1", first_wins)] {
            std::env::set_var("LIRA_SELECT_SEED", seed);
            std::fs::write(&source_path, source).expect("write JIT select source");
            let status =
                crate::jit_run_in_process(source_path.to_str().expect("utf-8 path"), source);
            assert_eq!(status, Ok(0), "JIT select failed for seed {seed}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
