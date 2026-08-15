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

use std::collections::{HashMap, HashSet};

use lirac::ast::{Field, Program, Statement, StatementKind, TypeExpr, TypeParam};
use lirac::checker::Type;

use crate::error::{CodegenError, CodegenResult};

/// Size of `LiraHeader`. Must match `LIRA_HEADER_SIZE` in `runtime/lira_rt.h`.
pub const HEADER_SIZE: i32 = 16;

/// Offsets into the runtime's built-in objects. Mirrored from `lira_rt.h`.
pub const STR_LEN_OFFSET: i32 = 16;
pub const ARRAY_LEN_OFFSET: i32 = 16;
pub const ENUM_TAG_OFFSET: i32 = 16;
pub const ENUM_PAYLOAD_OFFSET: i32 = 24;

/// A class instance carries a pointer to its virtual method table between the
/// header and its fields, so a method call is two loads and an indirect call.
pub const CLASS_VTABLE_OFFSET: i32 = 16;

/// A closure object: header, the code pointer, the capture count, then one
/// 8-byte cell per captured value.
///
/// The code it points at always takes the closure itself as its first argument,
/// whether or not it captures anything, so every call site is identical.
pub const CLOSURE_CODE_OFFSET: i32 = 16;
pub const CLOSURE_COUNT_OFFSET: i32 = 24;
pub const CLOSURE_CAPTURES_OFFSET: i32 = 32;

/// A boxed optional is a header and one 8-byte cell holding the payload.
pub const OPTIONAL_SLOT_OFFSET: i32 = 16;

/// The built-in `Result`. It is not registered as an enum layout: its payload
/// types come from the `Result<T, E>` at each use, not from a single shared
/// declaration, so `lower.rs` handles it against `Type::Result` directly.
pub const RESULT_TYPE: &str = "Result";

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
    /// The class this one extends, if any. A class's fields are laid out after
    /// its parent's, so a `Dog*` can be read as an `Animal*`.
    pub parent: Option<String>,
    /// Virtual method table, in slot order: the parent's methods first, with an
    /// `override` replacing the inherited entry, then any new ones appended.
    /// Each entry names the type that supplies the implementation.
    pub vtable: Vec<VtableEntry>,
    /// Whether instances carry a vtable pointer. Only classes do.
    pub is_class: bool,
}

/// One slot of a class's virtual method table.
#[derive(Debug, Clone)]
pub struct VtableEntry {
    pub method: String,
    /// The class whose implementation fills this slot.
    pub owner: String,
}

