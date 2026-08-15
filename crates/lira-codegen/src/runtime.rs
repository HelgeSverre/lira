//! Declarations for the symbols exported by `liblira_rt`.
//!
//! The signatures here are the single source of truth on the Rust side; they
//! must stay in step with `runtime/lira_rt.h`. A mismatch would be a silent ABI
//! break, so the JIT re-exports the same list through `jit::runtime_symbols`
//! and the linker checks it again for AOT builds.

use cranelift_codegen::ir::{types, AbiParam, Signature, Type as ClifType};
use cranelift_codegen::isa::CallConv;

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

/// (symbol, parameters, result)
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
    let (_, params, ret) = RUNTIME
        .iter()
        .find(|(sym, _, _)| *sym == name)
        .ok_or_else(|| CodegenError::internal(format!("unknown runtime symbol `{}`", name)))?;

    let mut sig = Signature::new(call_conv);
    for param in *params {
        sig.params.push(AbiParam::new(param.clif(pointer_ty)));
    }
    if let Some(ret) = ret {
        sig.returns.push(AbiParam::new(ret.clif(pointer_ty)));
    }
    Ok(sig)
}

/// Every runtime symbol the backend may reference.
pub fn symbol_names() -> impl Iterator<Item = &'static str> {
    RUNTIME.iter().map(|(name, _, _)| *name)
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
}
