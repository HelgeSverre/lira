//! Declarations for the symbols exported by `liblira_rt`.
//!
//! The signatures here are the single source of truth on the Rust side; they
//! must stay in step with `runtime/lira_rt.h`. A mismatch would be a silent ABI
//! break, so the JIT re-exports the same list through `jit::runtime_symbols`
//! and the linker checks it again for AOT builds.

use cranelift_codegen::ir::{types, AbiParam, Signature, Type as ClifType};
use cranelift_codegen::isa::CallConv;
use lirac::checker::Type;

use crate::error::{CodegenError, CodegenResult};

/// Parameter and result shapes of a runtime entry point.
///
/// `P` stands for "pointer", resolved against the target's pointer width.
#[derive(Debug, Clone, Copy)]
enum Ty {
    I8,
    I32,
    I64,
    F64,
    P,
}

impl Ty {
    fn clif(self, pointer_ty: ClifType) -> ClifType {
        match self {
            Ty::I8 => types::I8,
            Ty::I32 => types::I32,
            Ty::I64 => types::I64,
            Ty::F64 => types::F64,
            Ty::P => pointer_ty,
        }
    }
}

/// Lira-visible types a built-in can take or return.
///
/// Everything here is expressed in the language's own types, so `lower.rs` can
/// coerce each argument to the declared type and hand the result back without a
/// per-builtin special case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sig {
    Int,
    Float,
    Bool,
    Str,
    /// `[string]`
    StrArray,
    /// An opaque handle — a channel, or a value the backend does not model.
    Any,
    Void,
}

impl Sig {
    /// The checker type a builtin argument is coerced to before the call.
    pub fn lira_type(self) -> Type {
        match self {
            Sig::Int => Type::Int,
            Sig::Float => Type::Float,
            Sig::Bool => Type::Bool,
            Sig::Str => Type::String,
            Sig::StrArray => Type::Array(Box::new(Type::String)),
            Sig::Any => Type::Any,
            Sig::Void => Type::Void,
        }
    }

    fn abi(self) -> Option<Ty> {
        match self {
            Sig::Int => Some(Ty::I64),
            Sig::Float => Some(Ty::F64),
            Sig::Bool => Some(Ty::I8),
            Sig::Str | Sig::StrArray | Sig::Any => Some(Ty::P),
            Sig::Void => None,
        }
    }
}

/// A built-in function callable from Lira source, and the runtime symbol that
/// implements it.
///
/// The bytecode VM reaches these through numbered syscalls; native code calls
/// them directly. Both must agree on behaviour — the parity tests are what hold
/// that line.
pub struct Builtin {
    pub name: &'static str,
    pub symbol: &'static str,
    pub params: &'static [Sig],
    pub ret: Sig,
}

macro_rules! builtins {
    ($(($name:literal, $symbol:literal, [$($param:ident),*], $ret:ident)),* $(,)?) => {
        pub const BUILTINS: &[Builtin] = &[
            $(Builtin {
                name: $name,
                symbol: $symbol,
                params: &[$(Sig::$param),*],
                ret: Sig::$ret,
            }),*
        ];
    };
}

