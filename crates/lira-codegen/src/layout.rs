//! Memory layout for Lira's aggregate types.
//!
//! The bytecode VM stores every value as a tagged `Value`, so a struct is a map
//! of names to boxes. Native code does not need any of that: because the checker
//! has already proved every field's type, `Point { x: int, y: int }` becomes a
//! header followed by two 8-byte slots, and `pt.x` becomes a single load at a
//! constant offset.
//!
//! Every heap object begins with the 16-byte `LiraHeader` from
//! `runtime/lira_rt.h`; field offsets below are measured from the start of the
//! object, so they already include that header.

use std::collections::HashMap;

use lirac::ast::{Program, Statement, StatementKind};
use lirac::checker::Type;

use crate::error::{CodegenError, CodegenResult};

/// Size of `LiraHeader`. Must match `LIRA_HEADER_SIZE` in `runtime/lira_rt.h`.
pub const HEADER_SIZE: i32 = 16;

/// Offsets into the runtime's built-in objects. Mirrored from `lira_rt.h`.
pub const STR_LEN_OFFSET: i32 = 16;
pub const ARRAY_LEN_OFFSET: i32 = 16;
pub const ENUM_TAG_OFFSET: i32 = 16;
pub const ENUM_PAYLOAD_OFFSET: i32 = 24;

/// A closure object: header, the code pointer, the capture count, then one
/// 8-byte cell per captured value.
///
/// The code it points at always takes the closure itself as its first argument,
/// whether or not it captures anything, so every call site is identical.
pub const CLOSURE_CODE_OFFSET: i32 = 16;
pub const CLOSURE_COUNT_OFFSET: i32 = 24;
pub const CLOSURE_CAPTURES_OFFSET: i32 = 32;

/// Every enum payload slot and every array element is a uniform 8-byte cell:
/// floats are bit-cast into it and references are stored directly.
pub const SLOT_SIZE: i32 = 8;

/// A single field within a struct or class.
#[derive(Debug, Clone)]
pub struct FieldLayout {
    pub name: String,
    pub ty: Type,
    /// Byte offset from the start of the object, including the header.
    pub offset: i32,
    /// Storage width in bytes, which can be narrower than the register width.
    pub size: i32,
}

/// A struct or class laid out in memory.
#[derive(Debug, Clone)]
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<FieldLayout>,
    /// Total allocation size, rounded up to the type's alignment.
    pub size: i32,
}

impl StructLayout {
    pub fn field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// One variant of an enum. Payloads live in 8-byte slots after the tag.
#[derive(Debug, Clone)]
pub struct VariantLayout {
    pub name: String,
    pub tag: i64,
    pub field_types: Vec<Type>,
}

/// A tagged union: header, an i64 discriminant, then the widest variant's slots.
#[derive(Debug, Clone)]
pub struct EnumLayout {
    pub name: String,
    pub variants: Vec<VariantLayout>,
    pub size: i32,
}

impl EnumLayout {
    pub fn variant(&self, name: &str) -> Option<&VariantLayout> {
        self.variants.iter().find(|v| v.name == name)
    }
}

/// The built-in struct a `a..b` expression evaluates to, mirroring the object
/// the bytecode backend builds. Iterating a range reads these fields back.
pub const RANGE_TYPE: &str = "Range";

/// All aggregate layouts in a program, keyed by type name.
#[derive(Debug, Default, Clone)]
pub struct LayoutMap {
    pub structs: HashMap<String, StructLayout>,
    pub enums: HashMap<String, EnumLayout>,
    /// `type Name = ...` declarations, unresolved.
    pub aliases: HashMap<String, Type>,
}

impl LayoutMap {
    /// Walk the program and compute a layout for every struct, class and enum.
    ///
    /// Field types never need a layout of their own to be sized: an aggregate
    /// field is a pointer, so recursive and mutually recursive types fall out
    /// for free and declaration order does not matter.
    pub fn build(program: &Program) -> CodegenResult<Self> {
        let mut map = LayoutMap::default();
        // Seeded first so a user declaration of the same name replaces it and
        // `range_layout_is_usable` can then report the clash.
        map.structs.insert(
            RANGE_TYPE.to_string(),
            layout_fields(
                RANGE_TYPE.to_string(),
                &[
                    ("start".to_string(), Type::Int),
                    ("end".to_string(), Type::Int),
                    ("inclusive".to_string(), Type::Bool),
                ],
            ),
        );
        collect(&program.statements, &mut map)?;
        Ok(map)
    }

    /// Whether the `Range` layout is still the built-in one. A program that
    /// declares its own `struct Range` shadows it, and `a..b` can no longer be
    /// lowered against it.
    pub fn range_layout_is_usable(&self) -> bool {
        self.structs.get(RANGE_TYPE).is_some_and(|layout| {
            layout
                .field("start")
                .is_some_and(|f| matches!(f.ty, Type::Int))
                && layout
                    .field("end")
                    .is_some_and(|f| matches!(f.ty, Type::Int))
                && layout
                    .field("inclusive")
                    .is_some_and(|f| matches!(f.ty, Type::Bool))
        })
    }

