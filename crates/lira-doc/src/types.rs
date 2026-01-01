//! Documentation data structures
//!
//! Types representing extracted documentation from Lira source files.

/// Documentation for a complete module/file
#[derive(Debug, Clone)]
pub struct ModuleDoc {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub functions: Vec<FunctionDoc>,
    pub structs: Vec<StructDoc>,
    pub enums: Vec<EnumDoc>,
    pub traits: Vec<TraitDoc>,
    pub impls: Vec<ImplDoc>,
    pub classes: Vec<ClassDoc>,
    pub constants: Vec<ConstDoc>,
    pub type_aliases: Vec<TypeAliasDoc>,
}

/// Documentation for a function
#[derive(Debug, Clone)]
pub struct FunctionDoc {
    pub name: String,
    pub signature: String,
    pub doc_comment: Option<String>,
    pub params: Vec<ParamDoc>,
    pub return_type: Option<String>,
    pub type_params: Vec<String>,
    pub is_public: bool,
    pub line: usize,
}

/// Documentation for a function parameter
#[derive(Debug, Clone)]
pub struct ParamDoc {
    pub name: String,
    pub type_name: String,
    pub default_value: Option<String>,
}

/// Documentation for a struct
#[derive(Debug, Clone)]
pub struct StructDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub type_params: Vec<String>,
    pub fields: Vec<FieldDoc>,
    pub methods: Vec<FunctionDoc>,
    pub line: usize,
}

/// Documentation for a struct/class field
#[derive(Debug, Clone)]
pub struct FieldDoc {
    pub name: String,
    pub type_name: String,
    pub is_public: bool,
    pub is_mutable: bool,
}

/// Documentation for an enum
#[derive(Debug, Clone)]
pub struct EnumDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub variants: Vec<VariantDoc>,
    pub line: usize,
}

/// Documentation for an enum variant
#[derive(Debug, Clone)]
pub struct VariantDoc {
    pub name: String,
    pub fields: Vec<String>,
}

/// Documentation for a trait
#[derive(Debug, Clone)]
pub struct TraitDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub type_params: Vec<String>,
    pub methods: Vec<TraitMethodDoc>,
    pub is_public: bool,
    pub line: usize,
}

/// Documentation for a trait method signature
#[derive(Debug, Clone)]
pub struct TraitMethodDoc {
    pub name: String,
    pub signature: String,
    pub has_default: bool,
}

/// Documentation for an impl block
#[derive(Debug, Clone)]
pub struct ImplDoc {
    pub type_name: String,
    pub trait_name: Option<String>,
    pub type_params: Vec<String>,
    pub methods: Vec<FunctionDoc>,
    pub line: usize,
}

/// Documentation for a class
#[derive(Debug, Clone)]
pub struct ClassDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub parent: Option<String>,
    pub interfaces: Vec<String>,
    pub fields: Vec<FieldDoc>,
    pub methods: Vec<FunctionDoc>,
    pub line: usize,
}

/// Documentation for a constant
#[derive(Debug, Clone)]
pub struct ConstDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub type_name: Option<String>,
    pub line: usize,
}

/// Documentation for a type alias
#[derive(Debug, Clone)]
pub struct TypeAliasDoc {
    pub name: String,
    pub doc_comment: Option<String>,
    pub target_type: String,
    pub line: usize,
}