builtins! {
    // ---- Math -----------------------------------------------------------
    // `sqrt`, `abs`, `floor`, `ceil` and `trunc` are lowered to Cranelift
    // instructions instead and never reach this table.
    ("pow", "lira_rt_math_pow", [Float, Float], Float),
    ("exp", "lira_rt_math_exp", [Float], Float),
    ("ln", "lira_rt_math_ln", [Float], Float),
    ("log10", "lira_rt_math_log10", [Float], Float),
    ("log2", "lira_rt_math_log2", [Float], Float),
    ("sin", "lira_rt_math_sin", [Float], Float),
    ("cos", "lira_rt_math_cos", [Float], Float),
    ("tan", "lira_rt_math_tan", [Float], Float),
    ("asin", "lira_rt_math_asin", [Float], Float),
    ("acos", "lira_rt_math_acos", [Float], Float),
    ("atan", "lira_rt_math_atan", [Float], Float),
    ("atan2", "lira_rt_math_atan2", [Float, Float], Float),
    ("sinh", "lira_rt_math_sinh", [Float], Float),
    ("cosh", "lira_rt_math_cosh", [Float], Float),
    ("tanh", "lira_rt_math_tanh", [Float], Float),
    ("round", "lira_rt_math_round", [Float], Float),

    // ---- Strings --------------------------------------------------------
    ("str_char_code", "lira_rt_str_char_code", [Str, Int], Int),
    ("str_from_char_code", "lira_rt_str_from_char_code", [Int], Str),
    ("str_to_upper", "lira_rt_str_to_upper", [Str], Str),
    ("str_to_lower", "lira_rt_str_to_lower", [Str], Str),
    ("str_substring", "lira_rt_str_substring", [Str, Int, Int], Str),
    ("str_index_of", "lira_rt_str_index_of", [Str, Str], Int),
    ("str_split", "lira_rt_str_split", [Str, Str], StrArray),
    ("str_trim", "lira_rt_str_trim", [Str], Str),
    ("str_trim_start", "lira_rt_str_trim_start", [Str], Str),
    ("str_trim_end", "lira_rt_str_trim_end", [Str], Str),

    // ---- Time -----------------------------------------------------------
    ("time_ms", "lira_rt_time_ms", [], Int),
    ("time_secs", "lira_rt_time_secs", [], Int),
    ("time_micros", "lira_rt_time_micros", [], Int),
    ("time_nanos", "lira_rt_time_nanos", [], Int),
    ("sleep", "lira_rt_sleep", [Int], Void),
    ("time_format_iso", "lira_rt_time_format_iso", [Int], Str),
    ("time_parse_iso", "lira_rt_time_parse_iso", [Str], Int),
    ("time_timezone_offset", "lira_rt_time_timezone_offset", [], Int),

    // ---- Random ---------------------------------------------------------
    ("random", "lira_rt_random", [], Float),
    ("random_int", "lira_rt_random_int", [Int, Int], Int),

    // ---- Environment ----------------------------------------------------
    ("env_get", "lira_rt_env_get", [Str], Str),
    ("env_set", "lira_rt_env_set", [Str, Str], Bool),
    ("env_remove", "lira_rt_env_remove", [Str], Bool),
    ("env_has", "lira_rt_env_has", [Str], Bool),
    ("env_args", "lira_rt_env_args", [], StrArray),
    ("env_all", "lira_rt_env_all", [], StrArray),
    ("env_keys", "lira_rt_env_keys", [], StrArray),
    ("env_exe", "lira_rt_env_exe", [], Str),
    ("env_temp_dir", "lira_rt_env_temp_dir", [], Str),
    ("env_home_dir", "lira_rt_env_home_dir", [], Str),

    // ---- Files ----------------------------------------------------------
    ("file_open", "lira_rt_file_open", [Str, Int], Int),
    ("file_read", "lira_rt_file_read", [Int, Int], Str),
    ("file_write", "lira_rt_file_write", [Int, Str], Int),
    ("file_close", "lira_rt_file_close", [Int], Bool),
    ("file_exists", "lira_rt_file_exists", [Str], Bool),
    ("file_size", "lira_rt_file_size", [Str], Int),
    ("file_seek", "lira_rt_file_seek", [Int, Int, Int], Int),

    // ---- Filesystem -----------------------------------------------------
    ("getcwd", "lira_rt_getcwd", [], Str),
    ("chdir", "lira_rt_chdir", [Str], Bool),
    ("mkdir", "lira_rt_mkdir", [Str], Bool),
    ("mkdir_all", "lira_rt_mkdir_all", [Str], Bool),
    ("rmdir", "lira_rt_rmdir", [Str], Bool),
    ("remove", "lira_rt_remove", [Str], Bool),
    ("remove_all", "lira_rt_remove_all", [Str], Bool),
    ("listdir", "lira_rt_listdir", [Str], StrArray),
    ("is_dir", "lira_rt_is_dir", [Str], Bool),
    ("is_file", "lira_rt_is_file", [Str], Bool),
    ("rename", "lira_rt_rename", [Str, Str], Bool),
    ("copy", "lira_rt_copy", [Str, Str], Bool),

    // ---- Encoding -------------------------------------------------------
    ("base64_encode", "lira_rt_base64_encode", [Str], Str),
    ("base64_decode", "lira_rt_base64_decode", [Str], Str),
    ("base64_encode_url", "lira_rt_base64_encode_url", [Str], Str),
    ("base64_decode_url", "lira_rt_base64_decode_url", [Str], Str),
    ("url_encode", "lira_rt_url_encode", [Str], Str),
    ("url_decode", "lira_rt_url_decode", [Str], Str),

    // ---- Hashing --------------------------------------------------------
    ("md5", "lira_rt_md5", [Str], Str),
    ("sha1", "lira_rt_sha1", [Str], Str),
    ("sha256", "lira_rt_sha256", [Str], Str),
    ("sha512", "lira_rt_sha512", [Str], Str),

    // ---- UUID -----------------------------------------------------------
    ("uuid_v4", "lira_rt_uuid_v4", [], Str),
    ("uuid_v7", "lira_rt_uuid_v7", [], Str),
    ("uuid_is_valid", "lira_rt_uuid_is_valid", [Str], Bool),
    ("uuid_nil", "lira_rt_uuid_nil", [], Str),

    // ---- Network --------------------------------------------------------
    ("tcp_connect", "lira_rt_tcp_connect", [Str, Int], Int),
    ("tcp_write", "lira_rt_tcp_write", [Int, Str], Int),
    ("tcp_read", "lira_rt_tcp_read", [Int, Int], Str),
    ("tcp_close", "lira_rt_tcp_close", [Int], Bool),
    ("dns_lookup", "lira_rt_dns_lookup", [Str], Str),
}

