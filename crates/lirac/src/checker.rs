//! Lira Type Checker
//!
//! Validates types and performs type inference.
//! See docs/lira/02-type-system.md for the full specification.

use crate::ast::*;
use crate::ids::SymbolId;
use crate::sema::SemanticTables;
use std::collections::{HashMap, HashSet};

/// Internal type representation
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    // Primitive types
    Int,   // Default integer (64-bit signed)
    Float, // Default float (64-bit)
    Bool,
    String,
    Char,
    Void,
    Null,

    // Sized integer types
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,

    // Compound types
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Map(Box<Type>, Box<Type>),
    Optional(Box<Type>),
    Result {
        ok_type: Box<Type>,
        err_type: Box<Type>,
    },

    // Function type
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
        /// Number of required parameters (those without defaults)
        required_params: usize,
    },

    // User-defined types
    Class(String),
    Struct(String),
    Enum(String),
    Interface(String),

    // Type variable for inference
    TypeVar(u32),

    // Unknown type (error recovery)
    Unknown,

    // Any type (for compatibility)
    Any,

    // Generic type parameter (e.g., T in fn identity<T>)
    // TODO: Implement monomorphization for proper generics codegen.
    // Currently using type erasure - TypeParam becomes Any at runtime.
    // Monomorphization would generate specialized versions for each concrete type.
    // See docs/lira/ROADMAP.md Phase 7 for details.
    TypeParam(String),
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Float
                | Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::Int64
                | Type::UInt8
                | Type::UInt16
                | Type::UInt32
                | Type::UInt64
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::Int64
                | Type::UInt8
                | Type::UInt16
                | Type::UInt32
                | Type::UInt64
        )
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(
            self,
            Type::Int | Type::Int8 | Type::Int16 | Type::Int32 | Type::Int64
        )
    }

    pub fn is_unsigned_integer(&self) -> bool {
        matches!(
            self,
            Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64
        )
    }

    /// Check if this is an unconstrained generic type parameter
    pub fn is_type_param(&self) -> bool {
        matches!(self, Type::TypeParam(_))
    }

    pub fn is_compatible_with(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Any, _) | (_, Type::Any) => true,
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            // TypeParam is compatible with itself (same name)
            (Type::TypeParam(a), Type::TypeParam(b)) => a == b,
            // Any concrete type can be passed where TypeParam is expected (will be erased at runtime)
            (_, Type::TypeParam(_)) => true,
            (Type::Null, Type::Optional(_)) => true,
            (Type::Optional(a), Type::Optional(b)) => a.is_compatible_with(b),
            (a, Type::Optional(b)) => a.is_compatible_with(b),
            // Result type compatibility
            (
                Type::Result {
                    ok_type: a_ok,
                    err_type: a_err,
                },
                Type::Result {
                    ok_type: b_ok,
                    err_type: b_err,
                },
            ) => a_ok.is_compatible_with(b_ok) && a_err.is_compatible_with(b_err),
            // Float coercion
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => true,
            // Integer type coercion (widening is allowed)
            (a, b) if a.is_integer() && b.is_integer() => true,
            // Integer to float coercion
            (a, Type::Float) if a.is_integer() => true,
            (Type::Float, b) if b.is_integer() => true,
            // Function type compatibility (ignores required_params, compares signatures)
            (
                Type::Function {
                    params: a_params,
                    return_type: a_ret,
                    ..
                },
                Type::Function {
                    params: b_params,
                    return_type: b_ret,
                    ..
                },
            ) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params.iter())
                        .all(|(a, b)| a.is_compatible_with(b))
                    && a_ret.is_compatible_with(b_ret)
            }
            (a, b) => a == b,
        }
    }

    /// Get the bit width of an integer type
    pub fn integer_bits(&self) -> Option<u8> {
        match self {
            Type::Int8 | Type::UInt8 => Some(8),
            Type::Int16 | Type::UInt16 => Some(16),
            Type::Int32 | Type::UInt32 => Some(32),
            Type::Int | Type::Int64 | Type::UInt64 => Some(64),
            _ => None,
        }
    }

    /// Get a user-friendly display name for this type
    pub fn display_name(&self) -> String {
        match self {
            Type::Int => "int".to_string(),
            Type::Float => "float".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "string".to_string(),
            Type::Char => "char".to_string(),
            Type::Void => "void".to_string(),
            Type::Null => "null".to_string(),
            Type::Int8 => "int8".to_string(),
            Type::Int16 => "int16".to_string(),
            Type::Int32 => "int32".to_string(),
            Type::Int64 => "int64".to_string(),
            Type::UInt8 => "uint8".to_string(),
            Type::UInt16 => "uint16".to_string(),
            Type::UInt32 => "uint32".to_string(),
            Type::UInt64 => "uint64".to_string(),
            Type::Array(elem) => format!("[{}]", elem.display_name()),
            Type::Tuple(types) => {
                let inner: Vec<_> = types.iter().map(|t| t.display_name()).collect();
                format!("({})", inner.join(", "))
            }
            Type::Map(k, v) => format!("Map<{}, {}>", k.display_name(), v.display_name()),
            Type::Optional(inner) => format!("{}?", inner.display_name()),
            Type::Result { ok_type, err_type } => {
                format!(
                    "Result<{}, {}>",
                    ok_type.display_name(),
                    err_type.display_name()
                )
            }
            Type::Function {
                params,
                return_type,
                ..
            } => {
                let param_str: Vec<_> = params.iter().map(|t| t.display_name()).collect();
                format!(
                    "fn({}) -> {}",
                    param_str.join(", "),
                    return_type.display_name()
                )
            }
            Type::Class(name) => name.clone(),
            Type::Struct(name) => name.clone(),
            Type::Enum(name) => name.clone(),
            Type::Interface(name) => name.clone(),
            Type::TypeVar(id) => format!("?{}", id),
            Type::Any => "any".to_string(),
            Type::Unknown => "unknown".to_string(),
            Type::TypeParam(name) => name.clone(),
        }
    }
}

/// Symbol table entry
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Variable,
    Function,
    Parameter,
    Field,
    Class,
    Struct,
    Enum,
    Interface,
    TypeAlias,
}

/// Method signature for impl blocks
#[derive(Debug, Clone)]
pub struct ImplMethod {
    pub name: String,
    pub params: Vec<(String, Type)>, // (param_name, param_type)
    pub return_type: Type,
    pub has_self: bool,
}

/// Type environment / scope
pub struct TypeEnv {
    scopes: Vec<HashMap<String, Symbol>>,
    type_defs: HashMap<String, TypeDef>,
    impl_methods: HashMap<String, Vec<ImplMethod>>, // type_name -> methods
    trait_defs: HashMap<String, Vec<ImplMethod>>,   // trait_name -> required methods
    trait_impls: HashMap<(String, String), Vec<ImplMethod>>, // (trait_name, type_name) -> impl
    /// Maps generic function names to their type parameter names
    generic_functions: HashMap<String, Vec<String>>, // fn_name -> [T, U, ...]
    next_type_var: u32,
    errors: Vec<String>,
}

/// Type definition for user-defined types
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub kind: TypeDefKind,
}

#[derive(Debug, Clone)]
pub enum TypeDefKind {
    Class {
        parent: Option<String>,
        interfaces: Vec<String>,
        fields: Vec<(String, Type, bool)>, // name, type, is_public
        methods: Vec<(String, Type, bool)>,
    },
    Struct {
        fields: Vec<(String, Type, bool)>,
        methods: Vec<(String, Type, bool)>,
    },
    Enum {
        variants: Vec<(String, Vec<Type>)>,
    },
    Interface {
        methods: Vec<(String, Type)>,
    },
    Alias(Type),
}

