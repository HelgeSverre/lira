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

use lirac::ast::{Expression, Field, Program, Statement, StatementKind, TypeExpr, TypeParam};
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

/// A parameter in an interface method's normalized signature.
///
/// The first parameter is always the synthetic interface receiver. Its name
/// is `self`, its default is `None`, and its type is the containing interface.
/// Remaining parameters retain their source names and default expressions so
/// native call lowering can fill omitted arguments from the declaration.
#[derive(Debug, Clone)]
pub struct InterfaceParamLayout {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expression>,
}

/// One interface method in declaration order.
#[derive(Debug, Clone)]
pub struct InterfaceMethodLayout {
    pub name: String,
    /// The normalized checker signature, including the receiver at parameter
    /// slot zero.
    pub signature: Type,
    pub params: Vec<InterfaceParamLayout>,
    /// Stable zero-based ordinal in the containing interface.
    pub slot: usize,
}

/// Native metadata for an interface declaration.
#[derive(Debug, Clone)]
pub struct InterfaceLayout {
    pub name: String,
    pub methods: Vec<InterfaceMethodLayout>,
}

impl InterfaceLayout {
    pub fn method(&self, name: &str) -> Option<&InterfaceMethodLayout> {
        self.methods.iter().find(|method| method.name == name)
    }

    pub fn method_slot(&self, name: &str) -> Option<usize> {
        self.method(name).map(|method| method.slot)
    }
}

/// The built-in struct a `a..b` expression evaluates to, mirroring the object
/// the bytecode backend builds. Iterating a range reads these fields back.
///
/// The dollar sign cannot occur in a Lira source identifier, so this backend
/// name cannot collide with a user-declared `struct Range` (or any other
/// source-level aggregate).
pub const RANGE_TYPE: &str = lirac::checker::BUILTIN_RANGE_TYPE;

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
    pub interfaces: HashMap<String, InterfaceLayout>,
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
    let mut symbol = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            symbol.push(character);
        } else if character == '_' {
            symbol.push_str("_u");
        } else {
            use std::fmt::Write as _;
            write!(symbol, "_x{:x}_", character as u32).expect("writing to a String cannot fail");
        }
    }
    symbol
}

impl LayoutMap {
    /// Walk the program and compute a layout for every struct, class, enum, and
    /// interface.
    ///
    /// Field types never need a layout of their own to be sized: an aggregate
    /// field is a pointer, so recursive and mutually recursive types fall out
    /// for free and declaration order does not matter.
    pub fn build(program: &Program) -> CodegenResult<Self> {
        let mut map = LayoutMap::default();
        // Keep the compiler-created range layout under a name that source code
        // cannot spell. A user `struct Range` is therefore an independent
        // source-visible layout, not a replacement for this one.
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
        collect_interface_names(&program.statements, &mut map.interfaces);
        collect_aliases(&program.statements, &mut map.aliases);
        collect(&program.statements, &mut map)?;
        Ok(map)
    }

    /// Whether the compiler-created `Range` layout is present and well formed.
    /// The private key means a source `struct Range` cannot shadow it.
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