/// Look up a built-in by the name it has in Lira source.
pub fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name == name)
}

/// (symbol, parameters, result) for the runtime entry points the code generator
/// calls directly, rather than on behalf of a Lira built-in.
const RUNTIME: &[(&str, &[Ty], Option<Ty>)] = &[
    ("lira_rt_alloc", &[Ty::I64, Ty::I32], Some(Ty::P)),
    ("lira_rt_abort", &[Ty::P], None),
    // Strings
    ("lira_rt_str_new", &[Ty::P, Ty::I64], Some(Ty::P)),
    ("lira_rt_str_concat", &[Ty::P, Ty::P], Some(Ty::P)),
    ("lira_rt_str_len", &[Ty::P], Some(Ty::I64)),
    ("lira_rt_str_eq", &[Ty::P, Ty::P], Some(Ty::I8)),
    ("lira_rt_str_cmp", &[Ty::P, Ty::P], Some(Ty::I64)),
    ("lira_rt_int_to_str", &[Ty::I64], Some(Ty::P)),
    ("lira_rt_float_to_str", &[Ty::F64], Some(Ty::P)),
    ("lira_rt_bool_to_str", &[Ty::I8], Some(Ty::P)),
    // Printing
    ("lira_rt_print_str", &[Ty::P], None),
    ("lira_rt_println_str", &[Ty::P], None),
    ("lira_rt_print_int", &[Ty::I64], None),
    ("lira_rt_println_int", &[Ty::I64], None),
    ("lira_rt_print_float", &[Ty::F64], None),
    ("lira_rt_println_float", &[Ty::F64], None),
    ("lira_rt_print_bool", &[Ty::I8], None),
    ("lira_rt_println_bool", &[Ty::I8], None),
    // Arrays
    ("lira_rt_array_new", &[Ty::I64], Some(Ty::P)),
    ("lira_rt_array_push", &[Ty::P, Ty::I64], None),
    ("lira_rt_array_pop", &[Ty::P], Some(Ty::I64)),
    ("lira_rt_array_get", &[Ty::P, Ty::I64], Some(Ty::I64)),
    ("lira_rt_array_set", &[Ty::P, Ty::I64, Ty::I64], None),
    ("lira_rt_array_len", &[Ty::P], Some(Ty::I64)),
    // Arithmetic that needs a trap check
    ("lira_rt_idiv", &[Ty::I64, Ty::I64], Some(Ty::I64)),
    ("lira_rt_imod", &[Ty::I64, Ty::I64], Some(Ty::I64)),
    ("lira_rt_ipow", &[Ty::I64, Ty::I64], Some(Ty::I64)),
    // Fibers and channels
    ("lira_rt_boot", &[Ty::P, Ty::P], Some(Ty::I32)),
    // Handed argc/argv by the generated `main` so `env_args` can report them.
    ("lira_rt_set_args", &[Ty::I32, Ty::P], None),
    ("lira_rt_spawn", &[Ty::P, Ty::P], Some(Ty::I64)),
    ("lira_rt_yield", &[], None),
    ("lira_rt_fiber_id", &[], Some(Ty::I64)),
    ("lira_rt_chan_new", &[Ty::I64], Some(Ty::P)),
    ("lira_rt_chan_send", &[Ty::P, Ty::I64], None),
    ("lira_rt_chan_recv", &[Ty::P], Some(Ty::I64)),
    ("lira_rt_chan_close", &[Ty::P], None),
];

/// Build the Cranelift signature of a runtime symbol.
pub fn signature(
    name: &str,
    call_conv: CallConv,
    pointer_ty: ClifType,
) -> CodegenResult<Signature> {
    let (params, ret): (Vec<Ty>, Option<Ty>) =
        if let Some((_, params, ret)) = RUNTIME.iter().find(|(sym, _, _)| *sym == name) {
            (params.to_vec(), *ret)
        } else if let Some(builtin) = BUILTINS.iter().find(|b| b.symbol == name) {
            (
                builtin.params.iter().filter_map(|p| p.abi()).collect(),
                builtin.ret.abi(),
            )
        } else {
            return Err(CodegenError::internal(format!(
                "unknown runtime symbol `{}`",
                name
            )));
        };

    let mut sig = Signature::new(call_conv);
    for param in params {
        sig.params.push(AbiParam::new(param.clif(pointer_ty)));
    }
    if let Some(ret) = ret {
        sig.returns.push(AbiParam::new(ret.clif(pointer_ty)));
    }
    Ok(sig)
}