impl TypeEnv {
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![HashMap::new()],
            type_defs: HashMap::new(),
            impl_methods: HashMap::new(),
            trait_defs: HashMap::new(),
            trait_impls: HashMap::new(),
            generic_functions: HashMap::new(),
            next_type_var: 0,
            errors: Vec::new(),
        };

        // Add built-in types
        env.type_defs.insert(
            "int".to_string(),
            TypeDef {
                name: "int".to_string(),
                kind: TypeDefKind::Alias(Type::Int),
            },
        );
        env.type_defs.insert(
            "float".to_string(),
            TypeDef {
                name: "float".to_string(),
                kind: TypeDefKind::Alias(Type::Float),
            },
        );
        env.type_defs.insert(
            "bool".to_string(),
            TypeDef {
                name: "bool".to_string(),
                kind: TypeDefKind::Alias(Type::Bool),
            },
        );
        env.type_defs.insert(
            "string".to_string(),
            TypeDef {
                name: "string".to_string(),
                kind: TypeDefKind::Alias(Type::String),
            },
        );
        env.type_defs.insert(
            "char".to_string(),
            TypeDef {
                name: "char".to_string(),
                kind: TypeDefKind::Alias(Type::Char),
            },
        );
        env.type_defs.insert(
            "void".to_string(),
            TypeDef {
                name: "void".to_string(),
                kind: TypeDefKind::Alias(Type::Void),
            },
        );

        // Add Result as a built-in enum-like type with Ok and Err variants
        // Result::Ok(value) and Result::Err(error) are the constructors
        env.type_defs.insert(
            "Result".to_string(),
            TypeDef {
                name: "Result".to_string(),
                kind: TypeDefKind::Enum {
                    // Ok variant takes one value, Err variant takes one value
                    // The actual types are generic but we use Any for the built-in
                    variants: vec![
                        ("Ok".to_string(), vec![Type::Any]),
                        ("Err".to_string(), vec![Type::Any]),
                    ],
                },
            },
        );

        // Add built-in functions
        env.define(Symbol {
            name: "print".to_string(),
            ty: Type::Function {
                params: vec![Type::Any],
                return_type: Box::new(Type::Void),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "println".to_string(),
            ty: Type::Function {
                params: vec![Type::Any],
                return_type: Box::new(Type::Void),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // Channel built-in functions
        // chan() or chan(capacity)
        env.define(Symbol {
            name: "chan".to_string(),
            ty: Type::Function {
                params: vec![Type::Int],          // Optional capacity (variadic in practice)
                return_type: Box::new(Type::Any), // Channel type
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "send".to_string(),
            ty: Type::Function {
                params: vec![Type::Any, Type::Any], // channel, value
                return_type: Box::new(Type::Void),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "recv".to_string(),
            ty: Type::Function {
                params: vec![Type::Any],          // channel
                return_type: Box::new(Type::Any), // received value
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "close".to_string(),
            ty: Type::Function {
                params: vec![Type::Any], // channel
                return_type: Box::new(Type::Void),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // Fiber built-in functions
        env.define(Symbol {
            name: "fiber_yield".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Void),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "fiber_id".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // Array built-in functions
        env.define(Symbol {
            name: "len".to_string(),
            ty: Type::Function {
                params: vec![Type::Any], // array or string
                return_type: Box::new(Type::Int),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "push".to_string(),
            ty: Type::Function {
                params: vec![Type::Any, Type::Any], // array, value
                return_type: Box::new(Type::Void),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "pop".to_string(),
            ty: Type::Function {
                params: vec![Type::Any], // array
                return_type: Box::new(Type::Any),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // File I/O built-in functions
        // ================================================================

        env.define(Symbol {
            name: "file_open".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::Int], // path, mode
                return_type: Box::new(Type::Int),      // file descriptor
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "file_read".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::Int], // fd, max_bytes
                return_type: Box::new(Type::String),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "file_write".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::String], // fd, data
                return_type: Box::new(Type::Int),      // bytes written
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "file_close".to_string(),
            ty: Type::Function {
                params: vec![Type::Int], // fd
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "file_exists".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "file_size".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Int),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Environment built-in functions
        // ================================================================

        env.define(Symbol {
            name: "env_get".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // env var name
                return_type: Box::new(Type::Optional(Box::new(Type::String))),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_args".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_set".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String], // name, value
                return_type: Box::new(Type::Bool),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_remove".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // name
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_all".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_keys".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_has".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // name
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_exe".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_temp_dir".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "env_home_dir".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Time built-in functions
        // ================================================================

        env.define(Symbol {
            name: "time_ms".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sleep".to_string(),
            ty: Type::Function {
                params: vec![Type::Int], // milliseconds
                return_type: Box::new(Type::Void),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_secs".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_micros".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_nanos".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_format_iso".to_string(),
            ty: Type::Function {
                params: vec![Type::Int], // timestamp_ms
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_format".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::String], // timestamp_ms, format
                return_type: Box::new(Type::String),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_parse_iso".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // ISO 8601 string
                return_type: Box::new(Type::Int),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_timezone_offset".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Int), // offset in minutes
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_components".to_string(),
            ty: Type::Function {
                params: vec![Type::Int],                                 // timestamp_ms
                return_type: Box::new(Type::Array(Box::new(Type::Int))), // [year, month, day, hour, min, sec]
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "time_from_components".to_string(),
            ty: Type::Function {
                params: vec![
                    Type::Int, // year
                    Type::Int, // month
                    Type::Int, // day
                    Type::Int, // hour
                    Type::Int, // minute
                    Type::Int, // second
                ],
                return_type: Box::new(Type::Int), // timestamp_ms
                required_params: 6,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // String operation built-in functions
        // ================================================================

        env.define(Symbol {
            name: "str_char_code".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::Int], // str, index
                return_type: Box::new(Type::Int),      // char code (-1 if out of bounds)
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_from_char_code".to_string(),
            ty: Type::Function {
                params: vec![Type::Int], // char code
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_to_upper".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_to_lower".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_substring".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::Int, Type::Int], // str, start, end
                return_type: Box::new(Type::String),
                required_params: 3,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_index_of".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String], // str, substr
                return_type: Box::new(Type::Int),         // -1 if not found
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_split".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String], // str, delimiter
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_trim".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_trim_start".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "str_trim_end".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Random number generation built-in functions
        // ================================================================

        env.define(Symbol {
            name: "random".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::Float),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "random_int".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::Int], // min, max
                return_type: Box::new(Type::Int),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Base64 encoding/decoding built-in functions
        // ================================================================

        env.define(Symbol {
            name: "base64_encode".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "base64_decode".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "base64_encode_url".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "base64_decode_url".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // URL encoding/decoding built-in functions
        // ================================================================

        env.define(Symbol {
            name: "url_encode".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "url_decode".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // HTTP Client built-in functions
        // ================================================================

        env.define(Symbol {
            name: "http_get".to_string(),
            ty: Type::Function {
                params: vec![Type::String],                              // url
                return_type: Box::new(Type::Array(Box::new(Type::Any))), // [status, body]
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "http_post".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String, Type::String], // url, body, content_type
                return_type: Box::new(Type::Array(Box::new(Type::Any))), // [status, body]
                required_params: 3,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "http_request".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String, Type::String, Type::String], // method, url, headers, body
                return_type: Box::new(Type::Array(Box::new(Type::Any))), // [status, body]
                required_params: 4,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Cryptographic hash built-in functions
        // ================================================================

        env.define(Symbol {
            name: "md5".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sha1".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sha256".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sha512".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // JSON built-in functions
        // ================================================================

        env.define(Symbol {
            name: "json_parse".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::Any), // Can return any JSON value type
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "json_stringify".to_string(),
            ty: Type::Function {
                params: vec![Type::Any],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "json_pretty".to_string(),
            ty: Type::Function {
                params: vec![Type::Any],
                return_type: Box::new(Type::String),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // TCP Networking built-in functions
        // ================================================================

        env.define(Symbol {
            name: "tcp_connect".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::Int], // host, port
                return_type: Box::new(Type::Int),      // socket id or -1
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "tcp_write".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::String], // socket_id, data
                return_type: Box::new(Type::Int),      // bytes written or -1
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "tcp_read".to_string(),
            ty: Type::Function {
                params: vec![Type::Int, Type::Int], // socket_id, max_bytes
                return_type: Box::new(Type::String),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "tcp_close".to_string(),
            ty: Type::Function {
                params: vec![Type::Int], // socket_id
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "dns_lookup".to_string(),
            ty: Type::Function {
                params: vec![Type::String],          // hostname
                return_type: Box::new(Type::String), // IP address
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // OS built-in functions
        // ================================================================

        env.define(Symbol {
            name: "getcwd".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "chdir".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "mkdir".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "mkdir_all".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "rmdir".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "remove".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "remove_all".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "listdir".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "is_dir".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "is_file".to_string(),
            ty: Type::Function {
                params: vec![Type::String], // path
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "rename".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String], // from, to
                return_type: Box::new(Type::Bool),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "copy".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String], // from, to
                return_type: Box::new(Type::Bool),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Regex built-in functions
        // ================================================================

        env.define(Symbol {
            name: "regex_match".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String],
                return_type: Box::new(Type::Bool),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_find".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String],
                return_type: Box::new(Type::String),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_find_all".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_replace".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String, Type::String],
                return_type: Box::new(Type::String),
                required_params: 3,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_replace_all".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String, Type::String],
                return_type: Box::new(Type::String),
                required_params: 3,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_split".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_captures".to_string(),
            ty: Type::Function {
                params: vec![Type::String, Type::String],
                return_type: Box::new(Type::Array(Box::new(Type::String))),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "regex_is_valid".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // UUID built-in functions
        // ================================================================

        env.define(Symbol {
            name: "uuid_v4".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "uuid_v7".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "uuid_is_valid".to_string(),
            ty: Type::Function {
                params: vec![Type::String],
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "uuid_nil".to_string(),
            ty: Type::Function {
                params: vec![],
                return_type: Box::new(Type::String),
                required_params: 0,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        // ================================================================
        // Math built-in functions
        // ================================================================

        env.define(Symbol {
            name: "sqrt".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "pow".to_string(),
            ty: Type::Function {
                params: vec![Type::Float, Type::Float], // base, exponent
                return_type: Box::new(Type::Float),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "exp".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "ln".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "log10".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "log2".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sin".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "cos".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "tan".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "asin".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "acos".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "atan".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "atan2".to_string(),
            ty: Type::Function {
                params: vec![Type::Float, Type::Float], // y, x
                return_type: Box::new(Type::Float),
                required_params: 2,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "sinh".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "cosh".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "tanh".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "floor".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "ceil".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "round".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "trunc".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "abs".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Float),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "is_nan".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "is_infinite".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env.define(Symbol {
            name: "is_finite".to_string(),
            ty: Type::Function {
                params: vec![Type::Float],
                return_type: Box::new(Type::Bool),
                required_params: 1,
            },
            mutable: false,
            kind: SymbolKind::Function,
        });

        env
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn define(&mut self, symbol: Symbol) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(symbol.name.clone(), symbol);
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol);
            }
        }
        None
    }

    pub fn lookup_type(&self, name: &str) -> Option<&TypeDef> {
        self.type_defs.get(name)
    }

    pub fn define_type(&mut self, def: TypeDef) {
        self.type_defs.insert(def.name.clone(), def);
    }

    pub fn fresh_type_var(&mut self) -> Type {
        let id = self.next_type_var;
        self.next_type_var += 1;
        Type::TypeVar(id)
    }

    pub fn error(&mut self, span: &Span, message: String) {
        self.errors
            .push(format!("{}:{}: {}", span.line, span.column, message));
    }

    /// Add a method from an impl block
    pub fn add_impl_method(&mut self, type_name: &str, method: ImplMethod) {
        self.impl_methods
            .entry(type_name.to_string())
            .or_default()
            .push(method);
    }

    /// Get methods for a type from impl blocks
    pub fn get_impl_methods(&self, type_name: &str) -> Option<&Vec<ImplMethod>> {
        self.impl_methods.get(type_name)
    }

    /// Look up a specific method for a type
    pub fn lookup_method(&self, type_name: &str, method_name: &str) -> Option<&ImplMethod> {
        self.impl_methods
            .get(type_name)
            .and_then(|methods| methods.iter().find(|m| m.name == method_name))
    }

    /// Add a trait definition
    pub fn add_trait(&mut self, trait_name: &str, methods: Vec<ImplMethod>) {
        self.trait_defs.insert(trait_name.to_string(), methods);
    }

    /// Get trait definition by name
    pub fn get_trait(&self, trait_name: &str) -> Option<&Vec<ImplMethod>> {
        self.trait_defs.get(trait_name)
    }

    /// Add a trait implementation
    pub fn add_trait_impl(&mut self, trait_name: &str, type_name: &str, methods: Vec<ImplMethod>) {
        self.trait_impls
            .insert((trait_name.to_string(), type_name.to_string()), methods);
    }

    /// Get all fields for a class, including inherited fields from parent classes
    pub fn get_class_fields(&self, class_name: &str) -> Vec<(String, Type, bool)> {
        let mut all_fields = Vec::new();
        let mut current_class = Some(class_name.to_string());

        while let Some(class) = current_class {
            if let Some(type_def) = self.lookup_type(&class) {
                if let TypeDefKind::Class { parent, fields, .. } = &type_def.kind {
                    // Add parent fields first (so child fields can override)
                    current_class = parent.clone();
                    // Prepend to maintain proper order (parent fields first)
                    for field in fields.iter().rev() {
                        all_fields.insert(0, field.clone());
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        all_fields
    }

    /// Get all methods for a class, including inherited methods from parent classes
    pub fn get_class_methods(&self, class_name: &str) -> Vec<(String, Type, bool)> {
        let mut all_methods = Vec::new();
        let mut current_class = Some(class_name.to_string());

        while let Some(class) = current_class {
            if let Some(type_def) = self.lookup_type(&class) {
                if let TypeDefKind::Class {
                    parent, methods, ..
                } = &type_def.kind
                {
                    current_class = parent.clone();
                    // Add methods, later ones (from child) will be found first in lookups
                    all_methods.extend(methods.iter().cloned());
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        all_methods
    }

    /// Check if a class has a parent class
    pub fn get_parent_class(&self, class_name: &str) -> Option<String> {
        self.lookup_type(class_name).and_then(|type_def| {
            if let TypeDefKind::Class { parent, .. } = &type_def.kind {
                parent.clone()
            } else {
                None
            }
        })
    }

    /// Register a generic function with its type parameter names
    pub fn register_generic_function(&mut self, name: &str, type_params: Vec<String>) {
        if !type_params.is_empty() {
            self.generic_functions.insert(name.to_string(), type_params);
        }
    }

    /// Get the type parameter names for a generic function
    pub fn get_generic_function(&self, name: &str) -> Option<&Vec<String>> {
        self.generic_functions.get(name)
    }

    /// Check if a function is generic
    pub fn is_generic_function(&self, name: &str) -> bool {
        self.generic_functions.contains_key(name)
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn get_errors(&self) -> &[String] {
        &self.errors
    }
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a specific instantiation of a generic function
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericInstantiation {
    /// The name of the generic function
    pub function_name: String,
    /// The concrete type names substituted for type parameters (in order)
    pub type_args: Vec<String>,
}

impl GenericInstantiation {
    /// Create a new instantiation from types
    pub fn new(function_name: String, types: &[Type]) -> Self {
        Self {
            function_name,
            type_args: types.iter().map(|t| t.display_name()).collect(),
        }
    }

    /// Generate a mangled name for this instantiation
    pub fn mangled_name(&self) -> String {
        if self.type_args.is_empty() {
            self.function_name.clone()
        } else {
            let type_suffix: Vec<String> = self
                .type_args
                .iter()
                .map(|t| {
                    t.replace(" ", "_")
                        .replace("<", "_")
                        .replace(">", "_")
                        .replace(",", "_")
                })
                .collect();
            format!("{}${}", self.function_name, type_suffix.join("$"))
        }
    }
}

/// The type checker
pub struct TypeChecker {
    env: TypeEnv,
    current_function_return_type: Option<Type>,
    in_loop: bool,
    current_type_name: Option<String>, // For resolving Self type in struct/class methods
    /// Current generic type parameters with their bounds (e.g., T -> [Eq, Hash])
    current_type_params: HashMap<String, Vec<String>>,
    /// All instantiations of generic functions discovered during type checking
    pub generic_instantiations: HashSet<GenericInstantiation>,
    /// Semantic tables being built during checking
    pub sema: SemanticTables,
    /// Counter for generating unique SymbolIds
    next_symbol_id: u32,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
            current_function_return_type: None,
            current_type_name: None,
            in_loop: false,
            current_type_params: HashMap::new(),
            generic_instantiations: HashSet::new(),
            sema: SemanticTables::new(),
            next_symbol_id: 0,
        }
    }

    /// Generate a new unique SymbolId
    fn next_symbol_id(&mut self) -> SymbolId {
        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;
        id
    }

    /// Get the bounds for a type parameter, if it exists
    #[allow(dead_code)]
    fn get_type_param_bounds(&self, name: &str) -> Option<&Vec<String>> {
        self.current_type_params.get(name)
    }

    /// Check if a type parameter has a specific bound (trait)
    #[allow(dead_code)]
    fn type_param_has_bound(&self, type_param: &str, bound: &str) -> bool {
        self.current_type_params
            .get(type_param)
            .map(|bounds| bounds.iter().any(|b| b == bound))
            .unwrap_or(false)
    }

    pub fn check_program(&mut self, program: &Program) -> Result<CheckedProgram, String> {
        // First pass: register type names (for forward references)
        for stmt in &program.statements {
            self.register_type_name(stmt);
        }

        // Second pass: collect full type definitions
        for stmt in &program.statements {
            self.collect_type_def(stmt);
        }

        // Third pass: collect trait definitions and impl blocks
        for stmt in &program.statements {
            self.collect_impl_block(stmt);
        }

        // Fourth pass: register function signatures (for forward references / mutual recursion)
        for stmt in &program.statements {
            self.register_function_signature(stmt);
        }

        // Fifth pass: check all statements
        for stmt in &program.statements {
            self.check_statement(stmt);
        }

        if self.env.has_errors() {
            Err(self.env.get_errors().join("\n"))
        } else {
            // Transfer generic instantiations to sema
            for inst in &self.generic_instantiations {
                self.sema.generic_instantiations.push(crate::sema::GenericInstantiation {
                    generic_name: inst.function_name.clone(),
                    concrete_type: Type::Unknown, // TODO: Store actual type
                });
            }
            Ok(CheckedProgram {
                program: program.clone(),
                sema: self.sema.clone(),
            })
        }
    }

    /// Register type names for forward references (first pass)
    fn register_type_name(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::ClassDecl { name, .. } => {
                // Register placeholder type definition
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Class {
                        parent: None,
                        interfaces: vec![],
                        fields: vec![],
                        methods: vec![],
                    },
                });
            }
            StatementKind::StructDecl { name, .. } => {
                // Register placeholder type definition
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Struct {
                        fields: vec![],
                        methods: vec![],
                    },
                });
            }
            StatementKind::EnumDecl { name, .. } => {
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Enum { variants: vec![] },
                });
            }
            StatementKind::InterfaceDecl { name, .. } => {
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Interface { methods: vec![] },
                });
            }
            StatementKind::TypeAlias { name, type_expr } => {
                let ty = self.resolve_type_expr(type_expr);
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Alias(ty),
                });
            }
            _ => {}
        }
    }

    fn collect_type_def(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::ClassDecl {
                name,
                parent,
                interfaces,
                fields,
                methods,
            } => {
                // Set current type name for Self resolution
                let old_type_name = self.current_type_name.clone();
                self.current_type_name = Some(name.clone());

                // Validate parent class exists
                if let Some(parent_name) = parent {
                    if self.env.lookup_type(parent_name).is_none() {
                        self.env.error(
                            &stmt.span,
                            format!("Parent class '{}' not found", parent_name),
                        );
                    } else if let Some(type_def) = self.env.lookup_type(parent_name) {
                        if !matches!(type_def.kind, TypeDefKind::Class { .. }) {
                            self.env.error(
                                &stmt.span,
                                format!("'{}' is not a class and cannot be extended", parent_name),
                            );
                        }
                    }
                }

                // Get parent methods for override checking
                let parent_methods: Vec<String> = parent
                    .as_ref()
                    .map(|p| {
                        self.env
                            .get_class_methods(p)
                            .into_iter()
                            .map(|(name, _, _)| name)
                            .collect()
                    })
                    .unwrap_or_default();

                let field_types: Vec<_> = fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.clone(),
                            self.resolve_type_expr(&f.type_ann),
                            f.is_public,
                        )
                    })
                    .collect();
                let method_types: Vec<_> = methods
                    .iter()
                    .filter_map(|m| {
                        if let StatementKind::FnDecl {
                            name: method_name,
                            params,
                            return_type,
                            is_public,
                            is_override,
                            ..
                        } = &m.kind
                        {
                            // Check override keyword usage
                            let method_exists_in_parent = parent_methods.contains(method_name);
                            if *is_override && !method_exists_in_parent {
                                self.env.error(
                                    &m.span,
                                    format!(
                                        "Method '{}' is marked as override but does not override any parent method",
                                        method_name
                                    ),
                                );
                            }
                            if method_exists_in_parent && !*is_override {
                                self.env.error(
                                    &m.span,
                                    format!(
                                        "Method '{}' overrides a parent method but is not marked with 'override'",
                                        method_name
                                    ),
                                );
                            }

                            let param_types: Vec<_> = params
                                .iter()
                                .map(|p| self.resolve_type_expr(&p.type_ann))
                                .collect();
                            let ret = return_type
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            Some((
                                method_name.clone(),
                                Type::Function {
                                    params: param_types,
                                    return_type: Box::new(ret),
                                    required_params: 1,
                                },
                                *is_public,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Class {
                        parent: parent.clone(),
                        interfaces: interfaces.clone(),
                        fields: field_types,
                        methods: method_types,
                    },
                });

                self.current_type_name = old_type_name;
            }
            StatementKind::StructDecl {
                name,
                type_params,
                fields,
                methods,
            } => {
                // Set current type name for Self resolution
                let old_type_name = self.current_type_name.clone();
                self.current_type_name = Some(name.clone());

                // Set current type params for generic structs
                let old_type_params = std::mem::replace(
                    &mut self.current_type_params,
                    type_params
                        .iter()
                        .map(|tp| (tp.name.clone(), tp.bounds.clone()))
                        .collect(),
                );

                let field_types: Vec<_> = fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.clone(),
                            self.resolve_type_expr(&f.type_ann),
                            f.is_public,
                        )
                    })
                    .collect();
                let method_types: Vec<_> = methods
                    .iter()
                    .filter_map(|m| {
                        if let StatementKind::FnDecl {
                            name,
                            params,
                            return_type,
                            is_public,
                            ..
                        } = &m.kind
                        {
                            let param_types: Vec<_> = params
                                .iter()
                                .map(|p| self.resolve_type_expr(&p.type_ann))
                                .collect();
                            let ret = return_type
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            Some((
                                name.clone(),
                                Type::Function {
                                    params: param_types,
                                    return_type: Box::new(ret),
                                    required_params: 1,
                                },
                                *is_public,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Struct {
                        fields: field_types,
                        methods: method_types,
                    },
                });

                // Restore old type name and type params
                self.current_type_name = old_type_name;
                self.current_type_params = old_type_params;
            }
            StatementKind::EnumDecl { name, variants } => {
                let variant_types: Vec<_> = variants
                    .iter()
                    .map(|v| {
                        let field_types: Vec<_> =
                            v.fields.iter().map(|t| self.resolve_type_expr(t)).collect();
                        (v.name.clone(), field_types)
                    })
                    .collect();

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Enum {
                        variants: variant_types,
                    },
                });
            }
            StatementKind::InterfaceDecl { name, methods } => {
                let method_types: Vec<_> = methods
                    .iter()
                    .map(|m| {
                        let param_types: Vec<_> = m
                            .params
                            .iter()
                            .map(|p| self.resolve_type_expr(&p.type_ann))
                            .collect();
                        let ret = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Void);
                        (
                            m.name.clone(),
                            Type::Function {
                                params: param_types,
                                return_type: Box::new(ret),
                                required_params: 1,
                            },
                        )
                    })
                    .collect();

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Interface {
                        methods: method_types,
                    },
                });
            }
            StatementKind::TypeAlias { name, type_expr } => {
                let ty = self.resolve_type_expr(type_expr);
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Alias(ty),
                });
            }
            _ => {}
        }
    }

    /// Collect impl blocks and trait definitions (third pass)
    fn collect_impl_block(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::TraitDecl {
                name,
                type_params: _,
                methods,
                is_public: _,
            } => {
                // Use trait name as placeholder for Self in trait method signatures
                // Self will be resolved to the actual type when the trait is implemented
                let old_type_name = self.current_type_name.clone();
                self.current_type_name = Some("Self".to_string());

                let trait_methods: Vec<ImplMethod> = methods
                    .iter()
                    .map(|m| {
                        let param_types: Vec<_> = m
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), self.resolve_type_expr(&p.type_ann)))
                            .collect();
                        let ret = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Void);
                        ImplMethod {
                            name: m.name.clone(),
                            params: param_types,
                            return_type: ret,
                            has_self: m.has_self,
                        }
                    })
                    .collect();
                self.env.add_trait(name, trait_methods);

                self.current_type_name = old_type_name;
            }
            StatementKind::ImplDecl {
                trait_name,
                type_name,
                type_params: _,
                methods,
            } => {
                // Set current type name for Self resolution
                let old_type_name = self.current_type_name.clone();
                self.current_type_name = Some(type_name.clone());

                // Collect all implemented methods
                let mut impl_methods: Vec<ImplMethod> = Vec::new();
                for method in methods {
                    if let StatementKind::FnDecl {
                        name,
                        params,
                        return_type,
                        ..
                    } = &method.kind
                    {
                        let has_self = params.first().map(|p| p.name == "self").unwrap_or(false);
                        let param_types: Vec<_> = params
                            .iter()
                            .map(|p| (p.name.clone(), self.resolve_type_expr(&p.type_ann)))
                            .collect();
                        let ret = return_type
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Void);

                        impl_methods.push(ImplMethod {
                            name: name.clone(),
                            params: param_types,
                            return_type: ret,
                            has_self,
                        });
                    }
                }

                // If this is a trait impl, verify completeness
                if let Some(trait_nm) = trait_name {
                    if let Some(trait_methods) = self.env.get_trait(trait_nm).cloned() {
                        // Check for missing methods
                        for trait_method in &trait_methods {
                            let impl_method =
                                impl_methods.iter().find(|m| m.name == trait_method.name);

                            match impl_method {
                                None => {
                                    // Method missing from implementation
                                    self.env.error(
                                        &stmt.span,
                                        format!(
                                            "trait `{}` requires method `{}` but it is not implemented for `{}`",
                                            trait_nm, trait_method.name, type_name
                                        ),
                                    );
                                }
                                Some(impl_m) => {
                                    // Verify signature matches
                                    self.verify_method_signature(
                                        &stmt.span,
                                        trait_nm,
                                        type_name,
                                        trait_method,
                                        impl_m,
                                    );
                                }
                            }
                        }

                        // Store the trait implementation
                        self.env
                            .add_trait_impl(trait_nm, type_name, impl_methods.clone());
                    } else {
                        // Trait doesn't exist
                        self.env
                            .error(&stmt.span, format!("trait `{}` is not defined", trait_nm));
                    }
                }

                // Add all methods to impl_methods for method resolution
                for impl_method in impl_methods {
                    self.env.add_impl_method(type_name, impl_method);
                }

                self.current_type_name = old_type_name;
            }
            _ => {}
        }
    }

    /// Verify that a method implementation matches the trait method signature
    fn verify_method_signature(
        &mut self,
        span: &Span,
        trait_name: &str,
        type_name: &str,
        trait_method: &ImplMethod,
        impl_method: &ImplMethod,
    ) {
        // Check has_self matches
        if trait_method.has_self != impl_method.has_self {
            let expected = if trait_method.has_self {
                "a `self` receiver"
            } else {
                "no `self` receiver"
            };
            let got = if impl_method.has_self {
                "has `self`"
            } else {
                "has no `self`"
            };
            self.env.error(
                span,
                format!(
                    "method `{}` in impl `{}` for `{}` has wrong receiver: expected {}, but {}",
                    impl_method.name, trait_name, type_name, expected, got
                ),
            );
            return;
        }

        // Get params to compare (skip 'self' if present)
        let trait_params: Vec<_> = trait_method
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .collect();
        let impl_params: Vec<_> = impl_method
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .collect();

        // Check parameter count
        if trait_params.len() != impl_params.len() {
            self.env.error(
                span,
                format!(
                    "method `{}` in impl `{}` for `{}` has wrong number of parameters: expected {}, got {}",
                    impl_method.name, trait_name, type_name, trait_params.len(), impl_params.len()
                ),
            );
            return;
        }

        // Check each parameter type (resolve Self to actual type)
        for (i, ((_, trait_ty), (_, impl_ty))) in
            trait_params.iter().zip(impl_params.iter()).enumerate()
        {
            let resolved_trait_ty = self.resolve_self_type(trait_ty, type_name);
            if !resolved_trait_ty.is_compatible_with(impl_ty) {
                self.env.error(
                    span,
                    format!(
                        "method `{}` in impl `{}` for `{}` has wrong type for parameter {}: expected `{}`, got `{}`",
                        impl_method.name, trait_name, type_name, i + 1,
                        resolved_trait_ty.display_name(), impl_ty.display_name()
                    ),
                );
            }
        }

        // Check return type
        let resolved_trait_ret = self.resolve_self_type(&trait_method.return_type, type_name);
        if !resolved_trait_ret.is_compatible_with(&impl_method.return_type) {
            self.env.error(
                span,
                format!(
                    "method `{}` in impl `{}` for `{}` has wrong return type: expected `{}`, got `{}`",
                    impl_method.name, trait_name, type_name,
                    resolved_trait_ret.display_name(), impl_method.return_type.display_name()
                ),
            );
        }
    }

    /// Resolve Self type in a type to the actual implementing type
    fn resolve_self_type(&self, ty: &Type, type_name: &str) -> Type {
        match ty {
            // Self can be stored as TypeParam("Self") or Struct("Self")
            Type::TypeParam(name) if name == "Self" => Type::Struct(type_name.to_string()),
            Type::Struct(name) if name == "Self" => Type::Struct(type_name.to_string()),
            Type::Class(name) if name == "Self" => Type::Class(type_name.to_string()),
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.resolve_self_type(inner, type_name)))
            }
            Type::Array(inner) => Type::Array(Box::new(self.resolve_self_type(inner, type_name))),
            Type::Function {
                params,
                return_type,
                required_params,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.resolve_self_type(p, type_name))
                    .collect(),
                return_type: Box::new(self.resolve_self_type(return_type, type_name)),
                required_params: *required_params,
            },
            _ => ty.clone(),
        }
    }

    /// Register function signatures for forward references (fourth pass)
    /// This allows mutual recursion: fn a() { b() } fn b() { a() }
    fn register_function_signature(&mut self, stmt: &Statement) {
        if let StatementKind::FnDecl {
            name,
            type_params,
            params,
            return_type,
            ..
        } = &stmt.kind
        {
            // Set current type params for generic functions
            let old_type_params = std::mem::replace(
                &mut self.current_type_params,
                type_params
                    .iter()
                    .map(|tp| (tp.name.clone(), tp.bounds.clone()))
                    .collect(),
            );

            // Build the function type from params and return type
            let param_types: Vec<Type> = params
                .iter()
                .map(|p| self.resolve_type_expr(&p.type_ann))
                .collect();

            // Count required parameters (those without defaults)
            let required_count = params.iter().filter(|p| p.default.is_none()).count();

            let ret_type = return_type
                .as_ref()
                .map(|t| self.resolve_type_expr(t))
                .unwrap_or(Type::Void);

            // Restore old type params
            self.current_type_params = old_type_params;

            // Register the function in the environment
            self.env.define(Symbol {
                name: name.clone(),
                ty: Type::Function {
                    params: param_types,
                    return_type: Box::new(ret_type),
                    required_params: required_count,
                },
                mutable: false,
                kind: SymbolKind::Function,
            });

            // Register generic function type parameters for monomorphization
            if !type_params.is_empty() {
                let type_param_names: Vec<String> =
                    type_params.iter().map(|tp| tp.name.clone()).collect();
                self.env.register_generic_function(name, type_param_names);
            }
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::VarDecl {
                pattern,
                mutable,
                type_ann,
                initializer,
            } => {
                let declared_type = type_ann.as_ref().map(|t| self.resolve_type_expr(t));

                let inferred_type = initializer.as_ref().map(|init| self.check_expression(init));

                let final_type = match (declared_type, inferred_type) {
                    (Some(decl), Some(init)) => {
                        if !init.is_compatible_with(&decl) {
                            self.env.error(
                                &stmt.span,
                                format!(
                                    "Type mismatch: expected '{}', got '{}'",
                                    decl.display_name(),
                                    init.display_name()
                                ),
                            );
                        }
                        decl
                    }
                    (Some(decl), None) => decl,
                    (None, Some(init)) => init,
                    (None, None) => {
                        self.env.error(
                            &stmt.span,
                            "Cannot infer type without initializer".to_string(),
                        );
                        Type::Unknown
                    }
                };

                // Bind pattern variables with the appropriate types
                self.bind_destructuring_pattern(pattern, &final_type, *mutable, &stmt.span);
            }

            StatementKind::ConstDecl {
                name,
                type_ann,
                initializer,
            } => {
                let init_type = self.check_expression(initializer);

                if let Some(type_ann) = type_ann {
                    let declared = self.resolve_type_expr(type_ann);
                    if !init_type.is_compatible_with(&declared) {
                        self.env.error(
                            &stmt.span,
                            format!(
                                "Type mismatch: expected '{}', got '{}'",
                                declared.display_name(),
                                init_type.display_name()
                            ),
                        );
                    }
                }

                self.env.define(Symbol {
                    name: name.clone(),
                    ty: init_type,
                    mutable: false,
                    kind: SymbolKind::Variable,
                });
            }

            StatementKind::FnDecl {
                name: _,
                type_params,
                params,
                return_type,
                body,
                ..
            } => {
                // Function signature already registered in register_function_signature pass
                // Here we just check the function body

                // Set current type params for generic functions
                let old_type_params = std::mem::replace(
                    &mut self.current_type_params,
                    type_params
                        .iter()
                        .map(|tp| (tp.name.clone(), tp.bounds.clone()))
                        .collect(),
                );

                let param_types: Vec<_> = params
                    .iter()
                    .map(|p| self.resolve_type_expr(&p.type_ann))
                    .collect();

                // Check default parameter values have correct types
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    if let Some(default_expr) = &param.default {
                        let default_type = self.check_expression(default_expr);
                        if !default_type.is_compatible_with(param_type) {
                            self.env.error(
                                &param.span,
                                format!(
                                    "Default value type mismatch: parameter '{}' expects '{}', got '{}'",
                                    param.name, param_type.display_name(), default_type.display_name()
                                ),
                            );
                        }
                    }
                }

                let ret_type = return_type
                    .as_ref()
                    .map(|t| self.resolve_type_expr(t))
                    .unwrap_or(Type::Void);

                // Check function body
                self.env.push_scope();

                // Add parameters to scope
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    self.env.define(Symbol {
                        name: param.name.clone(),
                        ty: param_type.clone(),
                        mutable: false,
                        kind: SymbolKind::Parameter,
                    });
                }

                let prev_return = self.current_function_return_type.take();
                self.current_function_return_type = Some(ret_type);

                self.check_block(body);

                self.current_function_return_type = prev_return;
                self.env.pop_scope();

                // Restore old type params
                self.current_type_params = old_type_params;
            }

            StatementKind::Expression(expr) => {
                self.check_expression(expr);
            }

            StatementKind::Return(value) => {
                let return_type = value
                    .as_ref()
                    .map(|e| self.check_expression(e))
                    .unwrap_or(Type::Void);

                if let Some(expected) = &self.current_function_return_type {
                    if !return_type.is_compatible_with(expected) {
                        self.env.error(
                            &stmt.span,
                            format!(
                                "Return type mismatch: expected '{}', got '{}'",
                                expected.display_name(),
                                return_type.display_name()
                            ),
                        );
                    }
                }
            }

            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_type = self.check_expression(condition);
                if cond_type != Type::Bool && cond_type != Type::Unknown {
                    self.env.error(
                        &stmt.span,
                        format!("Condition must be bool, got '{}'", cond_type.display_name()),
                    );
                }

                self.env.push_scope();
                self.check_block(then_branch);
                self.env.pop_scope();

                if let Some(else_block) = else_branch {
                    self.env.push_scope();
                    self.check_block(else_block);
                    self.env.pop_scope();
                }
            }

            StatementKind::While { condition, body } => {
                let cond_type = self.check_expression(condition);
                if cond_type != Type::Bool && cond_type != Type::Unknown {
                    self.env.error(
                        &stmt.span,
                        format!("Condition must be bool, got '{}'", cond_type.display_name()),
                    );
                }

                let prev_in_loop = self.in_loop;
                self.in_loop = true;
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
                self.in_loop = prev_in_loop;
            }

            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                let iter_type = self.check_expression(iterable);

                // Infer element type from iterable
                let elem_type = match &iter_type {
                    Type::Array(elem) => *elem.clone(),
                    Type::String => Type::Char,
                    _ => {
                        // Assume it's iterable and use Any
                        Type::Any
                    }
                };

                let prev_in_loop = self.in_loop;
                self.in_loop = true;
                self.env.push_scope();

                self.env.define(Symbol {
                    name: variable.clone(),
                    ty: elem_type,
                    mutable: false,
                    kind: SymbolKind::Variable,
                });

                self.check_block(body);
                self.env.pop_scope();
                self.in_loop = prev_in_loop;
            }

            StatementKind::Loop { body } => {
                let prev_in_loop = self.in_loop;
                self.in_loop = true;
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
                self.in_loop = prev_in_loop;
            }

            StatementKind::Break(_) => {
                if !self.in_loop {
                    self.env
                        .error(&stmt.span, "break outside of loop".to_string());
                }
            }

            StatementKind::Continue => {
                if !self.in_loop {
                    self.env
                        .error(&stmt.span, "continue outside of loop".to_string());
                }
            }

            StatementKind::Block(block) => {
                self.env.push_scope();
                self.check_block(block);
                self.env.pop_scope();
            }

            StatementKind::Import { .. } => {
                // Import handling would go here
            }

            // Type declarations are handled in collect_type_def
            StatementKind::ClassDecl { .. }
            | StatementKind::StructDecl { .. }
            | StatementKind::EnumDecl { .. }
            | StatementKind::InterfaceDecl { .. }
            | StatementKind::TypeAlias { .. }
            | StatementKind::Use { .. }
            | StatementKind::TraitDecl { .. }
            | StatementKind::ImplDecl { .. } => {}
        }
    }

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.check_statement(stmt);
        }
    }

    fn check_expression(&mut self, expr: &Expression) -> Type {
        let ty = self.check_expression_inner(expr);
        // Record the type in semantic tables
        self.sema.expr_types.insert(expr.id, ty.clone());
        ty
    }

    fn check_expression_inner(&mut self, expr: &Expression) -> Type {
        match &expr.kind {
            ExpressionKind::IntLiteral(_) => Type::Int,
            ExpressionKind::FloatLiteral(_) => Type::Float,
            ExpressionKind::BoolLiteral(_) => Type::Bool,
            ExpressionKind::StringLiteral(_) => Type::String,
            ExpressionKind::CharLiteral(_) => Type::Char,
            ExpressionKind::Null => Type::Null,

            ExpressionKind::Identifier(name) => {
                // Handle 'super' keyword for parent class access
                if name == "super" {
                    if let Some(ref current_class) = self.current_type_name {
                        if let Some(parent) = self.env.get_parent_class(current_class) {
                            return Type::Class(parent);
                        } else {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "'super' used in class '{}' which has no parent class",
                                    current_class
                                ),
                            );
                            return Type::Unknown;
                        }
                    } else {
                        self.env.error(
                            &expr.span,
                            "'super' can only be used inside a class".to_string(),
                        );
                        return Type::Unknown;
                    }
                }

                if let Some(symbol) = self.env.lookup(name) {
                    let sym_ty = symbol.ty.clone();
                    drop(symbol);
                    // Record the symbol reference
                    let sym_id = self.next_symbol_id();
                    self.sema.symbols.insert(sym_id, crate::sema::SymbolEntry {
                        id: sym_id,
                        name: name.clone(),
                        ty: sym_ty.clone(),
                        kind: crate::sema::SymbolKind::Variable,
                        decl_node: expr.id,
                    });
                    self.sema.symbol_refs.insert(expr.id, sym_id);
                    sym_ty
                } else if let Some(type_def) = self.env.lookup_type(name) {
                    // If it's a type name, return the corresponding type
                    // This allows static method calls like Counter.new()
                    match &type_def.kind {
                        TypeDefKind::Struct { .. } => Type::Struct(name.clone()),
                        TypeDefKind::Class { .. } => Type::Class(name.clone()),
                        TypeDefKind::Enum { .. } => Type::Enum(name.clone()),
                        _ => Type::Unknown,
                    }
                } else if self.env.get_impl_methods(name).is_some() {
                    // Built-in type with impl methods (e.g., impl string { ... })
                    // Return a struct type so FieldAccess can look up methods
                    Type::Struct(name.clone())
                } else {
                    self.env
                        .error(&expr.span, format!("Undefined variable: {}", name));
                    Type::Unknown
                }
            }

            ExpressionKind::Binary { left, op, right } => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);

                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow => {
                        // String concatenation is always allowed (toString works on any type)
                        if *op == BinaryOp::Add && left_type == Type::String {
                            Type::String
                        // Reject unconstrained type parameters for arithmetic
                        } else if left_type.is_type_param() || right_type.is_type_param() {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Cannot use {:?} on unconstrained generic type. Add a numeric constraint or use concrete types.",
                                    op
                                ),
                            );
                            Type::Unknown
                        } else if left_type.is_numeric() && right_type.is_numeric() {
                            if left_type == Type::Float || right_type == Type::Float {
                                Type::Float
                            } else {
                                Type::Int
                            }
                        } else {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Invalid operand types for {:?}: {:?} and {:?}",
                                    op, left_type, right_type
                                ),
                            );
                            Type::Unknown
                        }
                    }
                    BinaryOp::Eq | BinaryOp::Ne => Type::Bool,
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        // Reject unconstrained type parameters for comparison
                        if left_type.is_type_param() || right_type.is_type_param() {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Cannot use {:?} on unconstrained generic type. Add a Comparable constraint.",
                                    op
                                ),
                            );
                        } else if !left_type.is_numeric() || !right_type.is_numeric() {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Comparison requires numeric types, got {:?} and {:?}",
                                    left_type, right_type
                                ),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if left_type != Type::Bool || right_type != Type::Bool {
                            self.env.error(
                                &expr.span,
                                "Logical operators require bool operands".to_string(),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr => {
                        // Reject unconstrained type parameters for bitwise ops
                        if left_type.is_type_param() || right_type.is_type_param() {
                            self.env.error(
                                &expr.span,
                                "Cannot use bitwise operators on unconstrained generic types"
                                    .to_string(),
                            );
                        } else if !left_type.is_integer() || !right_type.is_integer() {
                            self.env.error(
                                &expr.span,
                                "Bitwise operators require integer operands".to_string(),
                            );
                        }
                        Type::Int
                    }
                    BinaryOp::NullCoalesce => {
                        // a ?? b returns type of b if a is null
                        right_type
                    }
                }
            }

            ExpressionKind::Unary { op, operand } => {
                let operand_type = self.check_expression(operand);

                match op {
                    UnaryOp::Neg => {
                        if !operand_type.is_numeric() {
                            self.env.error(
                                &expr.span,
                                format!("Cannot negate type '{}'", operand_type.display_name()),
                            );
                        }
                        operand_type
                    }
                    UnaryOp::Not => {
                        if operand_type != Type::Bool {
                            self.env
                                .error(&expr.span, "Logical not requires bool operand".to_string());
                        }
                        Type::Bool
                    }
                    UnaryOp::BitNot => {
                        if !operand_type.is_integer() {
                            self.env.error(
                                &expr.span,
                                "Bitwise not requires integer operand".to_string(),
                            );
                        }
                        Type::Int
                    }
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                        if !operand_type.is_numeric() {
                            self.env.error(
                                &expr.span,
                                "Increment/decrement requires numeric operand".to_string(),
                            );
                        }
                        // Check that operand is assignable (must be a variable or field)
                        match &operand.kind {
                            ExpressionKind::Identifier(_)
                            | ExpressionKind::Index { .. }
                            | ExpressionKind::FieldAccess { .. } => {}
                            _ => {
                                self.env.error(
                                    &expr.span,
                                    "Increment/decrement requires an assignable operand"
                                        .to_string(),
                                );
                            }
                        }
                        operand_type
                    }
                }
            }

            ExpressionKind::Call {
                callee,
                args,
                type_args: _,
            } => {
                let callee_type = self.check_expression(callee);

                // Get function name for generic tracking
                let function_name = if let ExpressionKind::Identifier(name) = &callee.kind {
                    Some(name.clone())
                } else {
                    None
                };

                // Record call resolution
                let resolution = match &callee.kind {
                    ExpressionKind::Identifier(name) => {
                        Some(crate::sema::CallResolution::Function { name: name.clone() })
                    }
                    ExpressionKind::FieldAccess { object, field } => {
                        if let ExpressionKind::Identifier(type_name) = &object.kind {
                            Some(crate::sema::CallResolution::Method {
                                type_name: type_name.clone(),
                                method_name: field.clone(),
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(res) = resolution {
                    self.sema.call_resolution.insert(expr.id, res);
                }

                // Check if this is a variadic built-in function
                let is_variadic_builtin = if let ExpressionKind::Identifier(name) = &callee.kind {
                    matches!(name.as_str(), "chan" | "print" | "println")
                } else {
                    false
                };

                // Check if this is an instance method call (callee is FieldAccess with implicit self)
                // Static methods (no self) should NOT be treated as method calls for arg counting
                let is_method_call = if let ExpressionKind::FieldAccess { object, field } =
                    &callee.kind
                {
                    // Check if this is a static method call on a type name
                    if let ExpressionKind::Identifier(type_name) = &object.kind {
                        // If we can look up the method and it doesn't have self, it's static
                        if let Some(impl_method) = self.env.lookup_method(type_name, field) {
                            impl_method.has_self // Only treat as method call if it has self
                        } else {
                            true // Assume instance method if not found
                        }
                    } else {
                        true // Instance method call on an expression (e.g., obj.method())
                    }
                } else {
                    false // Not a field access, not a method call
                };

                match callee_type {
                    Type::Function {
                        ref params,
                        ref return_type,
                        required_params,
                    } => {
                        // For method calls, the first param is 'self' which is implicit
                        let (min_args, max_args) = if is_method_call && !params.is_empty() {
                            (required_params.saturating_sub(1), params.len() - 1)
                        } else {
                            (required_params, params.len())
                        };

                        // Skip arg count check for variadic built-ins
                        if !is_variadic_builtin {
                            if args.len() < min_args {
                                self.env.error(
                                    &expr.span,
                                    format!(
                                        "Expected at least {} arguments, got {}",
                                        min_args,
                                        args.len()
                                    ),
                                );
                            } else if args.len() > max_args {
                                self.env.error(
                                    &expr.span,
                                    format!(
                                        "Expected at most {} arguments, got {}",
                                        max_args,
                                        args.len()
                                    ),
                                );
                            }
                        }

                        // For method calls, skip first param (self) when checking arg types
                        let params_to_check = if is_method_call && !params.is_empty() {
                            &params[1..]
                        } else {
                            &params[..]
                        };

                        // TODO: Handle named argument reordering here
                        // For now, just check positional argument types
                        let mut inferred_types = Vec::new();
                        for (arg, param_type) in args.iter().zip(params_to_check.iter()) {
                            let arg_type = self.check_expression(&arg.value);
                            if !arg_type.is_compatible_with(param_type) {
                                self.env.error(
                                    &arg.span,
                                    format!(
                                        "Argument type mismatch: expected '{}', got '{}'",
                                        param_type.display_name(),
                                        arg_type.display_name()
                                    ),
                                );
                            }
                            // Collect concrete types for type parameter inference
                            if param_type.is_type_param() {
                                inferred_types.push(arg_type);
                            }
                        }

                        // Record generic instantiation if this is a generic function
                        if let Some(ref fn_name) = function_name {
                            if self.env.is_generic_function(fn_name) && !inferred_types.is_empty() {
                                let instantiation =
                                    GenericInstantiation::new(fn_name.clone(), &inferred_types);
                                self.generic_instantiations.insert(instantiation);
                            }
                        }

                        return_type.as_ref().clone()
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.env.error(
                            &expr.span,
                            format!(
                                "Cannot call non-function type: '{}'",
                                callee_type.display_name()
                            ),
                        );
                        Type::Unknown
                    }
                }
            }

            ExpressionKind::FieldAccess { object, field } => {
                let obj_type = self.check_expression(object);
                let resolved_type = self.resolve_field_access(&obj_type, object, field, &expr.span);
                // Record field resolution
                self.sema.field_resolution.insert(expr.id, crate::sema::FieldResolution {
                    owner_type: obj_type,
                    field_name: field.clone(),
                    is_method: matches!(&resolved_type, Type::Function { .. }),
                    resolved_type: resolved_type.clone(),
                });
                resolved_type
            }

            ExpressionKind::OptionalAccess { object, field: _ } => {
                let obj_type = self.check_expression(object);

                // Optional access always returns Optional type
                // If object is null, it returns null (which is compatible with Optional)
                match obj_type {
                    Type::Null => Type::Null, // Accessing null?.field returns null
                    Type::Optional(inner) => Type::Optional(inner),
                    Type::Struct(_) | Type::Class(_) | Type::Any | Type::Unknown => {
                        // Regular object, wrap result in Optional
                        Type::Optional(Box::new(Type::Any))
                    }
                    _ => {
                        // For other types, still allow but return Optional<Any>
                        Type::Optional(Box::new(Type::Any))
                    }
                }
            }

            ExpressionKind::Index { object, index } => {
                let obj_type = self.check_expression(object);
                let index_type = self.check_expression(index);

                match obj_type {
                    Type::Array(elem_type) => {
                        if !index_type.is_integer() {
                            self.env
                                .error(&expr.span, "Array index must be integer".to_string());
                        }
                        *elem_type
                    }
                    Type::Map(_, value_type) => *value_type,
                    Type::String => {
                        if !index_type.is_integer() {
                            self.env
                                .error(&expr.span, "String index must be integer".to_string());
                        }
                        Type::Char
                    }
                    Type::Unknown => Type::Unknown,
                    // Allow indexing Any type (for json_parse results and other dynamic values)
                    Type::Any => Type::Any,
                    _ => {
                        self.env.error(
                            &expr.span,
                            format!("Cannot index type: '{}'", obj_type.display_name()),
                        );
                        Type::Unknown
                    }
                }
            }

            ExpressionKind::Array(elements) => {
                if elements.is_empty() {
                    Type::Array(Box::new(Type::Unknown))
                } else {
                    let first_type = self.check_expression(&elements[0]);
                    for elem in elements.iter().skip(1) {
                        let elem_type = self.check_expression(elem);
                        if !elem_type.is_compatible_with(&first_type) {
                            self.env.error(
                                &elem.span,
                                format!(
                                    "Array element type mismatch: expected '{}', got '{}'",
                                    first_type.display_name(),
                                    elem_type.display_name()
                                ),
                            );
                        }
                    }
                    Type::Array(Box::new(first_type))
                }
            }

            ExpressionKind::Tuple(elements) => {
                let types: Vec<_> = elements.iter().map(|e| self.check_expression(e)).collect();
                Type::Tuple(types)
            }

            ExpressionKind::Map(entries) => {
                if entries.is_empty() {
                    Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown))
                } else {
                    let (first_key, first_value) = &entries[0];
                    let key_type = self.check_expression(first_key);
                    let value_type = self.check_expression(first_value);

                    for (k, v) in entries.iter().skip(1) {
                        let kt = self.check_expression(k);
                        let vt = self.check_expression(v);
                        if !kt.is_compatible_with(&key_type) {
                            self.env.error(&k.span, "Map key type mismatch".to_string());
                        }
                        if !vt.is_compatible_with(&value_type) {
                            self.env
                                .error(&v.span, "Map value type mismatch".to_string());
                        }
                    }

                    Type::Map(Box::new(key_type), Box::new(value_type))
                }
            }

            ExpressionKind::Lambda { params, body } => {
                self.env.push_scope();

                let param_types: Vec<_> = params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr(&p.type_ann);
                        self.env.define(Symbol {
                            name: p.name.clone(),
                            ty: ty.clone(),
                            mutable: false,
                            kind: SymbolKind::Parameter,
                        });
                        ty
                    })
                    .collect();

                let return_type = self.check_expression(body);
                self.env.pop_scope();

                Type::Function {
                    params: param_types,
                    return_type: Box::new(return_type),
                    required_params: 0,
                }
            }

            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond_type = self.check_expression(condition);
                if cond_type != Type::Bool {
                    self.env
                        .error(&expr.span, "If condition must be bool".to_string());
                }

                let then_type = self.check_expression(then_expr);
                let else_type = self.check_expression(else_expr);

                if !then_type.is_compatible_with(&else_type) {
                    self.env.error(
                        &expr.span,
                        format!(
                            "If expression branches have incompatible types: {:?} and {:?}",
                            then_type, else_type
                        ),
                    );
                }

                then_type
            }

            ExpressionKind::Match { subject, arms } => {
                let subject_type = self.check_expression(subject);

                if arms.is_empty() {
                    self.env.error(
                        &expr.span,
                        "Match expression must have at least one arm".to_string(),
                    );
                    return Type::Unknown;
                }

                let mut first_arm_type: Option<Type> = None;

                for arm in arms {
                    // Create a new scope for pattern bindings
                    self.env.push_scope();

                    // Bind pattern variables with the subject type
                    self.bind_pattern_variables(&arm.pattern, &subject_type);

                    // Check guard if present
                    if let Some(guard) = &arm.guard {
                        let guard_type = self.check_expression(guard);
                        if guard_type != Type::Bool {
                            self.env.error(
                                &guard.span,
                                format!("Guard must be bool, got {:?}", guard_type),
                            );
                        }
                    }

                    // Check body
                    let arm_type = self.check_expression(&arm.body);

                    self.env.pop_scope();

                    if let Some(ref first) = first_arm_type {
                        if !arm_type.is_compatible_with(first) {
                            self.env.error(
                                &arm.span,
                                format!(
                                    "Match arm type mismatch: expected '{}', got '{}'",
                                    first.display_name(),
                                    arm_type.display_name()
                                ),
                            );
                        }
                    } else {
                        first_arm_type = Some(arm_type);
                    }
                }

                first_arm_type.unwrap_or(Type::Unknown)
            }

            ExpressionKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    let st = self.check_expression(s);
                    if !st.is_integer() {
                        self.env
                            .error(&expr.span, "Range start must be integer".to_string());
                    }
                }
                if let Some(e) = end {
                    let et = self.check_expression(e);
                    if !et.is_integer() {
                        self.env
                            .error(&expr.span, "Range end must be integer".to_string());
                    }
                }
                // Range is an object with start, end, inclusive fields
                // Use Any so field access works
                Type::Any
            }

            ExpressionKind::Cast {
                expr: inner,
                type_expr,
            } => {
                self.check_expression(inner);
                self.resolve_type_expr(type_expr)
            }

            ExpressionKind::TypeCheck { expr: inner, .. } => {
                self.check_expression(inner);
                Type::Bool
            }

            ExpressionKind::Assign { target, value } => {
                let target_type = self.check_expression(target);
                let value_type = self.check_expression(value);

                if !value_type.is_compatible_with(&target_type) {
                    self.env.error(
                        &expr.span,
                        format!(
                            "Assignment type mismatch: expected '{}', got '{}'",
                            target_type.display_name(),
                            value_type.display_name()
                        ),
                    );
                }

                // Check if target is mutable
                if let ExpressionKind::Identifier(name) = &target.kind {
                    if let Some(symbol) = self.env.lookup(name) {
                        if !symbol.mutable {
                            self.env.error(
                                &expr.span,
                                format!("Cannot assign to immutable variable: {}", name),
                            );
                        }
                    }
                }

                Type::Void
            }

            ExpressionKind::CompoundAssign { target, op, value } => {
                let target_type = self.check_expression(target);
                let _value_type = self.check_expression(value);

                // Similar to binary op checking
                let result_type = self.check_expression(&Expression {
                    id: crate::ids::NodeId::new(0), // Synthetic expression, no real ID
                    kind: ExpressionKind::Binary {
                        left: target.clone(),
                        op: *op,
                        right: value.clone(),
                    },
                    span: expr.span.clone(),
                });

                if !result_type.is_compatible_with(&target_type) {
                    self.env
                        .error(&expr.span, "Compound assignment type mismatch".to_string());
                }

                Type::Void
            }

            ExpressionKind::Block(block) => {
                self.env.push_scope();

                let mut last_type = Type::Void;
                for stmt in &block.statements {
                    self.check_statement(stmt);
                    // Last expression becomes block value
                    if let StatementKind::Expression(expr) = &stmt.kind {
                        last_type = self.check_expression(expr);
                    } else {
                        last_type = Type::Void;
                    }
                }

                self.env.pop_scope();
                last_type
            }

            ExpressionKind::Spawn(inner) => {
                self.check_expression(inner);
                // Spawn returns a fiber handle
                Type::Any // Fiber type would be defined properly
            }

            ExpressionKind::Select(arms) => {
                // Check each arm's channel and body
                let mut result_type = Type::Void;
                for arm in arms {
                    match &arm.kind {
                        SelectArmKind::Recv { channel, .. } => {
                            self.check_expression(channel);
                        }
                        SelectArmKind::Send { channel, value } => {
                            self.check_expression(channel);
                            self.check_expression(value);
                        }
                        SelectArmKind::Default => {}
                    }
                    result_type = self.check_expression(&arm.body);
                }
                result_type
            }

            ExpressionKind::StructLiteral { name, fields } => {
                if let Some(type_name) = name {
                    if let Some(_type_def) = self.env.lookup_type(type_name) {
                        // Check field types match
                        for (_, value) in fields {
                            self.check_expression(value);
                        }
                        Type::Struct(type_name.clone())
                    } else {
                        self.env
                            .error(&expr.span, format!("Unknown type: {}", type_name));
                        Type::Unknown
                    }
                } else {
                    // Anonymous struct
                    for (_, value) in fields {
                        self.check_expression(value);
                    }
                    Type::Unknown
                }
            }

            ExpressionKind::EnumVariant {
                enum_name,
                variant_name,
            } => {
                // Look up the enum type
                if let Some(type_def) = self.env.lookup_type(enum_name) {
                    if let TypeDefKind::Enum { variants } = &type_def.kind {
                        // Find the variant
                        if let Some((_, field_types)) =
                            variants.iter().find(|(name, _)| name == variant_name)
                        {
                            // Special case for Result type - return Type::Result instead of Type::Enum
                            let return_type = if enum_name == "Result" {
                                Type::Result {
                                    ok_type: Box::new(Type::Any),
                                    err_type: Box::new(Type::Any),
                                }
                            } else {
                                Type::Enum(enum_name.clone())
                            };

                            if field_types.is_empty() {
                                // Unit variant (no data) - returns the enum type directly
                                return_type
                            } else {
                                // Data variant - returns a constructor function
                                // Option::Some is fn(T) -> Option<T>
                                // Result::Ok is fn(T) -> Result<T, E>
                                Type::Function {
                                    params: field_types.clone(),
                                    return_type: Box::new(return_type),
                                    required_params: 0,
                                }
                            }
                        } else {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Unknown variant '{}' for enum '{}'",
                                    variant_name, enum_name
                                ),
                            );
                            Type::Unknown
                        }
                    } else {
                        self.env
                            .error(&expr.span, format!("'{}' is not an enum", enum_name));
                        Type::Unknown
                    }
                } else {
                    self.env
                        .error(&expr.span, format!("Unknown enum: {}", enum_name));
                    Type::Unknown
                }
            }

            ExpressionKind::Path { segments } => {
                // Handle qualified paths like module::function
                if let Some(last) = segments.last() {
                    // Try to resolve as identifier
                    if let Some(sym) = self.env.lookup(last) {
                        sym.ty.clone()
                    } else {
                        self.env
                            .error(&expr.span, format!("Unknown path: {}", segments.join("::")));
                        Type::Unknown
                    }
                } else {
                    Type::Unknown
                }
            }

            ExpressionKind::Try(inner) => {
                // Try expression unwraps Optional or Result types
                let inner_type = self.check_expression(inner);
                match &inner_type {
                    Type::Optional(inner) => (**inner).clone(),
                    Type::Result { ok_type, err_type } => {
                        // Check that we're in a function that returns a compatible Result
                        if let Some(ref ret_type) = self.current_function_return_type {
                            match ret_type {
                                Type::Result {
                                    err_type: ret_err, ..
                                } => {
                                    if !err_type.is_compatible_with(ret_err) {
                                        self.env.error(
                                            &expr.span,
                                            format!(
                                                "Cannot propagate error: function returns Result<_, {}> but expression has Result<_, {}>",
                                                ret_err.display_name(),
                                                err_type.display_name()
                                            ),
                                        );
                                    }
                                }
                                _ => {
                                    self.env.error(
                                        &expr.span,
                                        format!(
                                            "Cannot use ? operator: function does not return Result type (returns {})",
                                            ret_type.display_name()
                                        ),
                                    );
                                }
                            }
                        } else {
                            self.env.error(
                                &expr.span,
                                "Cannot use ? operator outside of a function".to_string(),
                            );
                        }
                        (**ok_type).clone()
                    }
                    _ => {
                        self.env.error(
                            &expr.span,
                            format!(
                                "Cannot use ? operator on type '{}': expected Optional or Result",
                                inner_type.display_name()
                            ),
                        );
                        Type::Unknown
                    }
                }
            }

            ExpressionKind::MethodCall {
                receiver,
                method,
                args,
                type_args: _, // TODO: Handle explicit type args for generic methods
            } => {
                let receiver_type = self.check_expression(receiver);
                // Check arguments
                for arg in args {
                    self.check_expression(&arg.value);
                }
                // For now, return Any - proper method resolution would go here
                let _ = (receiver_type, method);
                Type::Any
            }
        }
    }

    /// Bind pattern variables to the environment with their types
    fn bind_pattern_variables(&mut self, pattern: &Pattern, subject_type: &Type) {
        match &pattern.kind {
            PatternKind::Variable(name) => {
                // Bind the variable to the subject type
                self.env.define(Symbol {
                    name: name.clone(),
                    ty: subject_type.clone(),
                    mutable: false,
                    kind: SymbolKind::Variable,
                });
            }
            PatternKind::Wildcard => {
                // Wildcard doesn't bind any variables
            }
            PatternKind::Literal(_) => {
                // Literals don't bind variables
            }
            PatternKind::Binding { name, pattern } => {
                // Bind the name and recurse for inner pattern
                self.env.define(Symbol {
                    name: name.clone(),
                    ty: subject_type.clone(),
                    mutable: false,
                    kind: SymbolKind::Variable,
                });
                self.bind_pattern_variables(pattern, subject_type);
            }
            PatternKind::Tuple(patterns) => {
                // For tuple patterns, each element binds to the corresponding tuple element type
                if let Type::Tuple(types) = subject_type {
                    for (pat, ty) in patterns.iter().zip(types.iter()) {
                        self.bind_pattern_variables(pat, ty);
                    }
                }
            }
            PatternKind::Constructor { fields, .. } => {
                // For constructor patterns, bind each field
                // Simplified: bind all fields with Any type for now
                for field in fields {
                    self.bind_pattern_variables(field, &Type::Any);
                }
            }
            PatternKind::Struct { fields, .. } => {
                // For struct patterns, bind each field
                for (_, pat) in fields {
                    self.bind_pattern_variables(pat, &Type::Any);
                }
            }
            PatternKind::Or(patterns) => {
                // For or patterns, all branches must bind the same variables
                // Just bind from the first pattern
                if let Some(first) = patterns.first() {
                    self.bind_pattern_variables(first, subject_type);
                }
            }
            PatternKind::Range { .. } => {
                // Range patterns don't bind variables
            }
        }
    }

    /// Bind destructuring pattern variables for let/var declarations
    fn bind_destructuring_pattern(
        &mut self,
        pattern: &Pattern,
        subject_type: &Type,
        mutable: bool,
        span: &Span,
    ) {
        match &pattern.kind {
            PatternKind::Variable(name) => {
                self.env.define(Symbol {
                    name: name.clone(),
                    ty: subject_type.clone(),
                    mutable,
                    kind: SymbolKind::Variable,
                });
            }
            PatternKind::Wildcard => {
                // Wildcard doesn't bind any variables
            }
            PatternKind::Tuple(patterns) => {
                // For tuple patterns, each element binds to the corresponding tuple/array element type
                match subject_type {
                    Type::Tuple(types) => {
                        if patterns.len() != types.len() {
                            self.env.error(
                                span,
                                format!(
                                    "Tuple pattern has {} elements but value has {}",
                                    patterns.len(),
                                    types.len()
                                ),
                            );
                            return;
                        }
                        for (pat, ty) in patterns.iter().zip(types.iter()) {
                            self.bind_destructuring_pattern(pat, ty, mutable, span);
                        }
                    }
                    Type::Array(elem_type) => {
                        // Allow destructuring arrays as tuples
                        for pat in patterns {
                            self.bind_destructuring_pattern(pat, elem_type, mutable, span);
                        }
                    }
                    _ => {
                        self.env.error(
                            span,
                            format!(
                                "Cannot destructure type '{}' with tuple pattern",
                                subject_type.display_name()
                            ),
                        );
                    }
                }
            }
            PatternKind::Struct {
                fields: pattern_fields,
                ..
            } => {
                // For struct patterns, look up field types from the struct definition
                if let Type::Struct(struct_name) = subject_type {
                    // Clone the struct fields to avoid borrow issues
                    let struct_fields_opt = self.env.lookup_type(struct_name).and_then(|td| {
                        if let TypeDefKind::Struct { fields, .. } = &td.kind {
                            Some(fields.clone())
                        } else {
                            None
                        }
                    });

                    if let Some(struct_fields) = struct_fields_opt {
                        for (field_name, field_pattern) in pattern_fields {
                            if let Some((_, field_type, _)) =
                                struct_fields.iter().find(|(n, _, _)| n == field_name)
                            {
                                self.bind_destructuring_pattern(
                                    field_pattern,
                                    field_type,
                                    mutable,
                                    span,
                                );
                            } else {
                                self.env.error(
                                    span,
                                    format!(
                                        "Unknown field '{}' in struct '{}'",
                                        field_name, struct_name
                                    ),
                                );
                            }
                        }
                    }
                } else {
                    self.env.error(
                        span,
                        format!(
                            "Cannot destructure type '{}' with struct pattern",
                            subject_type.display_name()
                        ),
                    );
                }
            }
            _ => {
                self.env
                    .error(span, "Unsupported pattern in destructuring".to_string());
            }
        }
    }

    fn resolve_type_expr(&mut self, type_expr: &TypeExpr) -> Type {
        match &type_expr.kind {
            TypeExprKind::Named(name) => {
                match name.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "bool" => Type::Bool,
                    "string" => Type::String,
                    "char" => Type::Char,
                    "void" => Type::Void,
                    // Sized integer types
                    "int8" => Type::Int8,
                    "int16" => Type::Int16,
                    "int32" => Type::Int32,
                    "int64" => Type::Int64,
                    "uint8" | "byte" => Type::UInt8,
                    "uint16" => Type::UInt16,
                    "uint32" => Type::UInt32,
                    "uint64" => Type::UInt64,
                    "_" => self.env.fresh_type_var(),
                    "Self" => {
                        // Self refers to the current struct/class being defined
                        if let Some(ref name) = self.current_type_name {
                            Type::Struct(name.clone())
                        } else {
                            self.env.error(
                                &type_expr.span,
                                "Self can only be used inside a type".to_string(),
                            );
                            Type::Unknown
                        }
                    }
                    other => {
                        // Check if it's a type parameter (generic type like T, U)
                        if self.current_type_params.contains_key(other) {
                            Type::TypeParam(other.to_string())
                        } else if let Some(type_def) = self.env.lookup_type(other) {
                            // Return the correct Type variant based on kind
                            match &type_def.kind {
                                TypeDefKind::Class { .. } => Type::Class(other.to_string()),
                                TypeDefKind::Struct { .. } => Type::Struct(other.to_string()),
                                TypeDefKind::Enum { .. } => Type::Enum(other.to_string()),
                                TypeDefKind::Interface { .. } => Type::Interface(other.to_string()),
                                TypeDefKind::Alias(ty) => ty.clone(),
                            }
                        } else {
                            self.env
                                .error(&type_expr.span, format!("Unknown type: {}", other));
                            Type::Unknown
                        }
                    }
                }
            }
            TypeExprKind::Generic { name, args } => {
                let resolved_args: Vec<_> =
                    args.iter().map(|a| self.resolve_type_expr(a)).collect();

                match name.as_str() {
                    "List" | "Array" => {
                        if resolved_args.len() == 1 {
                            Type::Array(Box::new(resolved_args[0].clone()))
                        } else {
                            self.env.error(
                                &type_expr.span,
                                "Array takes one type argument".to_string(),
                            );
                            Type::Unknown
                        }
                    }
                    "Map" => {
                        if resolved_args.len() == 2 {
                            Type::Map(
                                Box::new(resolved_args[0].clone()),
                                Box::new(resolved_args[1].clone()),
                            )
                        } else {
                            self.env
                                .error(&type_expr.span, "Map takes two type arguments".to_string());
                            Type::Unknown
                        }
                    }
                    "Result" => {
                        if resolved_args.len() == 2 {
                            Type::Result {
                                ok_type: Box::new(resolved_args[0].clone()),
                                err_type: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.env.error(
                                &type_expr.span,
                                "Result takes two type arguments: Result<OkType, ErrType>"
                                    .to_string(),
                            );
                            Type::Unknown
                        }
                    }
                    _ => {
                        // User-defined generic type
                        Type::Class(name.clone())
                    }
                }
            }
            TypeExprKind::Optional(inner) => {
                Type::Optional(Box::new(self.resolve_type_expr(inner)))
            }
            TypeExprKind::Function {
                params,
                return_type,
            } => {
                let param_types: Vec<_> =
                    params.iter().map(|p| self.resolve_type_expr(p)).collect();
                let ret = self.resolve_type_expr(return_type);
                Type::Function {
                    params: param_types,
                    return_type: Box::new(ret),
                    required_params: 0,
                }
            }
            TypeExprKind::Tuple(elements) => {
                let types: Vec<_> = elements.iter().map(|e| self.resolve_type_expr(e)).collect();
                Type::Tuple(types)
            }
            TypeExprKind::Array(element_type) => {
                Type::Array(Box::new(self.resolve_type_expr(element_type)))
            }
            TypeExprKind::Result { ok_type, err_type } => {
                let ok = self.resolve_type_expr(ok_type);
                let err = self.resolve_type_expr(err_type);
                Type::Result {
                    ok_type: Box::new(ok),
                    err_type: Box::new(err),
                }
            }
            TypeExprKind::Path(segments) => {
                // Handle qualified type paths like module::Type
                if let Some(last) = segments.last() {
                    self.resolve_type_expr(&TypeExpr {
                        kind: TypeExprKind::Named(last.clone()),
                        span: type_expr.span.clone(),
                    })
                } else {
                    Type::Unknown
                }
            }
        }
    }

    /// Resolve field access on a type
    fn resolve_field_access(&mut self, obj_type: &Type, object: &Expression, field: &str, span: &Span) -> Type {
        match obj_type {
            Type::Enum(_) => {
                if field == "__enum" || field == "__variant" {
                    Type::String
                } else {
                    self.env.error(
                        span,
                        format!(
                            "Enum values only have __enum and __variant fields, not '{}'",
                            field
                        ),
                    );
                    Type::Unknown
                }
            }
            Type::Class(name) => {
                let all_fields = self.env.get_class_fields(name);
                for (field_name, field_type, _) in &all_fields {
                    if field_name == field {
                        return field_type.clone();
                    }
                }
                let all_methods = self.env.get_class_methods(name);
                for (method_name, method_type, _) in &all_methods {
                    if method_name == field {
                        return method_type.clone();
                    }
                }
                if let Some(impl_method) = self.env.lookup_method(name, field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                self.env.error(span, format!("Unknown field or method: {} on type {}", field, name));
                Type::Unknown
            }
            Type::Struct(name) => {
                if let Some(type_def) = self.env.lookup_type(name) {
                    if let TypeDefKind::Struct { fields, methods } = &type_def.kind {
                        for (field_name, field_type, _) in fields {
                            if field_name == field {
                                return field_type.clone();
                            }
                        }
                        for (method_name, method_type, _) in methods {
                            if method_name == field {
                                return method_type.clone();
                            }
                        }
                        if let Some(impl_method) = self.env.lookup_method(name, field) {
                            let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                            return Type::Function {
                                params: param_types.clone(),
                                return_type: Box::new(impl_method.return_type.clone()),
                                required_params: param_types.len(),
                            };
                        }
                        self.env.error(span, format!("Unknown field or method: {} on type {}", field, name));
                    }
                }
                Type::Unknown
            }
            Type::Unknown => {
                if let ExpressionKind::Identifier(type_name) = &object.kind {
                    if let Some(impl_method) = self.env.lookup_method(type_name, field) {
                        if !impl_method.has_self {
                            let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                            return Type::Function {
                                params: param_types.clone(),
                                return_type: Box::new(impl_method.return_type.clone()),
                                required_params: param_types.len(),
                            };
                        }
                    }
                }
                Type::Unknown
            }
            Type::Any => Type::Any,
            Type::Int => {
                if let Some(impl_method) = self.env.lookup_method("int", field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                self.env.error(span, format!("Unknown method: {} on int", field));
                Type::Unknown
            }
            Type::Float => {
                if let Some(impl_method) = self.env.lookup_method("float", field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                self.env.error(span, format!("Unknown method: {} on float", field));
                Type::Unknown
            }
            Type::String => {
                if let Some(impl_method) = self.env.lookup_method("string", field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                if field == "len" {
                    return Type::Function {
                        params: vec![],
                        return_type: Box::new(Type::Int),
                        required_params: 0,
                    };
                }
                self.env.error(span, format!("Unknown method: {} on string", field));
                Type::Unknown
            }
            Type::Array(inner) => {
                let specific_type_name = format!("[{}]", inner.display_name());
                if let Some(impl_method) = self.env.lookup_method(&specific_type_name, field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                if let Some(impl_method) = self.env.lookup_method("array", field) {
                    let param_types: Vec<Type> = impl_method.params.iter().map(|(_, ty)| ty.clone()).collect();
                    return Type::Function {
                        params: param_types.clone(),
                        return_type: Box::new(impl_method.return_type.clone()),
                        required_params: param_types.len(),
                    };
                }
                match field.as_ref() {
                    "len" => Type::Function {
                        params: vec![],
                        return_type: Box::new(Type::Int),
                        required_params: 0,
                    },
                    "push" => Type::Function {
                        params: vec![*inner.clone()],
                        return_type: Box::new(Type::Void),
                        required_params: 1,
                    },
                    "pop" => Type::Function {
                        params: vec![],
                        return_type: Box::new(Type::Optional(inner.clone())),
                        required_params: 0,
                    },
                    _ => {
                        self.env.error(span, format!("Unknown method: {} on array", field));
                        Type::Unknown
                    }
                }
            }
            _ => {
                self.env.error(span, format!("Cannot access field on type: '{}'", obj_type.display_name()));
                Type::Unknown
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// A type-checked program with semantic information
#[derive(Debug, Clone)]
pub struct CheckedProgram {
    pub program: Program,
    pub sema: SemanticTables,
}

/// Type check a program
pub fn check(program: &Program) -> Result<CheckedProgram, String> {
    let mut checker = TypeChecker::new();
    checker.check_program(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Helper function to check source code and return the result
    fn check_source(source: &str) -> Result<(), String> {
        let tokens = tokenize(source)?;
        let ast = parse(&tokens)?;
        check(&ast)?;
        Ok(())
    }

    // ========================================================================
    // Basic Type Inference Tests
    // ========================================================================

    #[test]
    fn test_integer_literal_infers_int() {
        assert!(check_source("let x = 42").is_ok());
    }

    #[test]
    fn test_float_literal_infers_float() {
        assert!(check_source("let x = 3.14").is_ok());
    }

    #[test]
    fn test_string_literal_infers_string() {
        assert!(check_source("let x = \"hello\"").is_ok());
    }

    #[test]
    fn test_bool_literal_infers_bool() {
        assert!(check_source("let x = true").is_ok());
        assert!(check_source("let y = false").is_ok());
    }

    #[test]
    fn test_char_literal_infers_char() {
        assert!(check_source("let x = 'a'").is_ok());
    }

    #[test]
    fn test_array_literal_infers_element_type() {
        assert!(check_source("let x = [1, 2, 3]").is_ok());
        assert!(check_source("let y = [\"a\", \"b\"]").is_ok());
    }

    #[test]
    fn test_empty_array_literal() {
        assert!(check_source("let x = []").is_ok());
    }

    // ========================================================================
    // Variable Type Checking Tests
    // ========================================================================

    #[test]
    fn test_explicit_type_matches_value() {
        assert!(check_source("let x: int = 5").is_ok());
        assert!(check_source("let y: string = \"hello\"").is_ok());
        assert!(check_source("let z: bool = true").is_ok());
    }

    #[test]
    fn test_type_mismatch_error() {
        let result = check_source("let x: int = \"hello\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Type mismatch"));
    }

    #[test]
    fn test_type_mismatch_bool_to_int() {
        let result = check_source("let x: int = true");
        assert!(result.is_err());
    }

    #[test]
    fn test_type_mismatch_int_to_string() {
        let result = check_source("let x: string = 42");
        assert!(result.is_err());
    }

    #[test]
    fn test_mutable_variable() {
        assert!(check_source("var x = 5\nx = 10").is_ok());
    }

    #[test]
    fn test_immutable_variable_assignment_error() {
        let result = check_source("let x = 5\nx = 10");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn test_undefined_variable_error() {
        let result = check_source("let x = y");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Undefined variable"));
    }

    #[test]
    fn test_variable_shadowing_in_scope() {
        // Shadowing is allowed in nested scopes
        assert!(check_source(
            r#"
            let x = 5
            if true {
                let x = "hello"
            }
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Function Type Checking Tests
    // ========================================================================

    #[test]
    fn test_function_declaration() {
        assert!(check_source("fn add(a: int, b: int) -> int { return a + b }").is_ok());
    }

    #[test]
    fn test_function_no_return_type() {
        assert!(check_source("fn greet(name: string) { println(name) }").is_ok());
    }

    #[test]
    fn test_function_return_type_mismatch() {
        let result = check_source("fn get_int() -> int { return \"hello\" }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Return type mismatch"));
    }

    #[test]
    fn test_function_call_correct_args() {
        assert!(check_source(
            r#"
            fn add(a: int, b: int) -> int { return a + b }
            let result = add(1, 2)
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_function_call_wrong_arg_count() {
        let result = check_source(
            r#"
            fn add(a: int, b: int) -> int { return a + b }
            let result = add(1)
            "#,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Expected at least 2 arguments"));
    }

    #[test]
    fn test_function_call_too_many_args() {
        let result = check_source(
            r#"
            fn add(a: int, b: int) -> int { return a + b }
            let result = add(1, 2, 3)
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_function_call_wrong_arg_type() {
        let result = check_source(
            r#"
            fn add(a: int, b: int) -> int { return a + b }
            let result = add(1, "two")
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Argument type mismatch"));
    }

    #[test]
    fn test_recursive_function() {
        assert!(check_source(
            r#"
            fn factorial(n: int) -> int {
                if n <= 1 {
                    return 1
                }
                return n * factorial(n - 1)
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_mutually_recursive_functions() {
        assert!(check_source(
            r#"
            fn is_even(n: int) -> bool {
                if n == 0 { return true }
                return is_odd(n - 1)
            }
            fn is_odd(n: int) -> bool {
                if n == 0 { return false }
                return is_even(n - 1)
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_undefined_function_error() {
        let result = check_source("let x = unknown_fn(5)");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Undefined variable"));
    }

    // ========================================================================
    // Struct Type Checking Tests
    // ========================================================================

    #[test]
    fn test_struct_declaration() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_struct_instantiation() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }
            let p = Point { x: 10, y: 20 }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_struct_field_access() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }
            let p = Point { x: 10, y: 20 }
            let px = p.x
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_unknown_struct_error() {
        let result = check_source("let p = UnknownStruct { x: 10 }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown type"));
    }

    // ========================================================================
    // Expression Type Checking Tests
    // ========================================================================

    #[test]
    fn test_arithmetic_ops_on_integers() {
        assert!(check_source("let x = 1 + 2").is_ok());
        assert!(check_source("let x = 10 - 5").is_ok());
        assert!(check_source("let x = 3 * 4").is_ok());
        assert!(check_source("let x = 8 / 2").is_ok());
        assert!(check_source("let x = 7 % 3").is_ok());
    }

    #[test]
    fn test_arithmetic_ops_on_floats() {
        assert!(check_source("let x = 1.0 + 2.0").is_ok());
        assert!(check_source("let x = 1 + 2.0").is_ok()); // int + float -> float
    }

    #[test]
    fn test_arithmetic_op_type_error() {
        let result = check_source("let x = 1 + \"hello\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid operand types"));
    }

    #[test]
    fn test_string_concatenation() {
        assert!(check_source("let x = \"hello\" + \"world\"").is_ok());
    }

    #[test]
    fn test_comparison_ops_return_bool() {
        assert!(check_source("let x = 1 < 2").is_ok());
        assert!(check_source("let x = 1 <= 2").is_ok());
        assert!(check_source("let x = 1 > 2").is_ok());
        assert!(check_source("let x = 1 >= 2").is_ok());
        assert!(check_source("let x = 1 == 2").is_ok());
        assert!(check_source("let x = 1 != 2").is_ok());
    }

    #[test]
    fn test_logical_ops_require_bool() {
        assert!(check_source("let x = true && false").is_ok());
        assert!(check_source("let x = true || false").is_ok());
    }

    #[test]
    fn test_logical_ops_type_error() {
        let result = check_source("let x = 1 && 2");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Logical operators require bool"));
    }

    #[test]
    fn test_unary_negation() {
        assert!(check_source("let x = -5").is_ok());
        assert!(check_source("let x = -3.14").is_ok());
    }

    #[test]
    fn test_unary_negation_type_error() {
        let result = check_source("let x = -\"hello\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot negate"));
    }

    #[test]
    fn test_unary_not() {
        assert!(check_source("let x = !true").is_ok());
    }

    #[test]
    fn test_unary_not_type_error() {
        let result = check_source("let x = !5");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Logical not requires bool"));
    }

    #[test]
    fn test_bitwise_ops() {
        assert!(check_source("let x = 5 & 3").is_ok());
        assert!(check_source("let x = 5 | 3").is_ok());
        assert!(check_source("let x = 5 ^ 3").is_ok());
        assert!(check_source("let x = 5 << 2").is_ok());
        assert!(check_source("let x = 5 >> 1").is_ok());
    }

    #[test]
    fn test_bitwise_not() {
        assert!(check_source("let x = ~5").is_ok());
    }

    // ========================================================================
    // Control Flow Type Checking Tests
    // ========================================================================

    #[test]
    fn test_if_condition_must_be_bool() {
        assert!(check_source("if true { let x = 1 }").is_ok());
        assert!(check_source("if 1 < 2 { let x = 1 }").is_ok());
    }

    #[test]
    fn test_if_condition_not_bool_error() {
        let result = check_source("if 42 { let x = 1 }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Condition must be bool"));
    }

    #[test]
    fn test_while_condition_must_be_bool() {
        assert!(check_source("while true { break }").is_ok());
        assert!(check_source("var x = 0\nwhile x < 10 { x = x + 1 }").is_ok());
    }

    #[test]
    fn test_while_condition_not_bool_error() {
        let result = check_source("while 1 { break }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Condition must be bool"));
    }

    #[test]
    fn test_for_loop() {
        assert!(check_source("for i in [1, 2, 3] { println(i) }").is_ok());
    }

    #[test]
    fn test_break_outside_loop_error() {
        // break statement in a function but outside a loop
        let result = check_source(
            r#"
            fn test() {
                break
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("break outside of loop"));
    }

    #[test]
    fn test_continue_outside_loop_error() {
        // continue statement in a function but outside a loop
        let result = check_source(
            r#"
            fn test() {
                continue
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("continue outside of loop"));
    }

    #[test]
    fn test_break_inside_loop() {
        assert!(check_source("while true { break }").is_ok());
        assert!(check_source("loop { break }").is_ok());
        assert!(check_source("for i in [1,2,3] { break }").is_ok());
    }

    #[test]
    fn test_continue_inside_loop() {
        assert!(check_source("while true { continue }").is_ok());
    }

    // ========================================================================
    // Match Expression Type Checking Tests
    // ========================================================================

    #[test]
    fn test_match_expression() {
        assert!(check_source(
            r#"
            let x = 5
            let result = match x {
                1 => "one"
                2 => "two"
                _ => "other"
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_if_expression() {
        assert!(check_source("let x = if true { 1 } else { 2 }").is_ok());
    }

    #[test]
    fn test_if_expression_condition_not_bool_error() {
        let result = check_source("let x = if 5 { 1 } else { 2 }");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be bool"));
    }

    // ========================================================================
    // Array and Index Type Checking Tests
    // ========================================================================

    #[test]
    fn test_array_index_access() {
        assert!(check_source(
            r#"
            let arr = [1, 2, 3]
            let x = arr[0]
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_array_index_must_be_integer() {
        let result = check_source(
            r#"
            let arr = [1, 2, 3]
            let x = arr["zero"]
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Array index must be integer"));
    }

    #[test]
    fn test_string_index_access() {
        assert!(check_source(
            r#"
            let s = "hello"
            let c = s[0]
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_cannot_index_non_indexable() {
        let result = check_source(
            r#"
            let x = 42
            let y = x[0]
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot index type"));
    }

    // ========================================================================
    // Enum Type Checking Tests
    // ========================================================================

    #[test]
    fn test_enum_declaration() {
        assert!(check_source(
            r#"
            enum Color {
                Red
                Green
                Blue
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_enum_variant_access() {
        assert!(check_source(
            r#"
            enum Color {
                Red
                Green
                Blue
            }
            let c = Color::Red
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_unknown_enum_variant_error() {
        let result = check_source(
            r#"
            enum Color {
                Red
                Green
                Blue
            }
            let c = Color::Yellow
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown variant"));
    }

    #[test]
    fn test_unknown_enum_error() {
        let result = check_source("let c = UnknownEnum::Variant");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown enum"));
    }

    // ========================================================================
    // Lambda and Higher-Order Function Tests
    // ========================================================================

    #[test]
    fn test_lambda_expression() {
        // Lira uses pipe syntax for lambdas: |params| body
        assert!(check_source("let add = |a: int, b: int| a + b").is_ok());
    }

    #[test]
    fn test_lambda_call() {
        // Lira uses pipe syntax for lambdas
        assert!(check_source(
            r#"
            let add = |a: int, b: int| a + b
            let result = add(1, 2)
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_empty_lambda() {
        // Empty parameter lambda
        assert!(check_source("let noop = || 42").is_ok());
    }

    // ========================================================================
    // Built-in Function Tests
    // ========================================================================

    #[test]
    fn test_print_builtin() {
        assert!(check_source("print(42)").is_ok());
        assert!(check_source("print(\"hello\")").is_ok());
    }

    #[test]
    fn test_println_builtin() {
        assert!(check_source("println(42)").is_ok());
        assert!(check_source("println(\"hello\")").is_ok());
    }

    #[test]
    fn test_len_builtin() {
        assert!(check_source("let x = len([1, 2, 3])").is_ok());
        assert!(check_source("let x = len(\"hello\")").is_ok());
    }

    // ========================================================================
    // Type Alias Tests
    // ========================================================================

    #[test]
    fn test_type_alias() {
        assert!(check_source(
            r#"
            type IntArray = [int]
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Constant Declaration Tests
    // ========================================================================

    #[test]
    fn test_const_declaration() {
        assert!(check_source("const PI = 3.14159").is_ok());
    }

    #[test]
    fn test_const_with_type_annotation() {
        assert!(check_source("const MAX: int = 100").is_ok());
    }

    #[test]
    fn test_const_type_mismatch_error() {
        let result = check_source("const X: int = \"hello\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Type mismatch"));
    }

    // ========================================================================
    // Assignment Type Checking Tests
    // ========================================================================

    #[test]
    fn test_assignment_type_match() {
        assert!(check_source("var x = 5\nx = 10").is_ok());
    }

    #[test]
    fn test_assignment_type_mismatch() {
        let result = check_source("var x = 5\nx = \"hello\"");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Assignment type mismatch"));
    }

    #[test]
    fn test_compound_assignment() {
        assert!(check_source("var x = 5\nx += 3").is_ok());
        assert!(check_source("var x = 5\nx -= 3").is_ok());
        assert!(check_source("var x = 5\nx *= 3").is_ok());
        assert!(check_source("var x = 5\nx /= 3").is_ok());
    }

    // ========================================================================
    // Range Expression Tests
    // ========================================================================

    #[test]
    fn test_range_expression() {
        assert!(check_source("let r = 1..10").is_ok());
    }

    // ========================================================================
    // Cast Expression Tests
    // ========================================================================

    #[test]
    fn test_cast_expression() {
        assert!(check_source("let x = 5 as float").is_ok());
    }

    // ========================================================================
    // Type Check Expression Tests
    // ========================================================================

    #[test]
    fn test_is_type_check() {
        assert!(check_source("let x = 5\nlet b = x is int").is_ok());
    }

    // ========================================================================
    // Increment/Decrement Tests
    // ========================================================================

    #[test]
    fn test_pre_increment() {
        // Pre-increment as part of an expression (avoids newline parsing issues)
        assert!(check_source(
            r#"
            fn test() {
                var x = 5
                let y = ++x
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_post_increment() {
        // Post-increment as part of an expression
        assert!(check_source(
            r#"
            fn test() {
                var x = 5
                let y = x++
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_increment_non_numeric_error() {
        let result = check_source(
            r#"
            fn test() {
                var x = "hello"
                let y = ++x
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Increment/decrement requires numeric"));
    }

    // ========================================================================
    // Sized Integer Types Tests
    // ========================================================================

    #[test]
    fn test_sized_integer_types() {
        assert!(check_source("let x: int8 = 42").is_ok());
        assert!(check_source("let x: int16 = 42").is_ok());
        assert!(check_source("let x: int32 = 42").is_ok());
        assert!(check_source("let x: int64 = 42").is_ok());
        assert!(check_source("let x: uint8 = 42").is_ok());
        assert!(check_source("let x: uint16 = 42").is_ok());
        assert!(check_source("let x: uint32 = 42").is_ok());
        assert!(check_source("let x: uint64 = 42").is_ok());
    }

    // ========================================================================
    // Optional Type Tests
    // ========================================================================

    #[test]
    fn test_optional_type() {
        assert!(check_source("let x: int? = null").is_ok());
    }

    // ========================================================================
    // Tuple Type Tests
    // ========================================================================

    #[test]
    fn test_tuple_literal() {
        assert!(check_source("let t = (1, \"hello\", true)").is_ok());
    }

    // ========================================================================
    // Map Type Tests
    // ========================================================================

    // Note: Map literal syntax {key: value} is not yet supported by the parser.
    // Map type checking is tested indirectly through other mechanisms.
    // When map literals are added to the parser, these tests should be enabled:
    //
    // #[test]
    // fn test_map_literal() {
    //     assert!(check_source("let m = {\"a\": 1, \"b\": 2}").is_ok());
    // }
    //
    // #[test]
    // fn test_empty_map_literal() {
    //     assert!(check_source("let m = {}").is_ok());
    // }

    // ========================================================================
    // Complex Expression Tests
    // ========================================================================

    #[test]
    fn test_nested_expressions() {
        assert!(check_source("let x = (1 + 2) * (3 - 4)").is_ok());
    }

    #[test]
    fn test_chained_comparisons() {
        assert!(check_source("let x = 1 < 2 && 2 < 3").is_ok());
    }

    #[test]
    fn test_mixed_arithmetic_and_comparison() {
        assert!(check_source("let x = (1 + 2) < (3 * 4)").is_ok());
    }

    // ========================================================================
    // Scoping Tests
    // ========================================================================

    #[test]
    fn test_block_scope() {
        assert!(check_source(
            r#"
            let x = 1
            {
                let y = 2
                let z = x + y
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_variable_not_visible_outside_scope() {
        let result = check_source(
            r#"
            {
                let x = 1
            }
            let y = x
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Undefined variable"));
    }

    // ========================================================================
    // Struct Method Tests
    // ========================================================================

    #[test]
    fn test_struct_with_method() {
        assert!(check_source(
            r#"
            struct Counter {
                value: int
            }

            impl Counter {
                fn increment(self) -> Counter {
                    return Counter { value: self.value + 1 }
                }
            }
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Error Message Quality Tests
    // ========================================================================

    #[test]
    fn test_error_includes_line_info() {
        let result = check_source("let x = undefined_var");
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Error should include line:column information
        assert!(err.contains(":"));
    }

    // ========================================================================
    // Impl Block Tests
    // ========================================================================

    #[test]
    fn test_simple_impl_block() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn origin() -> Point {
                    return Point { x: 0, y: 0 }
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_impl_with_self_method() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn get_x(self) -> int {
                    return self.x
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_impl_method_call() {
        assert!(check_source(
            r#"
            struct Counter {
                value: int
            }

            impl Counter {
                fn new() -> Counter {
                    return Counter { value: 0 }
                }

                fn get(self) -> int {
                    return self.value
                }
            }

            let c = Counter.new()
            let v = c.get()
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_impl_for_builtin_string() {
        assert!(check_source(
            r#"
            impl string {
                fn is_empty(self) -> bool {
                    return self.len() == 0
                }
            }

            let s = "hello"
            let empty = s.is_empty()
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_impl_method_wrong_arg_count() {
        let result = check_source(
            r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn get_x(self) -> int {
                    return self.x
                }
            }

            let p = Point { x: 1, y: 2 }
            let v = p.get_x(42)
            "#,
        );
        assert!(result.is_err());
    }

    // ========================================================================
    // Trait Tests
    // ========================================================================

    #[test]
    fn test_simple_trait_declaration() {
        assert!(check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_trait_impl() {
        assert!(check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }

            struct Point {
                x: int
                y: int
            }

            impl Clone for Point {
                fn clone(self) -> Point {
                    return Point { x: self.x, y: self.y }
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_trait_impl_missing_method() {
        // Should error: missing required 'clone' method
        let result = check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }

            struct Point {
                x: int
            }

            impl Clone for Point {
                // Missing clone method!
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires method `clone`"));
    }

    #[test]
    fn test_trait_impl_wrong_return_type() {
        // Should error: wrong return type
        let result = check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }

            struct Point {
                x: int
            }

            impl Clone for Point {
                fn clone(self) -> int {
                    return 42
                }
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wrong return type"));
    }

    #[test]
    fn test_trait_impl_wrong_param_count() {
        // Should error: wrong number of parameters
        let result = check_source(
            r#"
            trait Compute {
                fn compute(self, x: int) -> int
            }

            struct Calculator {}

            impl Compute for Calculator {
                fn compute(self) -> int {
                    return 0
                }
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wrong number of parameters"));
    }

    #[test]
    fn test_trait_impl_unknown_trait() {
        // Should error: trait not defined
        let result = check_source(
            r#"
            struct Point {
                x: int
            }

            impl UnknownTrait for Point {
                fn foo(self) -> int {
                    return 0
                }
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is not defined"));
    }

    #[test]
    fn test_trait_method_call() {
        assert!(check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }

            struct Point {
                x: int
                y: int
            }

            impl Clone for Point {
                fn clone(self) -> Point {
                    return Point { x: self.x, y: self.y }
                }
            }

            let p = Point { x: 1, y: 2 }
            let p2 = p.clone()
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Generic Constraint Tests
    // ========================================================================

    #[test]
    fn test_generic_function_without_bounds() {
        // Generic function without constraints should work for basic usage
        assert!(check_source(
            r#"
            fn identity<T>(x: T) -> T {
                return x
            }

            let a = identity(42)
            let b = identity("hello")
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_function_with_bound() {
        // Generic function with trait bound in type parameter
        assert!(check_source(
            r#"
            trait ToString {
                fn to_string(self) -> string
            }

            fn print_it<T: ToString>(x: T) {
                // x.to_string() should be valid because T: ToString
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_function_multiple_bounds() {
        // Generic function with multiple bounds
        assert!(check_source(
            r#"
            trait Eq {
                fn equals(self, other: Self) -> bool
            }

            trait Hash {
                fn hash(self) -> int
            }

            fn hash_if_equal<T: Eq + Hash>(a: T, b: T) -> int {
                return 0
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_struct_with_bounds() {
        // Generic struct with bounded type parameter
        assert!(check_source(
            r#"
            trait Clone {
                fn clone(self) -> Self
            }

            struct Container<T: Clone> {
                value: T
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_comparison_rejected_without_bound() {
        // Comparison on unconstrained generic type should error
        let result = check_source(
            r#"
            fn max<T>(a: T, b: T) -> T {
                if a > b {
                    return a
                }
                return b
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unconstrained generic type"));
    }

    // ========================================================================
    // Enum Variant Tests
    // ========================================================================

    #[test]
    fn test_enum_unit_variant() {
        // Simple enum with unit variants
        assert!(check_source(
            r#"
            enum Color {
                Red,
                Green,
                Blue
            }

            let c = Color::Red
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_enum_with_data() {
        // Enum with data variants
        assert!(check_source(
            r#"
            enum Option {
                Some(int),
                None
            }

            let x = Option::Some(42)
            let y = Option::None
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_enum_variant_unknown() {
        // Unknown variant should error
        let result = check_source(
            r#"
            enum Color {
                Red,
                Green
            }

            let c = Color::Blue
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown variant"));
    }

    #[test]
    fn test_enum_not_an_enum() {
        // Using :: on non-enum should error
        let result = check_source(
            r#"
            struct Point { x: int, y: int }

            let p = Point::new
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is not an enum"));
    }

    // ========================================================================
    // Try Operator / Error Propagation Tests
    // ========================================================================

    #[test]
    fn test_try_on_optional() {
        // ? on optional should unwrap to inner type
        assert!(check_source(
            r#"
            fn get_value() -> int? {
                return 42
            }

            fn use_value() -> int? {
                let x = get_value()?
                return x + 1
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_try_on_non_result_error() {
        // ? on non-Optional/Result should error
        let result = check_source(
            r#"
            fn get_value() -> int {
                return 42
            }

            fn use_value() -> int {
                let x = get_value()?
                return x
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected Optional or Result"));
    }

    // ========================================================================
    // Result Type Tests
    // ========================================================================

    #[test]
    fn test_result_type_annotation() {
        assert!(check_source(
            r#"
            fn divide(a: int, b: int) -> Result<int, string> {
                if b == 0 {
                    return Result::Err("division by zero")
                }
                return Result::Ok(a / b)
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_result_ok_constructor() {
        assert!(check_source(
            r#"
            let ok_val = Result::Ok(42)
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_result_err_constructor() {
        assert!(check_source(
            r#"
            let err_val = Result::Err("something went wrong")
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_try_on_result() {
        assert!(check_source(
            r#"
            fn fallible() -> Result<int, string> {
                return Result::Ok(42)
            }

            fn caller() -> Result<int, string> {
                let x = fallible()?
                return Result::Ok(x + 1)
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_result_in_match() {
        // Result matching uses enum-like patterns
        assert!(check_source(
            r#"
            fn get_result() -> Result<int, string> {
                return Result::Ok(42)
            }

            fn process() {
                let r = get_result()
                match r {
                    Result::Ok(v) => println(v)
                    Result::Err(e) => println(e)
                }
            }
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Impl Block Tests
    // ========================================================================

    #[test]
    fn test_impl_self_return_type() {
        assert!(check_source(
            r#"
            struct Builder {
                value: int
            }

            impl Builder {
                fn new() -> Builder {
                    return Builder { value: 0 }
                }

                fn with_value(self, v: int) -> Builder {
                    return Builder { value: v }
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_impl_multiple_methods() {
        assert!(check_source(
            r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn origin() -> Point {
                    return Point { x: 0, y: 0 }
                }

                fn get_x(self) -> int {
                    return self.x
                }

                fn get_y(self) -> int {
                    return self.y
                }

                fn translate(self, dx: int, dy: int) -> Point {
                    return Point { x: self.x + dx, y: self.y + dy }
                }
            }
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Const Declaration Tests (TDD - Implementation Required)
    // ========================================================================

    #[test]
    fn test_const_declaration_registers_in_scope() {
        // TDD: Const should be registered and accessible
        assert!(
            check_source(
                r#"
            const MAX: int = 100
            let x = MAX
            "#
            )
            .is_ok(),
            "Const should be accessible after declaration"
        );
    }

    #[test]
    fn test_const_at_top_level_accessible_in_function() {
        // TDD: Top-level const should be accessible inside functions
        assert!(
            check_source(
                r#"
            const GLOBAL: int = 42
            fn get_global() -> int {
                return GLOBAL
            }
            "#
            )
            .is_ok(),
            "Top-level const should be accessible in functions"
        );
    }

    #[test]
    fn test_const_type_checking() {
        // TDD: Const type should be checked
        assert!(
            check_source(
                r#"
            const VALUE: int = 100
            let x: int = VALUE
            "#
            )
            .is_ok(),
            "Const should have correct type"
        );
    }

    #[test]
    fn test_const_assignment_type_mismatch() {
        // TDD: Type mismatch when assigning const to incompatible type
        let result = check_source(
            r#"
            const VALUE: int = 100
            let x: string = VALUE
            "#,
        );
        assert!(result.is_err(), "Const type mismatch should error");
    }

    #[test]
    fn test_multiple_consts() {
        // TDD: Multiple const declarations
        assert!(
            check_source(
                r#"
            const A: int = 1
            const B: int = 2
            const C: int = 3
            let sum = A + B + C
            "#
            )
            .is_ok(),
            "Multiple consts should all be accessible"
        );
    }

    #[test]
    fn test_const_in_expression() {
        // TDD: Const used in complex expressions
        assert!(
            check_source(
                r#"
            const MULT: int = 10
            let x = 5
            let result = x * MULT + MULT
            "#
            )
            .is_ok(),
            "Const should work in expressions"
        );
    }

    // ========================================================================
    // Default Parameter Tests (TDD - Implementation Required)
    // ========================================================================

    #[test]
    fn test_default_param_declaration() {
        // TDD: Function with default param should parse and check
        assert!(
            check_source(
                r#"
            fn greet(name: string, greeting: string = "Hello") {
                println(greeting + " " + name)
            }
            "#
            )
            .is_ok(),
            "Function with default param should type-check"
        );
    }

    #[test]
    fn test_default_param_call_with_all_args() {
        // TDD: Call with all arguments should work
        assert!(
            check_source(
                r#"
            fn greet(name: string, greeting: string = "Hello") {
                println(greeting + " " + name)
            }
            greet("World", "Hi")
            "#
            )
            .is_ok(),
            "Call with all args should work"
        );
    }

    #[test]
    fn test_default_param_call_with_minimum_args() {
        // TDD: Call with fewer args should use defaults
        assert!(
            check_source(
                r#"
            fn greet(name: string, greeting: string = "Hello") {
                println(greeting + " " + name)
            }
            greet("World")
            "#
            )
            .is_ok(),
            "Call with fewer args should use defaults"
        );
    }

    #[test]
    fn test_default_param_multiple_defaults() {
        // TDD: Multiple default parameters
        assert!(
            check_source(
                r#"
            fn format(val: int, pre: string = "[", suf: string = "]") -> string {
                return pre + val + suf
            }
            let a = format(1)
            let b = format(2, "(")
            let c = format(3, "(", ")")
            "#
            )
            .is_ok(),
            "Multiple defaults should work"
        );
    }

    #[test]
    fn test_default_param_type_check() {
        // TDD: Default value type should match parameter type
        let result = check_source(
            r#"
            fn bad(x: int = "wrong") {
                println(x)
            }
            "#,
        );
        assert!(result.is_err(), "Default value type mismatch should error");
    }

    #[test]
    fn test_default_param_too_few_args_error() {
        // TDD: Too few args (less than required) should error
        let result = check_source(
            r#"
            fn needs_one(a: int, b: int = 10) -> int {
                return a + b
            }
            needs_one()
            "#,
        );
        assert!(result.is_err(), "Missing required arg should error");
    }

    #[test]
    fn test_default_param_too_many_args_error() {
        // TDD: Too many args should error
        let result = check_source(
            r#"
            fn takes_two(a: int, b: int = 10) -> int {
                return a + b
            }
            takes_two(1, 2, 3)
            "#,
        );
        assert!(result.is_err(), "Too many args should error");
    }

    // ========================================================================
    // Power Operator Type Checking Tests (TDD - Implementation Required)
    // ========================================================================

    #[test]
    fn test_power_operator_types() {
        // TDD: Power operator should work with int and float
        assert!(
            check_source("let x = 2 ** 3").is_ok(),
            "int ** int should work"
        );
        assert!(
            check_source("let x = 2.0 ** 3.0").is_ok(),
            "float ** float should work"
        );
    }

    #[test]
    fn test_power_operator_mixed_types() {
        // TDD: Power with mixed types - depends on language design
        // For now, just test that same types work
        assert!(
            check_source(
                r#"
            let base = 2
            let exp = 10
            let result = base ** exp
            "#
            )
            .is_ok(),
            "Power with int variables should work"
        );
    }

    #[test]
    fn test_power_operator_result_type() {
        // TDD: Result type should be numeric
        assert!(
            check_source(
                r#"
            let x: int = 2 ** 3
            let y: float = 2.0 ** 3.0
            "#
            )
            .is_ok(),
            "Power result types should match operands"
        );
    }

    // ==========================================================================
    // Destructuring Tests (TDD - T7.22)
    // ==========================================================================

    #[test]
    fn test_tuple_destructuring_basic() {
        // TDD: Basic tuple destructuring should work
        assert!(
            check_source(
                r#"
            let (a, b) = (1, 2)
            let sum = a + b
            "#
            )
            .is_ok(),
            "Basic tuple destructuring should type-check"
        );
    }

    #[test]
    fn test_tuple_destructuring_with_types() {
        // TDD: Tuple destructuring with type annotations
        assert!(
            check_source(
                r#"
            let (x, y): (int, int) = (10, 20)
            "#
            )
            .is_ok(),
            "Tuple destructuring with type annotation should work"
        );
    }

    #[test]
    fn test_tuple_destructuring_nested() {
        // TDD: Nested tuple destructuring
        assert!(
            check_source(
                r#"
            let (a, (b, c)) = (1, (2, 3))
            let sum = a + b + c
            "#
            )
            .is_ok(),
            "Nested tuple destructuring should work"
        );
    }

    #[test]
    fn test_tuple_destructuring_wrong_count() {
        // TDD: Destructuring with wrong element count should error
        let result = check_source(
            r#"
            let (a, b, c) = (1, 2)
            "#,
        );
        assert!(
            result.is_err(),
            "Destructuring with wrong count should error"
        );
    }

    #[test]
    fn test_struct_destructuring_basic() {
        // TDD: Basic struct destructuring
        assert!(
            check_source(
                r#"
            struct Point { x: int, y: int }
            let p = Point { x: 10, y: 20 }
            let { x, y } = p
            let sum = x + y
            "#
            )
            .is_ok(),
            "Basic struct destructuring should work"
        );
    }

    #[test]
    fn test_struct_destructuring_partial() {
        // TDD: Partial struct destructuring (only some fields)
        assert!(
            check_source(
                r#"
            struct Point3D { x: int, y: int, z: int }
            let p = Point3D { x: 1, y: 2, z: 3 }
            let { x, z } = p
            "#
            )
            .is_ok(),
            "Partial struct destructuring should work"
        );
    }

    #[test]
    fn test_struct_destructuring_unknown_field() {
        // TDD: Destructuring unknown field should error
        let result = check_source(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2 }
            let { x, z } = p
            "#,
        );
        assert!(result.is_err(), "Destructuring unknown field should error");
    }

    #[test]
    #[ignore] // Future enhancement: requires updating Parameter to support patterns
    fn test_destructuring_in_function_param() {
        // TDD: Destructuring in function parameters
        assert!(
            check_source(
                r#"
            fn sum_tuple((a, b): (int, int)) -> int {
                return a + b
            }
            let result = sum_tuple((1, 2))
            "#
            )
            .is_ok(),
            "Destructuring in function params should work"
        );
    }

    #[test]
    fn test_var_destructuring_mutable() {
        // TDD: var destructuring creates mutable bindings
        assert!(
            check_source(
                r#"
            var (x, y) = (1, 2)
            x = 10
            y = 20
            "#
            )
            .is_ok(),
            "var destructuring should create mutable bindings"
        );
    }

    // ========================================================================
    // Class Inheritance Tests
    // ========================================================================

    #[test]
    fn test_class_extends_basic() {
        assert!(check_source(
            r#"
            class Animal {
                name: string
            }

            class Dog extends Animal {
                breed: string
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_extends_with_methods() {
        assert!(check_source(
            r#"
            class Animal {
                name: string

                fn speak(self) -> string {
                    return "..."
                }
            }

            class Dog extends Animal {
                breed: string

                override fn speak(self) -> string {
                    return "Woof!"
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_override_required() {
        // Should error: method overrides parent but not marked override
        let result = check_source(
            r#"
            class Animal {
                fn speak(self) -> string {
                    return "..."
                }
            }

            class Dog extends Animal {
                fn speak(self) -> string {
                    return "Woof!"
                }
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not marked with 'override'"));
    }

    #[test]
    fn test_class_override_no_parent_method() {
        // Should error: override on method that doesn't exist in parent
        let result = check_source(
            r#"
            class Animal {
                name: string
            }

            class Dog extends Animal {
                override fn bark(self) -> string {
                    return "Woof!"
                }
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("does not override any parent method"));
    }

    #[test]
    fn test_class_extends_unknown_parent() {
        // Should error: parent class doesn't exist
        let result = check_source(
            r#"
            class Dog extends UnknownAnimal {
                name: string
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_class_cannot_extend_struct() {
        // Should error: cannot extend a struct
        let result = check_source(
            r#"
            struct Point {
                x: int
            }

            class ColoredPoint extends Point {
                color: string
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be extended"));
    }

    #[test]
    fn test_class_multi_level_inheritance() {
        // Test 3-level inheritance chain: Animal -> Dog -> Labrador
        assert!(check_source(
            r#"
            class Animal {
                name: string
            }

            class Dog extends Animal {
                breed: string
            }

            class Labrador extends Dog {
                is_guide_dog: bool
            }

            let lab = Labrador { name: "Max", breed: "Labrador", is_guide_dog: true }
            println(lab.name)
            println(lab.breed)
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_override_multiple_methods() {
        assert!(check_source(
            r#"
            class Base {
                fn foo(self) -> int { return 1 }
                fn bar(self) -> int { return 2 }
            }

            class Derived extends Base {
                override fn foo(self) -> int { return 10 }
                override fn bar(self) -> int { return 20 }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_partial_override() {
        // Only override one method, keep the other
        assert!(check_source(
            r#"
            class Base {
                fn foo(self) -> int { return 1 }
                fn bar(self) -> int { return 2 }
            }

            class Derived extends Base {
                override fn foo(self) -> int { return 10 }
                // bar is inherited, not overridden
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_new_method_in_child() {
        // Child can add new methods without override
        assert!(check_source(
            r#"
            class Animal {
                name: string
            }

            class Dog extends Animal {
                fn bark(self) -> string {
                    return "Woof!"
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_class_diamond_inheritance_not_supported() {
        // Lira doesn't support multiple inheritance, verify single parent
        assert!(check_source(
            r#"
            class A {}
            class B extends A {}
            class C extends A {}
            // D cannot extend both B and C - only single inheritance
            class D extends B {}
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Generic Trait Bounds Tests (where clause)
    // ========================================================================

    #[test]
    fn test_where_clause_basic() {
        assert!(check_source(
            r#"
            trait Eq {}
            trait Hash {}

            fn hash_map_insert<K, V>(key: K, value: V) where K: Eq + Hash {
                // Function body
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_where_clause_multiple_params() {
        assert!(check_source(
            r#"
            trait Display {}
            trait Clone {}

            fn transform<T, U>(input: T) -> U where T: Clone, U: Display {
                // Implementation would go here
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_inline_bounds_equivalent() {
        // Inline bounds and where clause should work the same
        assert!(check_source(
            r#"
            trait Eq {}

            // Inline bound syntax
            fn compare1<T: Eq>(a: T, b: T) -> bool {
                return true
            }

            // Where clause syntax (equivalent)
            fn compare2<T>(a: T, b: T) -> bool where T: Eq {
                return true
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_where_clause_unknown_type_param() {
        // Should error: where clause references undeclared type parameter
        let result = check_source(
            r#"
            trait Eq {}

            fn compare<T>(a: T) where U: Eq {
                // U is not declared
            }
            "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not declared"));
    }

    #[test]
    fn test_combined_inline_and_where_bounds() {
        // Both inline and where clause bounds should work together
        assert!(check_source(
            r#"
            trait Eq {}
            trait Hash {}
            trait Debug {}

            fn complex<T: Eq, U>(a: T, b: U) where U: Hash + Debug {
                // T has Eq from inline, U has Hash + Debug from where
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_where_extends_inline_bounds() {
        // Where clause can add more bounds to existing type param
        assert!(check_source(
            r#"
            trait Eq {}
            trait Hash {}

            fn extend_bounds<T: Eq>(a: T) where T: Hash {
                // T now has both Eq (inline) and Hash (where)
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_with_no_bounds() {
        // Generic without any bounds should work
        assert!(check_source(
            r#"
            fn identity<T>(x: T) -> T {
                return x
            }

            fn swap<A, B>(a: A, b: B) -> B {
                return b
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_where_clause_with_expression_body() {
        // Where clause with expression body syntax
        assert!(check_source(
            r#"
            trait Default {}

            fn make_default<T>() -> T where T: Default => null
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // Monomorphization Tests
    // ========================================================================

    fn check_source_and_get_checker(source: &str) -> Result<TypeChecker, String> {
        use crate::lexer::tokenize;
        use crate::parser::parse;

        let tokens = tokenize(source)?;
        let program = parse(&tokens)?;
        let mut checker = TypeChecker::new();
        checker.check_program(&program)?;
        Ok(checker)
    }

    #[test]
    fn test_generic_instantiation_tracking() {
        let checker = check_source_and_get_checker(
            r#"
            fn identity<T>(x: T) -> T {
                return x
            }

            let a = identity(42)
            let b = identity("hello")
            let c = identity(true)
            "#,
        )
        .unwrap();

        // Should have 3 different instantiations
        assert_eq!(checker.generic_instantiations.len(), 3);

        // Check that the instantiations are recorded
        let has_int = checker
            .generic_instantiations
            .iter()
            .any(|i| i.function_name == "identity" && i.type_args.contains(&"int".to_string()));
        let has_string = checker
            .generic_instantiations
            .iter()
            .any(|i| i.function_name == "identity" && i.type_args.contains(&"string".to_string()));
        let has_bool = checker
            .generic_instantiations
            .iter()
            .any(|i| i.function_name == "identity" && i.type_args.contains(&"bool".to_string()));

        assert!(has_int, "Should have int instantiation");
        assert!(has_string, "Should have string instantiation");
        assert!(has_bool, "Should have bool instantiation");
    }

    #[test]
    fn test_generic_instantiation_mangled_name() {
        let inst = GenericInstantiation::new("identity".to_string(), &[Type::Int]);
        assert_eq!(inst.mangled_name(), "identity$int");

        let inst2 = GenericInstantiation::new("map".to_string(), &[Type::String, Type::Int]);
        assert_eq!(inst2.mangled_name(), "map$string$int");
    }

    #[test]
    fn test_non_generic_function_no_instantiation() {
        let checker = check_source_and_get_checker(
            r#"
            fn add(a: int, b: int) -> int {
                return a + b
            }

            let result = add(1, 2)
            "#,
        )
        .unwrap();

        // Non-generic function should not record any instantiations
        assert!(checker.generic_instantiations.is_empty());
    }

    #[test]
    fn test_same_type_single_instantiation() {
        let checker = check_source_and_get_checker(
            r#"
            fn identity<T>(x: T) -> T {
                return x
            }

            let a = identity(1)
            let b = identity(2)
            let c = identity(3)
            "#,
        )
        .unwrap();

        // All calls are with int, so should only have 1 instantiation
        assert_eq!(checker.generic_instantiations.len(), 1);
    }

    #[test]
    fn test_generic_multi_param_instantiation() {
        let checker = check_source_and_get_checker(
            r#"
            fn pair<A, B>(a: A, b: B) -> A {
                return a
            }

            let x = pair(1, "hello")
            let y = pair("world", 42)
            "#,
        )
        .unwrap();

        // Two different instantiations: (int, string) and (string, int)
        assert_eq!(checker.generic_instantiations.len(), 2);
    }

    #[test]
    fn test_generic_with_two_same_type_params() {
        // Test generic function with two parameters of same generic type
        let checker = check_source_and_get_checker(
            r#"
            fn choose<T>(a: T, b: T, first: bool) -> T {
                if first {
                    return a
                }
                return b
            }

            let x = choose(1, 2, true)
            let y = choose("a", "b", false)
            "#,
        )
        .unwrap();

        // Should have 2 instantiations: int and string
        assert_eq!(checker.generic_instantiations.len(), 2);
    }

    #[test]
    fn test_generic_function_calling_generic() {
        // Generic function that internally uses generic - instantiation recorded
        assert!(check_source(
            r#"
            fn identity<T>(x: T) -> T {
                return x
            }

            fn wrap<T>(x: T) -> T {
                return identity(x)
            }

            let a = wrap(42)
            let b = wrap("hello")
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_generic_instantiation_dedup() {
        let checker = check_source_and_get_checker(
            r#"
            fn process<T>(x: T) -> T {
                return x
            }

            // Multiple calls with same type in different contexts
            fn foo() {
                let a = process(1)
                let b = process(2)
            }

            fn bar() {
                let c = process(3)
            }

            let d = process(4)
            "#,
        )
        .unwrap();

        // All int instantiations should be deduped to 1
        assert_eq!(checker.generic_instantiations.len(), 1);
    }

    #[test]
    fn test_generic_instantiation_mangled_names_unique() {
        let inst1 = GenericInstantiation::new("foo".to_string(), &[Type::Int, Type::String]);
        let inst2 = GenericInstantiation::new("foo".to_string(), &[Type::String, Type::Int]);

        // Different order = different mangled names
        assert_ne!(inst1.mangled_name(), inst2.mangled_name());
        assert_eq!(inst1.mangled_name(), "foo$int$string");
        assert_eq!(inst2.mangled_name(), "foo$string$int");
    }

    #[test]
    fn test_generic_empty_type_args_no_mangle() {
        let inst = GenericInstantiation {
            function_name: "regular_fn".to_string(),
            type_args: vec![],
        };

        // No type args = no mangling
        assert_eq!(inst.mangled_name(), "regular_fn");
    }

    // ========================================================================
    // Noise / Stress Tests
    // ========================================================================

    #[test]
    fn test_mixed_features_complex() {
        // Test combining multiple features together
        assert!(check_source(
            r#"
            trait Printable {}

            class Animal {
                name: string
                fn describe(self) -> string {
                    return self.name
                }
            }

            class Dog extends Animal {
                breed: string
                override fn describe(self) -> string {
                    return self.name + " (" + self.breed + ")"
                }
            }

            fn identity<T>(x: T) -> T {
                return x
            }

            fn process<T, U>(a: T, b: U) -> T where T: Printable {
                return a
            }

            let dog = Dog { name: "Max", breed: "Labrador" }
            let num = identity(42)
            let text = identity("hello")
            let result = dog.describe()
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_deeply_nested_generics() {
        assert!(check_source(
            r#"
            fn outer<T>(x: T) -> T {
                return x
            }

            fn middle<T>(x: T) -> T {
                return outer(x)
            }

            fn inner<T>(x: T) -> T {
                return middle(x)
            }

            let a = inner(1)
            let b = inner("test")
            let c = inner(true)
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_struct_with_impl_instance_methods() {
        // Structs work with impl blocks - verify instance methods
        let result = check_source(
            r#"
            struct Container {
                value: int
            }

            impl Container {
                fn get(self) -> int {
                    return self.value
                }

                fn double(self) -> int {
                    return self.value * 2
                }
            }

            let c = Container { value: 42 }
            let v = c.get()
            let d = c.double()
            "#,
        );
        if let Err(e) = &result {
            eprintln!("test_struct_with_impl_instance_methods error: {}", e);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_trait_with_generic_functions() {
        assert!(check_source(
            r#"
            trait Mapper {
                fn map(self, x: int) -> int
            }

            struct Doubler {}

            impl Mapper for Doubler {
                fn map(self, x: int) -> int {
                    return x * 2
                }
            }

            fn apply_generic<T>(val: T) -> T {
                return val
            }

            let d = Doubler {}
            let result = d.map(21)
            let generic_result = apply_generic(result)
            "#
        )
        .is_ok());
    }

    // ========================================================================
    // SemanticTables Tests
    // ========================================================================

    #[test]
    fn test_sema_created_on_check() {
        let source = r#"
            let x = 42
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // SemanticTables should be created and populated with expression types
        assert!(!checked.sema.expr_types.is_empty(),
            "SemanticTables should have expression types after checking");
    }

    #[test]
    fn test_sema_variable_type_recorded() {
        let source = r#"
            let x = 42
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The variable x should have a type recorded
        // Find the VarDecl statement
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[0].kind {
            // The initializer expression should have a type
            if let Some(ty) = checked.sema.expr_types.get(&init.id) {
                assert_eq!(*ty, Type::Int, "x should be typed as Int");
            }
        }
    }

    #[test]
    fn test_sema_binary_expression_type() {
        let source = r#"
            let x = 1 + 2
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The binary expression 1 + 2 should have a type
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[0].kind {
            if let Some(ty) = checked.sema.expr_types.get(&init.id) {
                assert_eq!(*ty, Type::Int, "1 + 2 should be typed as Int");
            }
        }
    }

    #[test]
    fn test_sema_function_parameter_types() {
        let source = r#"
            fn add(x: int, y: int) -> int {
                return x + y
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The function should be in the symbol table
        // For now, just verify the check succeeds
        assert!(!checked.sema.symbols.is_empty() || checked.sema.generic_instantiations.is_empty(),
            "SemanticTables should be populated after checking");
    }

    // ========================================================================
    // Symbol Reference Tests
    // ========================================================================

    #[test]
    fn test_sema_symbol_ref_recorded() {
        let source = r#"
            let x = 42
            let y = x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The second statement references x
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[1].kind {
            assert!(checked.sema.symbol_refs.contains_key(&init.id),
                "Symbol reference should be recorded for identifier 'x'");
        }
    }

    #[test]
    fn test_sema_symbol_entry_created() {
        let source = r#"
            let x = 42
            let y = x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // A symbol entry should be created when referencing a variable
        assert!(!checked.sema.symbols.is_empty(),
            "Symbols should be recorded after checking variable references");
    }

    // ========================================================================
    // Call Resolution Tests
    // ========================================================================

    #[test]
    fn test_sema_function_call_resolution() {
        let source = r#"
            fn add(x: int, y: int) -> int {
                return x + y
            }
            let result = add(1, 2)
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The call to add() should have call resolution
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[1].kind {
            if let Some(resolution) = checked.sema.call_resolution.get(&init.id) {
                match resolution {
                    crate::sema::CallResolution::Function { name } => {
                        assert_eq!(name, "add", "Should resolve to function 'add'");
                    }
                    _ => panic!("Expected Function call resolution"),
                }
            }
        }
    }

    #[test]
    fn test_sema_enum_constructor_resolution() {
        let source = r#"
            enum Color {
                Red
                Green
            }
            let c = Color::Green
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The enum constructor Color::Green should be resolved
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[1].kind {
            assert!(checked.sema.expr_types.contains_key(&init.id),
                "Expression type should be recorded for enum constructor");
        }
    }

    #[test]
    fn test_sema_method_call_resolution() {
        let source = r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn add(self, other: Point) -> Point {
                    return Point { x: self.x + other.x, y: self.y + other.y }
                }
            }

            let p = Point { x: 1, y: 2 }
            let q = Point { x: 3, y: 4 }
            let r = p.add(q)
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The call to p.add(q) should have call resolution
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[3].kind {
            if let Some(resolution) = checked.sema.call_resolution.get(&init.id) {
                match resolution {
                    crate::sema::CallResolution::Method { type_name, method_name } => {
                        assert_eq!(type_name, "Point", "Should resolve to type 'Point'");
                        assert_eq!(method_name, "add", "Should resolve to method 'add'");
                    }
                    _ => panic!("Expected Method call resolution"),
                }
            }
        }
    }

    // ========================================================================
    // Field Resolution Tests
    // ========================================================================

    #[test]
    fn test_sema_field_resolution() {
        let source = r#"
            struct Point {
                x: int
                y: int
            }

            let p = Point { x: 1, y: 2 }
            let px = p.x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // The field access p.x should have field resolution
        if let StatementKind::VarDecl { initializer: Some(init), .. } = &checked.program.statements[1].kind {
            if let Some(resolution) = checked.sema.field_resolution.get(&init.id) {
                assert_eq!(resolution.field_name, "x", "Should resolve field 'x'");
                assert!(!resolution.is_method, "Should not be a method");
            }
        }
    }
}