    /// Canonical owner spelling used by native impl registration and calls.
    /// Aliases are transparent, including aliases nested inside array owners.
    pub fn canonical_impl_owner(&self, name: &str) -> String {
        fn canonical(map: &LayoutMap, ty: Type, seen: &mut HashSet<String>) -> Type {
            match ty {
                Type::Array(inner) => Type::Array(Box::new(canonical(map, *inner, seen))),
                Type::Struct(alias) | Type::Class(alias) | Type::Enum(alias) => {
                    if seen.insert(alias.clone()) {
                        if let Some(target) = map.aliases.get(&alias) {
                            return canonical(map, target.clone(), seen);
                        }
                    }
                    Type::Struct(alias)
                }
                other => other,
            }
        }

        let ty = if let Some(inner) = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
            Type::Array(Box::new(canonical(
                self,
                self.aliases
                    .get(inner)
                    .cloned()
                    .unwrap_or_else(|| named_type(inner)),
                &mut HashSet::new(),
            )))
        } else {
            self.aliases
                .get(name)
                .cloned()
                .unwrap_or_else(|| named_type(name))
        };
        canonical(self, ty, &mut HashSet::new()).display_name()
    }

    pub fn is_aggregate(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }

    pub fn interface(&self, name: &str) -> Option<&InterfaceLayout> {
        self.interfaces.get(name)
    }

    pub fn interface_method(
        &self,
        interface: &str,
        method: &str,
    ) -> Option<&InterfaceMethodLayout> {
        self.interface(interface)?.method(method)
    }

    pub fn interface_method_slot(&self, interface: &str, method: &str) -> Option<usize> {
        self.interface(interface)?.method_slot(method)
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

fn interface_method_layout(
    layouts: &LayoutMap,
    interface_name: &str,
    method: &lirac::ast::InterfaceMethod,
    slot: usize,
) -> InterfaceMethodLayout {
    let has_explicit_receiver = method
        .params
        .first()
        .is_some_and(|param| param.name == "self");
    let explicit_params = method
        .params
        .iter()
        .skip(usize::from(has_explicit_receiver));
    let explicit_layouts: Vec<_> = explicit_params
        .map(|param| InterfaceParamLayout {
            name: param.name.clone(),
            ty: interface_type_of_ann(layouts, &param.type_ann, interface_name),
            default: param.default.clone(),
        })
        .collect();
    let required_params = 1 + explicit_layouts
        .iter()
        .filter(|param| param.default.is_none())
        .count();
    let mut params = Vec::with_capacity(explicit_layouts.len() + 1);
    params.push(InterfaceParamLayout {
        name: "self".to_string(),
        ty: Type::Interface(interface_name.to_string()),
        default: None,
    });
    params.extend(explicit_layouts);
    let signature = Type::Function {
        params: params.iter().map(|param| param.ty.clone()).collect(),
        return_type: Box::new(
            method
                .return_type
                .as_ref()
                .map(|ann| interface_type_of_ann(layouts, ann, interface_name))
                .unwrap_or(Type::Any),
        ),
        required_params,
    };
    InterfaceMethodLayout {
        name: method.name.clone(),
        signature,
        params,
        slot,
    }
}

/// Resolve an interface annotation with the checker's `Self` behavior.
///
/// `type_of_ann` is deliberately owner-independent because it is also used by
/// aggregate layouts. Interface signatures need one small owner-aware layer so
/// `Self` and nested occurrences of `Self` become the interface nominal type.
fn interface_type_of_ann(layouts: &LayoutMap, ann: &TypeExpr, interface_name: &str) -> Type {
    use lirac::ast::TypeExprKind;

    let recur = |inner: &TypeExpr| interface_type_of_ann(layouts, inner, interface_name);
    match &ann.kind {
        TypeExprKind::Named(name) if name == "Self" => Type::Interface(interface_name.to_string()),
        TypeExprKind::Path(segments) if segments.last().is_some_and(|name| name == "Self") => {
            Type::Interface(interface_name.to_string())
        }
        TypeExprKind::Named(name) if layouts.interfaces.contains_key(name) => {
            Type::Interface(name.clone())
        }
        TypeExprKind::Named(name) => layouts
            .aliases
            .get(name)
            .cloned()
            .map(|ty| resolve_interface_alias(layouts, ty, &mut HashSet::new()))
            .unwrap_or_else(|| type_of_ann(ann)),
        TypeExprKind::Path(segments) => segments
            .last()
            .and_then(|name| layouts.aliases.get(name))
            .cloned()
            .map(|ty| resolve_interface_alias(layouts, ty, &mut HashSet::new()))
            .unwrap_or_else(|| type_of_ann(ann)),
        TypeExprKind::Infer => type_of_ann(ann),
        TypeExprKind::Generic { name, args } => {
            let arguments: Vec<Type> = args.iter().map(&recur).collect();
            match name.as_str() {
                "Array" | "List" if arguments.len() == 1 => {
                    Type::Array(Box::new(arguments[0].clone()))
                }
                "Channel" if arguments.len() == 1 => Type::Channel(Box::new(arguments[0].clone())),
                "Map" if arguments.len() == 2 => Type::Map(
                    Box::new(arguments[0].clone()),
                    Box::new(arguments[1].clone()),
                ),
                RESULT_TYPE if arguments.len() == 2 => Type::Result {
                    ok_type: Box::new(arguments[0].clone()),
                    err_type: Box::new(arguments[1].clone()),
                },
                _ => named_type(&mangle(name, &arguments)),
            }
        }
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
    }
}

fn resolve_interface_alias(layouts: &LayoutMap, ty: Type, seen: &mut HashSet<String>) -> Type {
    match ty {
        Type::Struct(name) | Type::Class(name) | Type::Enum(name)
            if seen.insert(name.clone()) && layouts.aliases.contains_key(&name) =>
        {
            let resolved = resolve_interface_alias(
                layouts,
                layouts
                    .aliases
                    .get(&name)
                    .cloned()
                    .expect("alias existence was checked"),
                seen,
            );
            seen.remove(&name);
            resolved
        }
        Type::Struct(name) if layouts.interfaces.contains_key(&name) => Type::Interface(name),
        Type::Array(inner) => Type::Array(Box::new(resolve_interface_alias(layouts, *inner, seen))),
        Type::Optional(inner) => {
            Type::Optional(Box::new(resolve_interface_alias(layouts, *inner, seen)))
        }
        Type::Tuple(items) => Type::Tuple(
            items
                .into_iter()
                .map(|item| resolve_interface_alias(layouts, item, seen))
                .collect(),
        ),
        Type::Map(key, value) => Type::Map(
            Box::new(resolve_interface_alias(layouts, *key, seen)),
            Box::new(resolve_interface_alias(layouts, *value, seen)),
        ),
        Type::Channel(inner) => {
            Type::Channel(Box::new(resolve_interface_alias(layouts, *inner, seen)))
        }
        Type::Result { ok_type, err_type } => Type::Result {
            ok_type: Box::new(resolve_interface_alias(layouts, *ok_type, seen)),
            err_type: Box::new(resolve_interface_alias(layouts, *err_type, seen)),
        },
        Type::Function {
            params,
            return_type,
            required_params,
        } => Type::Function {
            params: params
                .into_iter()
                .map(|param| resolve_interface_alias(layouts, param, seen))
                .collect(),
            return_type: Box::new(resolve_interface_alias(layouts, *return_type, seen)),
            required_params,
        },
        other => other,
    }
}

fn collect(statements: &[Statement], map: &mut LayoutMap) -> CodegenResult<()> {
    let mut classes = Vec::new();
    collect_into(statements, map, &mut classes)?;
    lay_out_classes(classes, map)
}

fn collect_interface_names(
    statements: &[Statement],
    interfaces: &mut HashMap<String, InterfaceLayout>,
) {
    for statement in statements {
        match &statement.kind {
            StatementKind::InterfaceDecl { name, .. } => {
                interfaces
                    .entry(name.clone())
                    .or_insert_with(|| InterfaceLayout {
                        name: name.clone(),
                        methods: Vec::new(),
                    });
            }
            StatementKind::Block(block) => collect_interface_names(&block.statements, interfaces),
            _ => {}
        }
    }
}

fn collect_aliases(statements: &[Statement], aliases: &mut HashMap<String, Type>) {
    for statement in statements {
        match &statement.kind {
            StatementKind::TypeAlias { name, type_expr } => {
                aliases.insert(name.clone(), type_of_ann(type_expr));
            }
            StatementKind::Block(block) => collect_aliases(&block.statements, aliases),
            _ => {}
        }
    }
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
            StatementKind::InterfaceDecl { name, methods } => {
                let mut seen = HashSet::new();
                let methods = methods
                    .iter()
                    .enumerate()
                    .map(|(slot, method)| {
                        if !seen.insert(method.name.clone()) {
                            return Err(CodegenError::unsupported(format!(
                                "duplicate method `{}` in interface `{}`",
                                method.name, name
                            )));
                        }
                        Ok(interface_method_layout(map, name, method, slot))
                    })
                    .collect::<CodegenResult<Vec<_>>>()?;
                map.interfaces.insert(
                    name.clone(),
                    InterfaceLayout {
                        name: name.clone(),
                        methods,
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
                    self.materialize_annotation(ann, &in_scope, &bindings)
                        .map(|ty| (field.clone(), ty))
                })
                .collect::<CodegenResult<_>>()?;
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
                .map(
                    |(tag, (variant, payloads))| -> CodegenResult<VariantLayout> {
                        let field_types: Vec<Type> = payloads
                            .iter()
                            .map(|ann| self.materialize_annotation(ann, &in_scope, &bindings))
                            .collect::<CodegenResult<_>>()?;
                        widest = widest.max(field_types.len());
                        Ok(VariantLayout {
                            name: variant.clone(),
                            tag: tag as i64,
                            field_types,
                        })
                    },
                )
                .collect::<CodegenResult<_>>()?;
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

    /// Resolve a template annotation under concrete bindings and eagerly
    /// instantiate any generic aggregate it contains. The checker represents
    /// user generic applications as nominal types, but the AST still gives us
    /// their argument boundaries; using those boundaries here avoids trying to
    /// reverse-engineer nested `$`-mangled names later.
    fn materialize_annotation(
        &mut self,
        ann: &TypeExpr,
        in_scope: &HashSet<String>,
        bindings: &HashMap<String, Type>,
    ) -> CodegenResult<Type> {
        use lirac::ast::TypeExprKind;

        let recur = |inner: &TypeExpr, layouts: &mut LayoutMap| {
            layouts.materialize_annotation(inner, in_scope, bindings)
        };
        let ty =
            match &ann.kind {
                TypeExprKind::Named(name) if in_scope.contains(name) => {
                    substitute(&Type::TypeParam(name.clone()), bindings)
                }
                TypeExprKind::Named(_) | TypeExprKind::Path(_) | TypeExprKind::Infer => {
                    substitute(&type_of_ann_in(ann, in_scope), bindings)
                }
                TypeExprKind::Generic { name, args } => {
                    let arguments: Vec<Type> = args
                        .iter()
                        .map(|arg| recur(arg, self))
                        .collect::<CodegenResult<_>>()?;
                    match name.as_str() {
                        "Array" | "List" if arguments.len() == 1 => {
                            Type::Array(Box::new(arguments.into_iter().next().ok_or_else(
                                || CodegenError::internal("array type lost its element"),
                            )?))
                        }
                        "Channel" if arguments.len() == 1 => {
                            Type::Channel(Box::new(arguments.into_iter().next().ok_or_else(
                                || CodegenError::internal("channel type lost its element"),
                            )?))
                        }
                        "Map" if arguments.len() == 2 => {
                            let mut arguments = arguments.into_iter();
                            Type::Map(
                                Box::new(arguments.next().ok_or_else(|| {
                                    CodegenError::internal("map type lost its key")
                                })?),
                                Box::new(arguments.next().ok_or_else(|| {
                                    CodegenError::internal("map type lost its value")
                                })?),
                            )
                        }
                        RESULT_TYPE if arguments.len() == 2 => {
                            let mut arguments = arguments.into_iter();
                            Type::Result {
                                ok_type: Box::new(arguments.next().ok_or_else(|| {
                                    CodegenError::internal("result type lost its ok payload")
                                })?),
                                err_type: Box::new(arguments.next().ok_or_else(|| {
                                    CodegenError::internal("result type lost its error payload")
                                })?),
                            }
                        }
                        _ if self.generics.contains_key(name) => {
                            let is_enum = !self
                                .generics
                                .get(name)
                                .is_some_and(|aggregate| aggregate.variants.is_empty());
                            let mangled = self.instantiate(name, &arguments)?;
                            if is_enum {
                                Type::Enum(mangled)
                            } else {
                                Type::Struct(mangled)
                            }
                        }
                        _ => substitute(&type_of_ann_in(ann, in_scope), bindings),
                    }
                }
                TypeExprKind::Optional(inner) => Type::Optional(Box::new(recur(inner, self)?)),
                TypeExprKind::Array(inner) => Type::Array(Box::new(recur(inner, self)?)),
                TypeExprKind::Tuple(items) => Type::Tuple(
                    items
                        .iter()
                        .map(|item| recur(item, self))
                        .collect::<CodegenResult<_>>()?,
                ),
                TypeExprKind::Function {
                    params,
                    return_type,
                } => Type::Function {
                    params: params
                        .iter()
                        .map(|param| recur(param, self))
                        .collect::<CodegenResult<_>>()?,
                    return_type: Box::new(recur(return_type, self)?),
                    required_params: params.len(),
                },
                TypeExprKind::Result { ok_type, err_type } => Type::Result {
                    ok_type: Box::new(recur(ok_type, self)?),
                    err_type: Box::new(recur(err_type, self)?),
                },
            };
        Ok(substitute(&ty, bindings))
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
        Type::Struct(name) => Type::Struct(substitute_mangled_name(name, bindings)),
        Type::Class(name) => Type::Class(substitute_mangled_name(name, bindings)),
        Type::Enum(name) => Type::Enum(substitute_mangled_name(name, bindings)),
        other => other.clone(),
    }
}

/// Generic applications are represented in the layout map by a mangled name
/// (`Box$T`) rather than a separate type node. Substitute the parameter
/// segments as well, so a field such as `inner: Box<T>` lays out as
/// `Box$int` when the surrounding aggregate is instantiated.
fn substitute_mangled_name(name: &str, bindings: &HashMap<String, Type>) -> String {
    // Generic names are encoded as `$`-separated segments (`Pair$T$T2`).
    // Replacing substrings is incorrect here: a binding for `T` must not turn
    // the `T` prefix of a distinct parameter named `T2` into `int2`.  Parsing
    // the segments also makes the result independent of HashMap iteration
    // order.  A substituted type may itself contain `$` segments; appending
    // its display name preserves that nested mangling exactly.
    let mut segments = name.split('$');
    let mut result = segments.next().unwrap_or_default().to_owned();
    for segment in segments {
        result.push('$');
        if let Some(ty) = bindings.get(segment) {
            result.push_str(&ty.display_name());
        } else {
            result.push_str(segment);
        }
    }
    result
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
            "Channel" if args.len() == 1 => Type::Channel(Box::new(recur(&args[0]))),
            "Map" if args.len() == 2 => {
                Type::Map(Box::new(recur(&args[0])), Box::new(recur(&args[1])))
            }
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
    fn range_layout_is_private_and_survives_a_user_struct() {
        let map = LayoutMap::build(&program("let x = 1")).unwrap();
        assert!(map.range_layout_is_usable());
        let with_user_range = LayoutMap::build(&program("struct Range { lo: string }")).unwrap();
        assert!(with_user_range.range_layout_is_usable());
        assert_eq!(
            with_user_range
                .structs
                .get(RANGE_TYPE)
                .and_then(|layout| layout.field("start"))
                .map(|field| &field.ty),
            Some(&Type::Int)
        );
        assert_eq!(
            with_user_range
                .structs
                .get("Range")
                .and_then(|layout| layout.field("lo"))
                .map(|field| &field.ty),
            Some(&Type::String)
        );
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

    #[test]
    fn generic_field_layout_materializes_nested_type_arguments() {
        let src = "struct Box<T> { value: T }\nstruct Pair<T> { inner: Box<T> }";
        let mut map = LayoutMap::build(&program(src)).unwrap();
        let pair = map.instantiate("Pair", &[Type::Int]).unwrap();
        let pair_layout = map.structs.get(&pair).unwrap();
        assert_eq!(
            pair_layout.field("inner").unwrap().ty,
            Type::Struct("Box$int".into())
        );
        assert_eq!(
            map.structs
                .get("Box$int")
                .unwrap()
                .field("value")
                .unwrap()
                .ty,
            Type::Int
        );
    }

    #[test]
    fn generic_name_substitution_matches_complete_parameter_segments() {
        let src = "struct Box<T> { value: T }\nstruct Pair<T, T2> { first: T\nsecond: T2\ninner: Box<T2> }";
        let mut map = LayoutMap::build(&program(src)).unwrap();
        let pair = map.instantiate("Pair", &[Type::Int, Type::String]).unwrap();
        assert_eq!(pair, "Pair$int$string");
        let layout = map.structs.get(&pair).unwrap();
        assert_eq!(layout.field("first").unwrap().ty, Type::Int);
        assert_eq!(layout.field("second").unwrap().ty, Type::String);
        assert_eq!(
            layout.field("inner").unwrap().ty,
            Type::Struct("Box$string".into())
        );
        assert_eq!(
            map.structs
                .get("Box$string")
                .unwrap()
                .field("value")
                .unwrap()
                .ty,
            Type::String
        );

        let bindings = HashMap::from([
            ("T".to_string(), Type::Int),
            ("T2".to_string(), Type::String),
        ]);
        assert_eq!(
            substitute_mangled_name("Pair$T$T2", &bindings),
            "Pair$int$string"
        );
    }

    #[test]
    fn interface_methods_keep_declaration_order_and_slots() {
        let src = "interface Drawable { fn area() -> int fn perimeter() -> int }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let layout = map.interface("Drawable").unwrap();
        assert_eq!(
            layout
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>(),
            vec!["area", "perimeter"]
        );
        assert_eq!(layout.method_slot("area"), Some(0));
        assert_eq!(map.interface_method_slot("Drawable", "perimeter"), Some(1));
    }

    #[test]
    fn interface_methods_normalize_implicit_and_explicit_receivers() {
        let src = "interface Explicit { fn value(this, amount: int) -> Self }\n\
                   interface Implicit { fn value(amount: int) -> Self }";
        let map = LayoutMap::build(&program(src)).unwrap();
        for name in ["Explicit", "Implicit"] {
            let method = map.interface(name).unwrap().method("value").unwrap();
            let Type::Function {
                params,
                return_type,
                required_params,
            } = &method.signature
            else {
                panic!("interface method must have a function signature");
            };
            assert_eq!(params.first(), Some(&Type::Interface(name.to_string())));
            assert_eq!(params.get(1), Some(&Type::Int));
            assert_eq!(return_type.as_ref(), &Type::Interface(name.to_string()));
            assert_eq!(*required_params, 2);
            assert_eq!(method.params[0].name, "self");
            assert!(method.params[0].default.is_none());
            assert_eq!(method.params[1].name, "amount");
        }
    }

    #[test]
    fn interface_method_required_count_tracks_defaults() {
        let src = "interface Flexible { fn value(first: int, second: int = 2, third: int) -> int }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let method = map.interface("Flexible").unwrap().method("value").unwrap();
        let Type::Function {
            required_params, ..
        } = &method.signature
        else {
            panic!("interface method must have a function signature");
        };
        assert_eq!(*required_params, 3);
        assert!(method.params[1].default.is_none());
        assert!(method.params[2].default.is_some());
        assert!(method.params[3].default.is_none());
    }

    #[test]
    fn interface_method_layout_resolves_forward_alias_chains() {
        let src = "interface Convert { fn apply(value: Number) -> Integer }\n\
                   type Number = Integer\n\
                   type Integer = int";
        let map = LayoutMap::build(&program(src)).unwrap();
        let method = map.interface("Convert").unwrap().method("apply").unwrap();
        let Type::Function {
            params,
            return_type,
            ..
        } = &method.signature
        else {
            panic!("interface method must have a function signature");
        };
        assert_eq!(params, &[Type::Interface("Convert".to_string()), Type::Int]);
        assert_eq!(return_type.as_ref(), &Type::Int);
    }

    #[test]
    fn interface_method_layout_resolves_aliases_to_interfaces() {
        let src = "interface Named { fn name() -> string }\n\
                   type NamedAlias = Named\n\
                   interface Factory { fn make(values: [NamedAlias]) -> NamedAlias }";
        let map = LayoutMap::build(&program(src)).unwrap();
        let method = map.interface("Factory").unwrap().method("make").unwrap();
        let Type::Function {
            params,
            return_type,
            ..
        } = &method.signature
        else {
            panic!("interface method must have a function signature");
        };
        assert_eq!(
            params,
            &[
                Type::Interface("Factory".to_string()),
                Type::Array(Box::new(Type::Interface("Named".to_string())))
            ]
        );
        assert_eq!(return_type.as_ref(), &Type::Interface("Named".to_string()));
    }

    #[test]
    fn duplicate_interface_methods_are_rejected() {
        let error = LayoutMap::build(&program("interface Duplicate { fn value() fn value() }"))
            .unwrap_err();
        assert!(error.to_string().contains("duplicate method `value`"));
    }

    #[test]
    fn linker_symbol_sanitising_preserves_distinct_type_keys() {
        assert_eq!(sanitise_symbol("Plain123"), "Plain123");
        assert_ne!(sanitise_symbol("Box$int"), sanitise_symbol("Box_int"));
        assert_ne!(sanitise_symbol("A::run"), sanitise_symbol("A__run"));
        assert_eq!(sanitise_symbol("A$int::foo"), "A_x24_int_x3a__x3a_foo");
        assert_ne!(
            sanitise_symbol("A$int::foo"),
            sanitise_symbol("A_x24_int_x3a::x3a_foo")
        );
        assert!(sanitise_symbol("Box<[int]>")
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'));
    }
}