/// Every runtime symbol the backend may reference.
pub fn symbol_names() -> impl Iterator<Item = &'static str> {
    RUNTIME
        .iter()
        .map(|(name, _, _)| *name)
        .chain(BUILTINS.iter().map(|b| b.symbol))
}

/// Object kind tags stored in `LiraHeader.kind`. Mirrors `enum LiraKind`.
pub const KIND_STRING: i64 = 1;
pub const KIND_ARRAY: i64 = 2;
pub const KIND_STRUCT: i64 = 3;
pub const KIND_ENUM: i64 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_resolve_pointer_width() {
        let sig = signature("lira_rt_str_concat", CallConv::SystemV, types::I64).unwrap();
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(sig.returns[0].value_type, types::I64);
    }

    #[test]
    fn void_runtime_calls_have_no_results() {
        let sig = signature("lira_rt_println_int", CallConv::SystemV, types::I64).unwrap();
        assert!(sig.returns.is_empty());
    }

    #[test]
    fn unknown_symbols_are_an_internal_error() {
        assert!(signature("lira_rt_nope", CallConv::SystemV, types::I64).is_err());
    }

    #[test]
    fn builtin_signatures_follow_their_declared_types() {
        let sig = signature("lira_rt_str_substring", CallConv::SystemV, types::I64).unwrap();
        assert_eq!(sig.params.len(), 3);
        assert_eq!(sig.returns.len(), 1);

        let void = signature("lira_rt_sleep", CallConv::SystemV, types::I64).unwrap();
        assert!(void.returns.is_empty());
    }

    #[test]
    fn builtin_names_and_symbols_are_unique() {
        let mut names: Vec<&str> = BUILTINS.iter().map(|b| b.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate built-in name");

        let mut symbols: Vec<&str> = BUILTINS.iter().map(|b| b.symbol).collect();
        symbols.sort_unstable();
        let count = symbols.len();
        symbols.dedup();
        assert_eq!(symbols.len(), count, "duplicate runtime symbol");
    }
}

#[cfg(test)]
mod checker_parity {
    use super::*;
    use lirac::checker::TypeEnv;

    /// The native ABI has to agree with the types the checker hands the program.
    ///
    /// A mismatch would not be caught anywhere else: the checker would type
    /// `str_split(...)` as `[string]` while the backend called a symbol
    /// returning something else, and the result would be a wrong value rather
    /// than a compile error.
    #[test]
    fn every_builtin_matches_the_checkers_declared_signature() {
        let env = TypeEnv::new();
        let mut problems = Vec::new();

        for builtin in BUILTINS {
            let Some(symbol) = env.lookup(builtin.name) else {
                problems.push(format!("`{}` is not a checker built-in", builtin.name));
                continue;
            };
            let Type::Function {
                params,
                return_type,
                ..
            } = &symbol.ty
            else {
                problems.push(format!("`{}` is not a function", builtin.name));
                continue;
            };

            if params.len() != builtin.params.len() {
                problems.push(format!(
                    "`{}` takes {} argument(s) in the checker, {} here",
                    builtin.name,
                    params.len(),
                    builtin.params.len()
                ));
                continue;
            }
            for (index, (checked, declared)) in params.iter().zip(builtin.params).enumerate() {
                if !compatible(checked, declared.lira_type()) {
                    problems.push(format!(
                        "`{}` argument {}: checker says `{}`, the backend passes `{}`",
                        builtin.name,
                        index,
                        checked.display_name(),
                        declared.lira_type().display_name()
                    ));
                }
            }
            if !compatible(return_type, builtin.ret.lira_type()) {
                problems.push(format!(
                    "`{}` returns `{}` in the checker, `{}` here",
                    builtin.name,
                    return_type.display_name(),
                    builtin.ret.lira_type().display_name()
                ));
            }
        }

        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    /// Whether the backend's declared type can stand in for the checker's.
    ///
    /// `Any` matches anything by construction, and `T?` is the same pointer as
    /// `T` at run time — `env_get` is declared `string?` and handled as a
    /// string.
    fn compatible(checked: &Type, declared: Type) -> bool {
        if matches!(checked, Type::Any) || matches!(declared, Type::Any) {
            return true;
        }
        if let Type::Optional(inner) = checked {
            return compatible(inner, declared);
        }
        *checked == declared
    }
}
