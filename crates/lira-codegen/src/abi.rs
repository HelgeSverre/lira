//! Mapping Lira's checked types onto Cranelift's machine types.
//!
//! Lira is statically typed, so nothing here is a guess: an `int` is an `i64`
//! register, a `float` is an `f64` register, a `bool` is an `i8`, and every
//! heap value is a plain pointer. There are no tag bits, no NaN boxing, and no
//! unboxing guards in the emitted code.

use cranelift_codegen::ir::{types, Type as ClifType};
use lirac::checker::Type;

use crate::error::{CodegenError, CodegenResult};

/// How a Lira value is held in a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repr {
    /// 64-bit signed/unsigned integer. Narrower integer types widen into this.
    Int,
    /// IEEE-754 double.
    Float,
    /// 0 or 1 in an i8.
    Bool,
    /// A pointer to a heap object with a `LiraHeader`.
    Ref,
    /// No value at all.
    Void,
}

impl Repr {
    /// The Cranelift type for this representation. `Void` has none.
    pub fn clif(self, pointer_ty: ClifType) -> Option<ClifType> {
        match self {
            Repr::Int => Some(types::I64),
            Repr::Float => Some(types::F64),
            Repr::Bool => Some(types::I8),
            Repr::Ref => Some(pointer_ty),
            Repr::Void => None,
        }
    }

    pub fn is_ref(self) -> bool {
        matches!(self, Repr::Ref)
    }
}

/// Classify a checked type into its register representation.
pub fn repr_of(ty: &Type) -> CodegenResult<Repr> {
    Ok(match ty {
        Type::Int
        | Type::Int8
        | Type::Int16
        | Type::Int32
        | Type::Int64
        | Type::UInt8
        | Type::UInt16
        | Type::UInt32
        // A `char` is an integer code point at run time, matching the bytecode
        // VM, which has no separate character value and prints `'a'` as `97`.
        | Type::Char
        | Type::UInt64 => Repr::Int,
        Type::Float => Repr::Float,
        Type::Bool => Repr::Bool,
        Type::Void => Repr::Void,
        Type::String
        | Type::Array(_)
        | Type::Map(_, _)
        | Type::Struct(_)
        | Type::Class(_)
        | Type::Enum(_)
        | Type::Null => Repr::Ref,
        // A channel handle comes back from the checker as `Any`, and so does an
        // erased generic. Both are pointer-shaped at runtime.
        Type::Any | Type::TypeParam(_) | Type::Interface(_) => Repr::Ref,
        // `T?` is a nullable pointer, which only works when `T` is already
        // pointer-shaped; a null `int` has no bit pattern to spare.
        Type::Optional(inner) => {
            let inner_repr = repr_of(inner)?;
            if !inner_repr.is_ref() {
                return Err(CodegenError::unsupported(format!(
                    "optional `{}?` wraps a non-reference type; the native backend has no niche for it yet",
                    inner.display_name()
                )));
            }
            Repr::Ref
        }
        Type::Tuple(_) => {
            return Err(CodegenError::unsupported(
                "tuples are not lowered by the native backend yet",
            ))
        }
        Type::Result { .. } => {
            return Err(CodegenError::unsupported(
                "`Result` values are not lowered by the native backend yet",
            ))
        }
        Type::Function { .. } => {
            return Err(CodegenError::unsupported(
                "first-class functions and lambdas are not lowered by the native backend yet",
            ))
        }
        // An unconstrained type variable is what an empty literal such as
        // `let xs = []` leaves behind: the bytecode VM can defer the decision to
        // run time, native code cannot.
        Type::TypeVar(_) | Type::Unknown => {
            return Err(CodegenError::unsupported(
                "this value's type is not pinned down; add an annotation such as `let xs: [int] = []`",
            ))
        }
    })
}

/// True when the type's values are unsigned, which decides between `sextend`
/// and `uextend` on a narrow load and between signed and unsigned comparisons.
pub fn is_unsigned(ty: &Type) -> bool {
    matches!(
        ty,
        Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64 | Type::Char
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_map_to_hardware_types() {
        assert_eq!(repr_of(&Type::Int).unwrap(), Repr::Int);
        assert_eq!(repr_of(&Type::Float).unwrap(), Repr::Float);
        assert_eq!(repr_of(&Type::Bool).unwrap(), Repr::Bool);
        assert_eq!(repr_of(&Type::Void).unwrap(), Repr::Void);
    }

    #[test]
    fn narrow_integers_widen_to_i64_registers() {
        for ty in [
            Type::Int8,
            Type::UInt16,
            Type::Int32,
            Type::UInt64,
            Type::Char,
        ] {
            assert_eq!(repr_of(&ty).unwrap(), Repr::Int);
            assert_eq!(repr_of(&ty).unwrap().clif(types::I64), Some(types::I64));
        }
    }

    #[test]
    fn heap_types_are_pointers() {
        assert!(repr_of(&Type::String).unwrap().is_ref());
        assert!(repr_of(&Type::Array(Box::new(Type::Int))).unwrap().is_ref());
        assert!(repr_of(&Type::Struct("P".into())).unwrap().is_ref());
        assert!(repr_of(&Type::Enum("E".into())).unwrap().is_ref());
    }

    #[test]
    fn optional_scalars_are_rejected_rather_than_mis_lowered() {
        assert!(repr_of(&Type::Optional(Box::new(Type::Int))).is_err());
        assert!(repr_of(&Type::Optional(Box::new(Type::String))).is_ok());
    }
}