    /// Follow `type A = B` chains to the type they finally name.
    pub fn resolve_alias(&self, name: &str) -> Option<Type> {
        let mut current = self.aliases.get(name)?.clone();
        // Bounded so a cyclic alias reports nothing instead of hanging; the
        // checker rejects those anyway.
        for _ in 0..16 {
            let Type::Struct(next) = &current else {
                return Some(current);
            };
            match self.aliases.get(next) {
                Some(target) => current = target.clone(),
                None => return Some(current),
            }
        }
        None
    }

    pub fn is_aggregate(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }
}

fn collect(statements: &[Statement], map: &mut LayoutMap) -> CodegenResult<()> {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::StructDecl { name, fields, .. } => {
                let named: Vec<(String, Type)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), type_of_ann(&f.type_ann)))
                    .collect();
                map.structs
                    .insert(name.clone(), layout_fields(name.clone(), &named));
            }
            StatementKind::ClassDecl {
                name,
                parent,
                fields,
                ..
            } => {
                if parent.is_some() {
                    // Inherited fields would have to be prefixed onto the child's
                    // layout, and virtual dispatch needs a vtable. Neither is
                    // wired up yet, so refuse instead of laying out a class whose
                    // parent fields silently go missing.
                    return Err(CodegenError::unsupported(format!(
                        "class `{}` uses inheritance, which the native backend does not support yet",
                        name
                    )));
                }
                let named: Vec<(String, Type)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), type_of_ann(&f.type_ann)))
                    .collect();
                map.structs
                    .insert(name.clone(), layout_fields(name.clone(), &named));
            }
            StatementKind::EnumDecl { name, variants, .. } => {
                let mut laid_out = Vec::with_capacity(variants.len());
                let mut widest = 0usize;
                for (tag, variant) in variants.iter().enumerate() {
                    let field_types: Vec<Type> = variant.fields.iter().map(type_of_ann).collect();
                    widest = widest.max(field_types.len());
                    laid_out.push(VariantLayout {
                        name: variant.name.clone(),
                        tag: tag as i64,
                        field_types,
                    });
                }
                map.enums.insert(
                    name.clone(),
                    EnumLayout {
                        name: name.clone(),
                        variants: laid_out,
                        size: ENUM_PAYLOAD_OFFSET + SLOT_SIZE * widest as i32,
                    },
                );
            }
            StatementKind::TypeAlias { name, type_expr } => {
                map.aliases.insert(name.clone(), type_of_ann(type_expr));
            }
            StatementKind::Block(block) => collect(&block.statements, map)?,
            _ => {}
        }
    }
    Ok(())
}

/// Lay fields out in declaration order with natural C alignment.
fn layout_fields(name: String, fields: &[(String, Type)]) -> StructLayout {
    let mut offset = HEADER_SIZE;
    let mut max_align = 8; // the header itself is 8-byte aligned
    let mut laid_out = Vec::with_capacity(fields.len());

    for (field_name, ty) in fields {
        let size = storage_size(ty);
        let align = size.clamp(1, 8);
        max_align = max_align.max(align);
        offset = align_to(offset, align);
        laid_out.push(FieldLayout {
            name: field_name.clone(),
            ty: ty.clone(),
            offset,
            size,
        });
        offset += size;
    }

    StructLayout {
        name,
        fields: laid_out,
        size: align_to(offset, max_align),
    }
}

fn align_to(value: i32, align: i32) -> i32 {
    debug_assert!(align > 0 && (align & (align - 1)) == 0);
    (value + align - 1) & !(align - 1)
}

/// In-memory width of a type. Registers always widen integers to 64 bits; this
/// is only about how many bytes a field or slot occupies.
pub fn storage_size(ty: &Type) -> i32 {
    match ty {
        Type::Bool => 1,
        Type::Int8 | Type::UInt8 => 1,
        Type::Int16 | Type::UInt16 => 2,
        Type::Int32 | Type::UInt32 | Type::Char => 4,
        // Everything else is either a 64-bit scalar or a pointer.
        _ => 8,
    }
}