impl StructLayout {
    pub fn field(&self, name: &str) -> Option<&FieldLayout> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Slot index of a method in the virtual table.
    pub fn vtable_slot(&self, method: &str) -> Option<usize> {
        self.vtable.iter().position(|entry| entry.method == method)
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

/// A generic aggregate declaration, kept as a template until a concrete use
/// demands an instantiation.
#[derive(Debug, Clone)]
pub struct GenericAggregate {
    pub name: String,
    pub type_params: Vec<String>,
    /// `(field name, annotation)` for a struct; empty for an enum.
    pub fields: Vec<(String, TypeExpr)>,
    /// `(variant name, payload annotations)` for an enum; empty for a struct.
    pub variants: Vec<(String, Vec<TypeExpr>)>,
}

/// All aggregate layouts in a program, keyed by type name.
///
/// A generic type contributes no layout of its own: `Box<T>` has no size until
/// `T` is known. Its template lives in `generics`, and each concrete use adds a
/// layout under a mangled name such as `Box$int`.
#[derive(Debug, Default, Clone)]
pub struct LayoutMap {
    pub structs: HashMap<String, StructLayout>,
    pub enums: HashMap<String, EnumLayout>,
    /// `type Name = ...` declarations, unresolved.
    pub aliases: HashMap<String, Type>,
    /// Generic struct and enum templates, by their unparameterised name.
    pub generics: HashMap<String, GenericAggregate>,
}

/// The name a generic type takes once its arguments are known: `Box<int>`
/// becomes `Box$int`.
///
/// The separator is chosen to be something no Lira identifier can contain, so a
/// mangled name can never collide with a declared one.
pub fn mangle(name: &str, args: &[Type]) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let mut mangled = String::from(name);
    for arg in args {
        mangled.push('$');
        mangled.push_str(&arg.display_name());
    }
    mangled
}

/// Turn a mangled name into something a linker will accept as a symbol.
pub fn sanitise_symbol(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
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

/// A class declaration, gathered before anything is laid out so parents can be
/// resolved regardless of declaration order.
struct ClassDecl {
    name: String,
    parent: Option<String>,
    fields: Vec<(String, Type)>,
    methods: Vec<String>,
}

fn collect(statements: &[Statement], map: &mut LayoutMap) -> CodegenResult<()> {
    let mut classes = Vec::new();
    collect_into(statements, map, &mut classes)?;
    lay_out_classes(classes, map)
}

fn collect_into(
    statements: &[Statement],
    map: &mut LayoutMap,
    classes: &mut Vec<ClassDecl>,
) -> CodegenResult<()> {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::StructDecl {
                name,
                type_params,
                fields,
                ..
            } if !type_params.is_empty() => {
                map.generics.insert(
                    name.clone(),
                    GenericAggregate {
                        name: name.clone(),
                        type_params: param_names(type_params),
                        fields: fields
                            .iter()
                            .map(|f: &Field| (f.name.clone(), f.type_ann.clone()))
                            .collect(),
                        variants: Vec::new(),
                    },
                );
            }
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
                methods,
                ..
            } => {
                // Deferred: a child's fields sit after its parent's, so the
                // parent has to be laid out first whatever the source order.
                classes.push(ClassDecl {
                    name: name.clone(),
                    parent: parent.clone(),
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), type_of_ann(&f.type_ann)))
                        .collect(),
                    methods: methods
                        .iter()
                        .filter_map(|m| match &m.kind {
                            StatementKind::FnDecl { name, .. } => Some(name.clone()),
                            _ => None,
                        })
                        .collect(),
                });
            }
            StatementKind::EnumDecl {
                name,
                type_params,
                variants,
            } if !type_params.is_empty() => {
                map.generics.insert(
                    name.clone(),
                    GenericAggregate {
                        name: name.clone(),
                        type_params: param_names(type_params),
                        fields: Vec::new(),
                        variants: variants
                            .iter()
                            .map(|v| (v.name.clone(), v.fields.clone()))
                            .collect(),
                    },
                );
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
            StatementKind::Block(block) => collect_into(&block.statements, map, classes)?,
            _ => {}
        }
    }
    Ok(())
}