/// Resolve an AST type annotation into a checker `Type`.
///
/// The checker already resolved these for expressions, but field annotations
/// are consulted directly while laying types out, before any expression has
/// been visited.
pub fn type_of_ann(ann: &lirac::ast::TypeExpr) -> Type {
    use lirac::ast::TypeExprKind;
    match &ann.kind {
        TypeExprKind::Named(name) => named_type(name),
        TypeExprKind::Generic { name, args } => match name.as_str() {
            "Array" | "List" if args.len() == 1 => Type::Array(Box::new(type_of_ann(&args[0]))),
            _ => named_type(name),
        },
        TypeExprKind::Optional(inner) => Type::Optional(Box::new(type_of_ann(inner))),
        TypeExprKind::Array(inner) => Type::Array(Box::new(type_of_ann(inner))),
        TypeExprKind::Tuple(items) => Type::Tuple(items.iter().map(type_of_ann).collect()),
        TypeExprKind::Function {
            params,
            return_type,
        } => Type::Function {
            params: params.iter().map(type_of_ann).collect(),
            return_type: Box::new(type_of_ann(return_type)),
            required_params: params.len(),
        },
        TypeExprKind::Result { ok_type, err_type } => Type::Result {
            ok_type: Box::new(type_of_ann(ok_type)),
            err_type: Box::new(type_of_ann(err_type)),
        },
        TypeExprKind::Path(segments) => segments
            .last()
            .map(|s| named_type(s))
            .unwrap_or(Type::Unknown),
        TypeExprKind::Infer => Type::Any,
    }
}

/// The checker's type for a built-in type name, or `None` for a user type.
pub fn primitive_type(name: &str) -> Option<Type> {
    Some(match name {
        "int" => Type::Int,
        "float" => Type::Float,
        "bool" => Type::Bool,
        "string" => Type::String,
        "char" => Type::Char,
        "void" => Type::Void,
        "int8" => Type::Int8,
        "int16" => Type::Int16,
        "int32" => Type::Int32,
        "int64" => Type::Int64,
        // `byte` is the checker's alias for `uint8`.
        "uint8" | "byte" => Type::UInt8,
        "uint16" => Type::UInt16,
        "uint32" => Type::UInt32,
        "uint64" => Type::UInt64,
        "any" => Type::Any,
        _ => return None,
    })
}

fn named_type(name: &str) -> Type {
    // A bare capitalised name is a user type. Whether it is a struct or an enum
    // is settled once the layouts are known, so record it as a struct and let
    // the lowering consult `LayoutMap` for the real answer.
    primitive_type(name).unwrap_or_else(|| Type::Struct(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(src: &str) -> Program {
        lirac::parse_source(src).expect("parses")
    }

    #[test]
    fn struct_fields_sit_after_the_header() {
        let map = LayoutMap::build(&program("struct Point { x: int\ny: int }")).unwrap();
        let point = map.structs.get("Point").unwrap();
        assert_eq!(point.field("x").unwrap().offset, 16);
        assert_eq!(point.field("y").unwrap().offset, 24);
        assert_eq!(point.size, 32);
    }

    #[test]
    fn narrow_fields_pack_and_align() {
        let src = "struct S { a: int8\nb: int32\nc: int8\nd: int }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let s = map.structs.get("S").unwrap();
        assert_eq!(s.field("a").unwrap().offset, 16);
        // b needs 4-byte alignment, so a's single byte is followed by padding.
        assert_eq!(s.field("b").unwrap().offset, 20);
        assert_eq!(s.field("c").unwrap().offset, 24);
        assert_eq!(s.field("d").unwrap().offset, 32);
        assert_eq!(s.size, 40);
    }

    #[test]
    fn aggregate_fields_are_pointers() {
        let src = "struct Point { x: int }\nstruct Line { start: Point\nend: Point }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let line = map.structs.get("Line").unwrap();
        assert_eq!(line.field("end").unwrap().offset, 24);
        assert_eq!(line.size, 32);
    }

    #[test]
    fn recursive_structs_are_layable() {
        let src = "struct Node { value: int\nnext: Node }";
        let map = LayoutMap::build(&program(src)).unwrap();
        assert_eq!(map.structs.get("Node").unwrap().size, 32);
    }

    #[test]
    fn enum_size_follows_the_widest_variant() {
        let src = "enum Shape { Dot, Circle(float), Rect(float, float) }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let shape = map.enums.get("Shape").unwrap();
        assert_eq!(shape.variant("Dot").unwrap().tag, 0);
        assert_eq!(shape.variant("Rect").unwrap().tag, 2);
        // header + tag + two payload slots
        assert_eq!(shape.size, 40);
    }

    #[test]
    fn type_aliases_resolve_through_chains() {
        let map = LayoutMap::build(&program("type A = B\ntype B = int")).unwrap();
        assert_eq!(map.resolve_alias("A"), Some(Type::Int));
        assert_eq!(map.resolve_alias("B"), Some(Type::Int));
        assert_eq!(map.resolve_alias("Missing"), None);
    }

    #[test]
    fn range_is_a_builtin_layout_a_user_struct_can_shadow() {
        let map = LayoutMap::build(&program("let x = 1")).unwrap();
        assert!(map.range_layout_is_usable());
        let shadowed = LayoutMap::build(&program("struct Range { lo: string }")).unwrap();
        assert!(!shadowed.range_layout_is_usable());
    }

    #[test]
    fn class_inheritance_is_rejected() {
        let src = "class Base { x: int }\nclass Child extends Base { y: int }";
        let err = LayoutMap::build(&program(src)).unwrap_err();
        assert!(err.to_string().contains("inheritance"));
    }
}