/// Lay out classes parents-first, prefixing inherited fields and building each
/// virtual method table.
fn lay_out_classes(mut classes: Vec<ClassDecl>, map: &mut LayoutMap) -> CodegenResult<()> {
    let mut pending = classes.len();
    while !classes.is_empty() {
        let before = classes.len();
        let mut deferred = Vec::new();

        for class in classes {
            let parent_layout = match &class.parent {
                Some(parent) => match map.structs.get(parent) {
                    Some(layout) => Some(layout.clone()),
                    None => {
                        // Parent not laid out yet; try again next round.
                        deferred.push(class);
                        continue;
                    }
                },
                None => None,
            };

            let mut fields: Vec<(String, Type)> = parent_layout
                .as_ref()
                .map(|layout| {
                    layout
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for (name, ty) in &class.fields {
                if !fields.iter().any(|(existing, _)| existing == name) {
                    fields.push((name.clone(), ty.clone()));
                }
            }

            // The parent's slots keep their indices so an inherited method
            // called through a child's table lands in the same place.
            let mut vtable = parent_layout
                .as_ref()
                .map(|layout| layout.vtable.clone())
                .unwrap_or_default();
            for method in &class.methods {
                match vtable.iter_mut().find(|entry| &entry.method == method) {
                    Some(entry) => entry.owner = class.name.clone(),
                    None => vtable.push(VtableEntry {
                        method: method.clone(),
                        owner: class.name.clone(),
                    }),
                }
            }

            let mut layout = layout_fields_from(class.name.clone(), &fields, CLASS_FIELD_START);
            layout.parent = class.parent.clone();
            layout.vtable = vtable;
            layout.is_class = true;
            map.structs.insert(class.name.clone(), layout);
        }

        classes = deferred;
        if classes.len() == before {
            // No progress: a parent that is not a class in this program, or a
            // cycle. Report the first offender rather than looping.
            let orphan = &classes[0];
            return Err(CodegenError::unsupported(format!(
                "class `{}` extends `{}`, which is not a class in this program",
                orphan.name,
                orphan.parent.as_deref().unwrap_or("?")
            )));
        }
        pending = classes.len();
    }
    let _ = pending;
    Ok(())
}

/// Where a class's own fields begin: after the header and the vtable pointer.
const CLASS_FIELD_START: i32 = CLASS_VTABLE_OFFSET + SLOT_SIZE;

/// Lay fields out in declaration order with natural C alignment.
fn layout_fields(name: String, fields: &[(String, Type)]) -> StructLayout {
    layout_fields_from(name, fields, HEADER_SIZE)
}

fn layout_fields_from(name: String, fields: &[(String, Type)], start: i32) -> StructLayout {
    let mut offset = start;
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
        parent: None,
        vtable: Vec::new(),
        is_class: false,
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

fn param_names(params: &[TypeParam]) -> Vec<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

impl LayoutMap {
    /// Build the layout of `name<args>`, adding it under its mangled name.
    ///
    /// Returns the mangled name. Instantiating the same arguments twice is a
    /// no-op, which is what stops a recursive generic from looping.
    pub fn instantiate(&mut self, name: &str, args: &[Type]) -> CodegenResult<String> {
        let mangled = mangle(name, args);
        if self.structs.contains_key(&mangled) || self.enums.contains_key(&mangled) {
            return Ok(mangled);
        }
        let template = self
            .generics
            .get(name)
            .ok_or_else(|| CodegenError::unsupported(format!("`{}` is not a generic type", name)))?
            .clone();
        if template.type_params.len() != args.len() {
            return Err(CodegenError::unsupported(format!(
                "`{}` takes {} type argument(s), not {}",
                name,
                template.type_params.len(),
                args.len()
            )));
        }
        let bindings: HashMap<String, Type> = template
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let in_scope: HashSet<String> = template.type_params.iter().cloned().collect();

        // Reserve the name before laying the body out, so a type that refers to
        // itself finds the entry rather than recursing forever.
        if template.variants.is_empty() {
            self.structs
                .insert(mangled.clone(), layout_fields(mangled.clone(), &[]));
            let fields: Vec<(String, Type)> = template
                .fields
                .iter()
                .map(|(field, ann)| {
                    (
                        field.clone(),
                        substitute(&type_of_ann_in(ann, &in_scope), &bindings),
                    )
                })
                .collect();
            self.structs
                .insert(mangled.clone(), layout_fields(mangled.clone(), &fields));
        } else {
            self.enums.insert(
                mangled.clone(),
                EnumLayout {
                    name: mangled.clone(),
                    variants: Vec::new(),
                    size: ENUM_PAYLOAD_OFFSET,
                },
            );
            let mut widest = 0usize;
            let variants: Vec<VariantLayout> = template
                .variants
                .iter()
                .enumerate()
                .map(|(tag, (variant, payloads))| {
                    let field_types: Vec<Type> = payloads
                        .iter()
                        .map(|ann| substitute(&type_of_ann_in(ann, &in_scope), &bindings))
                        .collect();
                    widest = widest.max(field_types.len());
                    VariantLayout {
                        name: variant.clone(),
                        tag: tag as i64,
                        field_types,
                    }
                })
                .collect();
            self.enums.insert(
                mangled.clone(),
                EnumLayout {
                    name: mangled.clone(),
                    variants,
                    size: ENUM_PAYLOAD_OFFSET + SLOT_SIZE * widest as i32,
                },
            );
        }
        Ok(mangled)
    }
}

/// Replace every type parameter in `ty` with what it is bound to.
pub fn substitute(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(inner) => Type::Array(Box::new(substitute(inner, bindings))),
        Type::Optional(inner) => Type::Optional(Box::new(substitute(inner, bindings))),
        Type::Tuple(items) => Type::Tuple(items.iter().map(|t| substitute(t, bindings)).collect()),
        Type::Map(key, value) => Type::Map(
            Box::new(substitute(key, bindings)),
            Box::new(substitute(value, bindings)),
        ),
        Type::Result { ok_type, err_type } => Type::Result {
            ok_type: Box::new(substitute(ok_type, bindings)),
            err_type: Box::new(substitute(err_type, bindings)),
        },
        Type::Function {
            params,
            return_type,
            required_params,
        } => Type::Function {
            params: params.iter().map(|t| substitute(t, bindings)).collect(),
            return_type: Box::new(substitute(return_type, bindings)),
            required_params: *required_params,
        },
        other => other.clone(),
    }
}

/// Resolve an AST type annotation into a checker `Type`.
///
/// The checker already resolved these for expressions, but field annotations
/// are consulted directly while laying types out, before any expression has
/// been visited.
pub fn type_of_ann(ann: &lirac::ast::TypeExpr) -> Type {
    type_of_ann_in(ann, &HashSet::new())
}

/// Resolve an annotation with a set of type parameter names in scope, so a bare
/// `T` inside `fn f<T>(...)` becomes `Type::TypeParam("T")` rather than a
/// reference to a type named `T`.
pub fn type_of_ann_in(ann: &lirac::ast::TypeExpr, type_params: &HashSet<String>) -> Type {
    use lirac::ast::TypeExprKind;
    let recur = |inner: &lirac::ast::TypeExpr| type_of_ann_in(inner, type_params);
    match &ann.kind {
        TypeExprKind::Named(name) if type_params.contains(name) => Type::TypeParam(name.clone()),
        TypeExprKind::Named(name) => named_type(name),
        TypeExprKind::Generic { name, args } => match name.as_str() {
            "Array" | "List" if args.len() == 1 => Type::Array(Box::new(recur(&args[0]))),
            // `Result<T, E>` reaches here as a generic application rather than
            // as `TypeExprKind::Result`, depending on how it was written.
            RESULT_TYPE if args.len() == 2 => Type::Result {
                ok_type: Box::new(recur(&args[0])),
                err_type: Box::new(recur(&args[1])),
            },
            // A user generic such as `Box<int>` names its instantiation. The
            // layout itself is built on demand by `LayoutMap::instantiate`.
            _ => {
                let arguments: Vec<Type> = args.iter().map(&recur).collect();
                named_type(&mangle(name, &arguments))
            }
        },
        TypeExprKind::Optional(inner) => Type::Optional(Box::new(recur(inner))),
        TypeExprKind::Array(inner) => Type::Array(Box::new(recur(inner))),
        TypeExprKind::Tuple(items) => Type::Tuple(items.iter().map(&recur).collect()),
        TypeExprKind::Function {
            params,
            return_type,
        } => Type::Function {
            params: params.iter().map(&recur).collect(),
            return_type: Box::new(recur(return_type)),
            required_params: params.len(),
        },
        TypeExprKind::Result { ok_type, err_type } => Type::Result {
            ok_type: Box::new(recur(ok_type)),
            err_type: Box::new(recur(err_type)),
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
    // A bare `Result` carries no payload types; treat both as unconstrained.
    if name == RESULT_TYPE {
        return Type::Result {
            ok_type: Box::new(Type::Any),
            err_type: Box::new(Type::Any),
        };
    }
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
    fn a_child_class_prefixes_its_parents_fields() {
        let src = "class Base { x: int }\nclass Child extends Base { y: int }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let base = map.structs.get("Base").unwrap();
        let child = map.structs.get("Child").unwrap();
        // Header, vtable pointer, then the fields.
        assert_eq!(base.field("x").unwrap().offset, 24);
        // The inherited field keeps its offset, so a `Child*` reads as a `Base*`.
        assert_eq!(child.field("x").unwrap().offset, 24);
        assert_eq!(child.field("y").unwrap().offset, 32);
        assert_eq!(child.parent.as_deref(), Some("Base"));
    }

    #[test]
    fn a_child_class_is_laid_out_after_its_parent_whatever_the_order() {
        let src = "class Child extends Base { y: int }\nclass Base { x: int }";
        let map = LayoutMap::build(&program(src)).unwrap();
        assert_eq!(
            map.structs.get("Child").unwrap().field("x").unwrap().offset,
            24
        );
    }

    #[test]
    fn an_override_replaces_the_inherited_vtable_slot() {
        let src = "class Animal { fn speak(self) -> string { return \"...\" }\n                   fn describe(self) -> string { return \"x\" } }\n                   class Dog extends Animal { override fn speak(self) -> string { return \"Woof\" } }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let animal = map.structs.get("Animal").unwrap();
        let dog = map.structs.get("Dog").unwrap();
        assert_eq!(animal.vtable_slot("speak"), Some(0));
        // The override keeps the slot index but changes whose code fills it.
        assert_eq!(dog.vtable_slot("speak"), Some(0));
        assert_eq!(dog.vtable[0].owner, "Dog");
        assert_eq!(dog.vtable_slot("describe"), Some(1));
        assert_eq!(dog.vtable[1].owner, "Animal");
    }

    #[test]
    fn a_class_extending_something_unknown_is_reported() {
        let err = LayoutMap::build(&program("class Child extends Missing { y: int }")).unwrap_err();
        assert!(err.to_string().contains("not a class"));
    }
}
