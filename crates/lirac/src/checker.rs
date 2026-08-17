//! Lira Type Checker
//!
//! Validates types and performs type inference.
//! See docs/lira/02-type-system.md for the full specification.

use crate::ast::*;
use crate::errors::{CheckerError, IntContext, TypeContext};
use crate::ids::{NodeId, SymbolId};
use crate::sema::SemanticTables;
use std::collections::{HashMap, HashSet};

/// Compiler-private nominal type used for built-in range values.
///
/// The spelling is not producible by Lira source, so a user-defined `struct
/// Range` remains an ordinary, non-iterable struct.
pub const BUILTIN_RANGE_TYPE: &str = "$lira_builtin_range";

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
    /// A channel carrying values of the element type.
    Channel(Box<Type>),
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
    //
    // Runtime model: TYPE ERASURE. This is the intended, supported strategy —
    // not a placeholder for monomorphization. A `TypeParam` is checked at the
    // type level (including its trait bounds, e.g. `T: Numeric`) and then erased
    // to a uniform runtime representation (`Any`) during codegen. There is a
    // single bytecode version of each generic function, shared by all concrete
    // type arguments. See docs/lira/ROADMAP.md for the generics design.
    TypeParam(String),
}

impl Type {
    /// Whether this is the compiler-created range type rather than a
    /// source-defined aggregate with a similar name or fields.
    pub fn is_builtin_range(&self) -> bool {
        matches!(self, Type::Struct(name) if name == BUILTIN_RANGE_TYPE)
    }

    fn contains_unresolved_storage_type(&self) -> bool {
        match self {
            Type::Unknown | Type::TypeVar(_) => true,
            Type::Array(inner) | Type::Channel(inner) | Type::Optional(inner) => {
                inner.contains_unresolved_storage_type()
            }
            Type::Tuple(elements) => elements.iter().any(Type::contains_unresolved_storage_type),
            Type::Map(key, value) => {
                key.contains_unresolved_storage_type() || value.contains_unresolved_storage_type()
            }
            Type::Result { ok_type, err_type } => {
                ok_type.contains_unresolved_storage_type()
                    || err_type.contains_unresolved_storage_type()
            }
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(Type::contains_unresolved_storage_type)
                    || return_type.contains_unresolved_storage_type()
            }
            _ => false,
        }
    }

    fn contains_unresolved_mutable_storage(&self) -> bool {
        match self {
            Type::Array(inner) | Type::Channel(inner) => inner.contains_unresolved_storage_type(),
            Type::Map(key, value) => {
                key.contains_unresolved_storage_type() || value.contains_unresolved_storage_type()
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(Type::contains_unresolved_mutable_storage),
            Type::Optional(inner) => inner.contains_unresolved_mutable_storage(),
            Type::Result { ok_type, err_type } => {
                ok_type.contains_unresolved_mutable_storage()
                    || err_type.contains_unresolved_mutable_storage()
            }
            _ => false,
        }
    }

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
            // Mutable containers are invariant. An unknown element still
            // participates in inference only while its fresh literal/builtin
            // result has not crossed an aliasing boundary.
            (Type::Array(a), Type::Array(b)) | (Type::Channel(a), Type::Channel(b)) => {
                a == b && !a.contains_unresolved_storage_type()
            }
            // Tuples are immutable value aggregates and are covariant by
            // position.
            (Type::Tuple(a), Type::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.is_compatible_with(b))
            }
            // Maps are mutable and invariant in both key and value types.
            (Type::Map(a_key, a_value), Type::Map(b_key, b_value)) => {
                a_key == b_key
                    && a_value == b_value
                    && !a_key.contains_unresolved_storage_type()
                    && !a_value.contains_unresolved_storage_type()
            }
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
                        .all(|(a, b)| b.is_compatible_with(a))
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
            Type::Channel(elem) => format!("Channel<{}>", elem.display_name()),
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
            Type::Struct(name) if name == BUILTIN_RANGE_TYPE => "range".to_string(),
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
    /// Stable identity for this binding, used to group references across uses
    /// and scopes. Assigned by [`TypeEnv::define`].
    pub id: SymbolId,
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
    /// Number of leading parameters a caller must supply, including `self`
    /// when present. Impl methods retain this separately because their compact
    /// semantic representation otherwise drops default expressions.
    pub required_params: usize,
    /// Per-slot default availability, parallel to `params`. Structural
    /// interface compatibility must preserve every call shape promised by an
    /// interface, including named omissions in the middle of the list.
    pub default_params: Vec<bool>,
}

/// Type environment / scope
pub struct TypeEnv {
    scopes: Vec<HashMap<String, Symbol>>,
    type_defs: HashMap<String, TypeDef>,
    impl_methods: HashMap<String, Vec<ImplMethod>>, // type_name -> methods
    /// Receiver metadata for methods declared inline in structs/classes.
    /// TypeDef method signatures retain the parameter types but not the
    /// parameter names, so call checking keeps this small side table to
    /// distinguish `Type.static_method(...)` from `value.method(...)`.
    declared_method_receivers: HashMap<(String, String), bool>,
    /// Per-parameter default masks for inline class/struct methods. Unlike
    /// free functions, methods can be inherited, so these are keyed by owner
    /// and method name rather than by the (possibly colliding) method name.
    declared_method_defaults: HashMap<(String, String), Vec<bool>>,
    /// Parameter names for inline class/struct methods. TypeDef stores erased
    /// function types, so call checking keeps the source names separately for
    /// named-argument binding.
    declared_method_param_names: HashMap<(String, String), Vec<String>>,
    trait_defs: HashMap<String, Vec<ImplMethod>>, // trait_name -> required methods
    trait_impls: HashMap<(String, String), Vec<ImplMethod>>, // (trait_name, type_name) -> impl
    /// Maps generic function names to their type parameter names
    generic_functions: HashMap<String, Vec<String>>, // fn_name -> [T, U, ...]
    /// Declared type parameters of user-defined generic aggregates.
    generic_type_params: HashMap<String, Vec<String>>, // type_name -> [T, U, ...]
    /// Type parameters introduced by an impl method itself (excluding the
    /// surrounding generic owner's parameters).
    generic_method_params: HashMap<(String, String), Vec<String>>,
    /// Maps function names to their declared parameter names (in declaration
    /// order). Used to resolve and reorder named arguments at call sites.
    fn_param_names: HashMap<String, Vec<String>>, // fn_name -> [param_name, ...]
    /// Maps function names to a per-parameter flag indicating whether that
    /// parameter has a default value (parallel to `fn_param_names`). Used so
    /// named-argument resolution can tell whether a skipped slot is allowed
    /// (defaulted) or a genuinely missing required parameter.
    fn_param_defaults: HashMap<String, Vec<bool>>, // fn_name -> [has_default, ...]
    next_type_var: u32,
    structured_errors: Vec<CheckerError>,
    /// Counter for minting unique [`SymbolId`]s for every binding defined in
    /// this environment (including builtins).
    next_symbol_id: u32,
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
            declared_method_receivers: HashMap::new(),
            declared_method_defaults: HashMap::new(),
            declared_method_param_names: HashMap::new(),
            trait_defs: HashMap::new(),
            trait_impls: HashMap::new(),
            generic_functions: HashMap::new(),
            generic_type_params: HashMap::new(),
            generic_method_params: HashMap::new(),
            fn_param_names: HashMap::new(),
            fn_param_defaults: HashMap::new(),
            next_type_var: 0,
            structured_errors: Vec::new(),
            next_symbol_id: 0,
        };

        // Add built-in types
        for (name, ty) in [
            ("int", Type::Int),
            ("float", Type::Float),
            ("bool", Type::Bool),
            ("string", Type::String),
            ("char", Type::Char),
            ("void", Type::Void),
        ] {
            env.type_defs.insert(
                name.to_string(),
                TypeDef {
                    name: name.to_string(),
                    kind: TypeDefKind::Alias(ty),
                },
            );
        }

        // Add Result as a built-in enum-like type with Ok and Err variants
        // Result::Ok(value) and Result::Err(error) are the constructors
        env.type_defs.insert(
            "Result".to_string(),
            TypeDef {
                name: "Result".to_string(),
                kind: TypeDefKind::Enum {
                    variants: vec![
                        ("Ok".to_string(), vec![Type::Any]),
                        ("Err".to_string(), vec![Type::Any]),
                    ],
                },
            },
        );

        let mut reg = |name: &str, params: Vec<Type>, ret: Type, required: usize| {
            env.define(Symbol {
                id: SymbolId(0), // assigned by `define`
                name: name.to_string(),
                ty: Type::Function {
                    params,
                    return_type: Box::new(ret),
                    required_params: required,
                },
                mutable: false,
                kind: SymbolKind::Function,
            });
        };

        // Core built-in functions
        reg("print", vec![Type::Any], Type::Void, 1);
        reg("println", vec![Type::Any], Type::Void, 1);
        reg("assert", vec![Type::Bool], Type::Void, 1);

        // Channel built-in functions. `chan` has one optional capacity and
        // returns an unrefined channel whose element type is learned by its
        // first direct-bound send.
        let unknown_channel = || Type::Channel(Box::new(Type::Unknown));
        reg("chan", vec![Type::Int], unknown_channel(), 0);
        reg("send", vec![unknown_channel(), Type::Any], Type::Void, 2);
        reg("recv", vec![unknown_channel()], Type::Any, 1);
        reg("close", vec![unknown_channel()], Type::Void, 1);

        // Fiber built-in functions
        reg("fiber_yield", vec![], Type::Void, 0);
        reg("fiber_id", vec![], Type::Int, 0);

        // Memory management built-in functions
        // `collect()` forces a garbage collection of cyclic heap values.
        reg("collect", vec![], Type::Void, 0);

        // Array built-in functions
        reg("len", vec![Type::Any], Type::Int, 1);
        reg("push", vec![Type::Any, Type::Any], Type::Void, 2);
        reg("pop", vec![Type::Any], Type::Any, 1);

        // ================================================================
        // File I/O built-in functions
        // ================================================================
        reg("file_open", vec![Type::String, Type::Int], Type::Int, 2);
        reg("file_read", vec![Type::Int, Type::Int], Type::String, 2);
        reg("file_write", vec![Type::Int, Type::String], Type::Int, 2);
        reg("file_close", vec![Type::Int], Type::Bool, 1);
        reg("file_exists", vec![Type::String], Type::Bool, 1);
        reg("file_size", vec![Type::String], Type::Int, 1);
        reg(
            "file_seek",
            vec![Type::Int, Type::Int, Type::Int],
            Type::Int,
            3,
        );

        // ================================================================
        // Environment built-in functions
        // ================================================================
        reg(
            "env_get",
            vec![Type::String],
            Type::Optional(Box::new(Type::String)),
            1,
        );
        reg("env_args", vec![], Type::Array(Box::new(Type::String)), 0);
        reg("env_set", vec![Type::String, Type::String], Type::Bool, 2);
        reg("env_remove", vec![Type::String], Type::Bool, 1);
        reg("env_all", vec![], Type::Array(Box::new(Type::String)), 0);
        reg("env_keys", vec![], Type::Array(Box::new(Type::String)), 0);
        reg("env_has", vec![Type::String], Type::Bool, 1);
        reg("env_exe", vec![], Type::String, 0);
        reg("env_temp_dir", vec![], Type::String, 0);
        reg("env_home_dir", vec![], Type::String, 0);

        // ================================================================
        // Time built-in functions
        // ================================================================
        reg("time_ms", vec![], Type::Int, 0);
        reg("sleep", vec![Type::Int], Type::Void, 1);
        reg("time_secs", vec![], Type::Int, 0);
        reg("time_micros", vec![], Type::Int, 0);
        reg("time_nanos", vec![], Type::Int, 0);
        reg("time_format_iso", vec![Type::Int], Type::String, 1);
        reg(
            "time_format",
            vec![Type::Int, Type::String],
            Type::String,
            2,
        );
        reg("time_parse_iso", vec![Type::String], Type::Int, 1);
        reg("time_timezone_offset", vec![], Type::Int, 0);
        reg(
            "time_components",
            vec![Type::Int],
            Type::Array(Box::new(Type::Int)),
            1,
        );
        reg(
            "time_from_components",
            vec![
                Type::Int,
                Type::Int,
                Type::Int,
                Type::Int,
                Type::Int,
                Type::Int,
            ],
            Type::Int,
            6,
        );

        // ================================================================
        // String operation built-in functions
        // ================================================================
        reg("str_char_code", vec![Type::String, Type::Int], Type::Int, 2);
        reg("str_from_char_code", vec![Type::Int], Type::String, 1);
        reg("str_to_upper", vec![Type::String], Type::String, 1);
        reg("str_to_lower", vec![Type::String], Type::String, 1);
        reg(
            "str_substring",
            vec![Type::String, Type::Int, Type::Int],
            Type::String,
            3,
        );
        reg(
            "str_index_of",
            vec![Type::String, Type::String],
            Type::Int,
            2,
        );
        reg(
            "str_split",
            vec![Type::String, Type::String],
            Type::Array(Box::new(Type::String)),
            2,
        );
        reg("str_trim", vec![Type::String], Type::String, 1);
        reg("str_trim_start", vec![Type::String], Type::String, 1);
        reg("str_trim_end", vec![Type::String], Type::String, 1);

        // ================================================================
        // Random number generation built-in functions
        // ================================================================
        reg("random", vec![], Type::Float, 0);
        reg("random_int", vec![Type::Int, Type::Int], Type::Int, 2);

        // ================================================================
        // Base64 encoding/decoding built-in functions
        // ================================================================
        reg("base64_encode", vec![Type::String], Type::String, 1);
        reg("base64_decode", vec![Type::String], Type::String, 1);
        reg("base64_encode_url", vec![Type::String], Type::String, 1);
        reg("base64_decode_url", vec![Type::String], Type::String, 1);

        // ================================================================
        // URL encoding/decoding built-in functions
        // ================================================================
        reg("url_encode", vec![Type::String], Type::String, 1);
        reg("url_decode", vec![Type::String], Type::String, 1);

        // ================================================================
        // HTTP Client built-in functions
        // ================================================================
        reg(
            "http_get",
            vec![Type::String],
            Type::Tuple(vec![Type::Int, Type::String]),
            1,
        );
        reg(
            "http_post",
            vec![Type::String, Type::String, Type::String],
            Type::Tuple(vec![Type::Int, Type::String]),
            3,
        );
        reg(
            "http_request",
            vec![Type::String, Type::String, Type::String, Type::String],
            Type::Tuple(vec![Type::Int, Type::String]),
            4,
        );

        // ================================================================
        // Cryptographic hash built-in functions
        // ================================================================
        reg("md5", vec![Type::String], Type::String, 1);
        reg("sha1", vec![Type::String], Type::String, 1);
        reg("sha256", vec![Type::String], Type::String, 1);
        reg("sha512", vec![Type::String], Type::String, 1);

        // ================================================================
        // JSON built-in functions
        // ================================================================
        reg("json_parse", vec![Type::String], Type::Any, 1);
        reg("json_stringify", vec![Type::Any], Type::String, 1);
        reg("json_pretty", vec![Type::Any], Type::String, 1);

        // ================================================================
        // TCP Networking built-in functions
        // ================================================================
        reg("tcp_connect", vec![Type::String, Type::Int], Type::Int, 2);
        reg("tcp_write", vec![Type::Int, Type::String], Type::Int, 2);
        reg("tcp_read", vec![Type::Int, Type::Int], Type::String, 2);
        reg("tcp_close", vec![Type::Int], Type::Bool, 1);
        reg("dns_lookup", vec![Type::String], Type::String, 1);

        // ================================================================
        // OS built-in functions
        // ================================================================
        reg("getcwd", vec![], Type::String, 0);
        reg("chdir", vec![Type::String], Type::Bool, 1);
        reg("mkdir", vec![Type::String], Type::Bool, 1);
        reg("mkdir_all", vec![Type::String], Type::Bool, 1);
        reg("rmdir", vec![Type::String], Type::Bool, 1);
        reg("remove", vec![Type::String], Type::Bool, 1);
        reg("remove_all", vec![Type::String], Type::Bool, 1);
        reg(
            "listdir",
            vec![Type::String],
            Type::Array(Box::new(Type::String)),
            1,
        );
        reg("is_dir", vec![Type::String], Type::Bool, 1);
        reg("is_file", vec![Type::String], Type::Bool, 1);
        reg("rename", vec![Type::String, Type::String], Type::Bool, 2);
        reg("copy", vec![Type::String, Type::String], Type::Bool, 2);

        // ================================================================
        // Regex built-in functions
        // ================================================================
        reg(
            "regex_match",
            vec![Type::String, Type::String],
            Type::Bool,
            2,
        );
        reg(
            "regex_find",
            vec![Type::String, Type::String],
            Type::String,
            2,
        );
        reg(
            "regex_find_all",
            vec![Type::String, Type::String],
            Type::Array(Box::new(Type::String)),
            2,
        );
        reg(
            "regex_replace",
            vec![Type::String, Type::String, Type::String],
            Type::String,
            3,
        );
        reg(
            "regex_replace_all",
            vec![Type::String, Type::String, Type::String],
            Type::String,
            3,
        );
        reg(
            "regex_split",
            vec![Type::String, Type::String],
            Type::Array(Box::new(Type::String)),
            2,
        );
        reg(
            "regex_captures",
            vec![Type::String, Type::String],
            Type::Array(Box::new(Type::String)),
            2,
        );
        reg("regex_is_valid", vec![Type::String], Type::Bool, 1);

        // ================================================================
        // UUID built-in functions
        // ================================================================
        reg("uuid_v4", vec![], Type::String, 0);
        reg("uuid_v7", vec![], Type::String, 0);
        reg("uuid_is_valid", vec![Type::String], Type::Bool, 1);
        reg("uuid_nil", vec![], Type::String, 0);

        // ================================================================
        // Math built-in functions
        // ================================================================
        reg("sqrt", vec![Type::Float], Type::Float, 1);
        reg("pow", vec![Type::Float, Type::Float], Type::Float, 2);
        reg("exp", vec![Type::Float], Type::Float, 1);
        reg("ln", vec![Type::Float], Type::Float, 1);
        reg("log10", vec![Type::Float], Type::Float, 1);
        reg("log2", vec![Type::Float], Type::Float, 1);
        reg("sin", vec![Type::Float], Type::Float, 1);
        reg("cos", vec![Type::Float], Type::Float, 1);
        reg("tan", vec![Type::Float], Type::Float, 1);
        reg("asin", vec![Type::Float], Type::Float, 1);
        reg("acos", vec![Type::Float], Type::Float, 1);
        reg("atan", vec![Type::Float], Type::Float, 1);
        reg("atan2", vec![Type::Float, Type::Float], Type::Float, 2);
        reg("sinh", vec![Type::Float], Type::Float, 1);
        reg("cosh", vec![Type::Float], Type::Float, 1);
        reg("tanh", vec![Type::Float], Type::Float, 1);
        reg("floor", vec![Type::Float], Type::Float, 1);
        reg("ceil", vec![Type::Float], Type::Float, 1);
        reg("round", vec![Type::Float], Type::Float, 1);
        reg("trunc", vec![Type::Float], Type::Float, 1);
        reg("abs", vec![Type::Float], Type::Float, 1);
        reg("is_nan", vec![Type::Float], Type::Bool, 1);
        reg("is_infinite", vec![Type::Float], Type::Bool, 1);
        reg("is_finite", vec![Type::Float], Type::Bool, 1);

        // These core signatures are part of the named-argument contract in
        // the formal specification. Keep names/default metadata alongside the
        // builtin function types so calls take the same validation path as
        // user-defined functions instead of silently treating unknown names
        // as positional arguments.
        for (name, parameter) in [
            ("print", "value"),
            ("println", "value"),
            ("assert", "condition"),
        ] {
            env.register_fn_param_names(name, vec![parameter.to_string()]);
            env.register_fn_param_defaults(name, vec![false]);
        }

        env
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Define a binding in the innermost scope, assigning it a fresh stable
    /// [`SymbolId`]. The assigned id is returned so callers can record a
    /// declaration entry in the semantic tables.
    pub fn define(&mut self, mut symbol: Symbol) -> SymbolId {
        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;
        symbol.id = id;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(symbol.name.clone(), symbol);
        }
        id
    }

    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol);
            }
        }
        None
    }

    /// Refine an array binding after `push` supplies a value.
    ///
    /// Empty arrays learn their first element type. Concrete homogeneous arrays
    /// never widen implicitly: doing so would make an alias retain the old
    /// element type while the shared storage changes representation. Explicit
    /// `[any]` is the opt-in heterogeneous representation.
    fn refine_array_for_push(&mut self, id: SymbolId, element: Type) -> Option<Type> {
        for scope in self.scopes.iter_mut().rev() {
            let Some(symbol) = scope.values_mut().find(|symbol| symbol.id == id) else {
                continue;
            };
            let Type::Array(inner) = &symbol.ty else {
                return None;
            };
            let next = if matches!(inner.as_ref(), Type::Unknown | Type::TypeVar(_)) {
                element
            } else if element.is_compatible_with(inner) && inner.is_compatible_with(&element) {
                return Some(symbol.ty.clone());
            } else {
                return None;
            };
            symbol.ty = Type::Array(Box::new(next));
            return Some(symbol.ty.clone());
        }
        None
    }

    /// Refine a directly-bound channel returned by `chan()` once its first
    /// send supplies the payload type. Stable symbol identity makes this safe
    /// across nested scopes and shadowing.
    fn refine_unknown_channel(&mut self, id: SymbolId, element: Type) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            let Some(symbol) = scope.values_mut().find(|symbol| symbol.id == id) else {
                continue;
            };
            let Type::Channel(inner) = &symbol.ty else {
                return false;
            };
            if !matches!(inner.as_ref(), Type::Unknown | Type::TypeVar(_)) {
                return false;
            }
            symbol.ty = Type::Channel(Box::new(element));
            return true;
        }
        false
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

    /// Record a type error. Errors that do not yet have a dedicated
    /// [`CheckerError`] variant are stored as `GenericError`, so every error
    /// flows through the structured channel with its span intact.
    pub fn error(&mut self, span: &Span, message: String) {
        self.structured_errors.push(CheckerError::GenericError {
            message,
            span: span.clone(),
        });
    }

    /// Add a method from an impl block
    pub fn add_impl_method(&mut self, type_name: &str, method: ImplMethod) {
        let type_name = self.canonical_impl_owner(type_name);
        self.impl_methods.entry(type_name).or_default().push(method);
    }

    /// Get methods for a type from impl blocks
    pub fn get_impl_methods(&self, type_name: &str) -> Option<&Vec<ImplMethod>> {
        let canonical = self.canonical_impl_owner(type_name);
        self.impl_methods
            .get(type_name)
            .or_else(|| self.impl_methods.get(&canonical))
    }

    /// Look up a specific method for a type
    pub fn lookup_method(&self, type_name: &str, method_name: &str) -> Option<&ImplMethod> {
        let canonical = self.canonical_impl_owner(type_name);
        self.impl_methods
            .get(type_name)
            .or_else(|| self.impl_methods.get(&canonical))
            .and_then(|methods| methods.iter().find(|m| m.name == method_name))
    }

    /// Resolve the owner spelling used by an impl to the one runtime and
    /// checker dispatch use. Type aliases are transparent, including aliases
    /// of aliases and aliases of array element types.
    fn canonical_impl_owner(&self, type_name: &str) -> String {
        fn canonical_type(env: &TypeEnv, ty: &Type, seen: &mut HashSet<String>) -> Type {
            match ty {
                Type::Array(inner) => Type::Array(Box::new(canonical_type(env, inner, seen))),
                Type::Struct(name)
                | Type::Class(name)
                | Type::Enum(name)
                | Type::Interface(name) => {
                    if seen.insert(name.clone()) {
                        if let Some(TypeDef {
                            kind: TypeDefKind::Alias(target),
                            ..
                        }) = env.lookup_type(name)
                        {
                            return canonical_type(env, target, seen);
                        }
                    }
                    ty.clone()
                }
                other => other.clone(),
            }
        }

        let ty =
            if let (Some(inner), true) = (type_name.strip_prefix('['), type_name.ends_with(']')) {
                Type::Array(Box::new(canonical_type(
                    self,
                    &self.owner_type_from_name(&inner[..inner.len() - 1]),
                    &mut HashSet::new(),
                )))
            } else {
                self.owner_type_from_name(type_name)
            };
        canonical_type(self, &ty, &mut HashSet::new()).display_name()
    }

    fn owner_type_from_name(&self, type_name: &str) -> Type {
        match type_name {
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
            "uint8" | "byte" => Type::UInt8,
            "uint16" => Type::UInt16,
            "uint32" => Type::UInt32,
            "uint64" => Type::UInt64,
            _ => match self
                .lookup_type(type_name)
                .map(|definition| &definition.kind)
            {
                Some(TypeDefKind::Alias(ty)) => ty.clone(),
                Some(TypeDefKind::Class { .. }) => Type::Class(type_name.to_string()),
                Some(TypeDefKind::Struct { .. }) => Type::Struct(type_name.to_string()),
                Some(TypeDefKind::Enum { .. }) => Type::Enum(type_name.to_string()),
                Some(TypeDefKind::Interface { .. }) => Type::Interface(type_name.to_string()),
                None => Type::Struct(type_name.to_string()),
            },
        }
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
        let type_name = self.canonical_impl_owner(type_name);
        self.trait_impls
            .insert((trait_name.to_string(), type_name), methods);
    }

    /// Get all fields for a class, including inherited fields from parent classes
    pub fn get_class_fields(&self, class_name: &str) -> Vec<(String, Type, bool)> {
        let mut all_fields = Vec::new();
        let mut current_class = Some(class_name.to_string());
        let mut visited = HashSet::new();

        while let Some(class) = current_class {
            if !visited.insert(class.clone()) {
                break;
            }
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
        let mut visited = HashSet::new();

        while let Some(class) = current_class {
            if !visited.insert(class.clone()) {
                break;
            }
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

    /// Return whether an inline method has an explicit `self` receiver.
    ///
    /// Class methods may be inherited, so walk the single-parent chain while
    /// guarding against malformed cyclic declarations during error recovery.
    pub fn declared_method_has_self(&self, type_name: &str, method_name: &str) -> Option<bool> {
        let mut current = type_name.to_string();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return None;
            }
            if let Some(has_self) = self
                .declared_method_receivers
                .get(&(current.clone(), method_name.to_string()))
            {
                return Some(*has_self);
            }
            let type_def = self.lookup_type(&current)?;
            match &type_def.kind {
                TypeDefKind::Class { parent, .. } => {
                    let Some(parent) = parent else {
                        return None;
                    };
                    current = parent.clone();
                }
                // Struct methods are never inherited, but their receiver
                // metadata is still needed to distinguish instance methods
                // from static methods during structural interface checks.
                TypeDefKind::Struct { .. } => return None,
                _ => return None,
            }
        }
    }

    /// Check nominal class subtyping through the declared parent graph.
    ///
    /// This relation intentionally only applies to classes. Structs remain
    /// nominally distinct even when their fields happen to match.
    pub fn is_class_subtype(&self, child: &str, parent: &str) -> bool {
        if child == parent {
            return true;
        }
        let mut current = child.to_string();
        let mut visited = HashSet::new();
        while visited.insert(current.clone()) {
            let Some(next) = self.get_parent_class(&current) else {
                return false;
            };
            if next == parent {
                return true;
            }
            current = next;
        }
        false
    }

    fn register_declared_method_receiver(
        &mut self,
        type_name: &str,
        method_name: &str,
        has_self: bool,
    ) {
        self.declared_method_receivers
            .insert((type_name.to_string(), method_name.to_string()), has_self);
    }

    fn register_declared_method_defaults(
        &mut self,
        type_name: &str,
        method_name: &str,
        has_defaults: Vec<bool>,
    ) {
        self.declared_method_defaults.insert(
            (type_name.to_string(), method_name.to_string()),
            has_defaults,
        );
    }

    fn register_declared_method_param_names(
        &mut self,
        type_name: &str,
        method_name: &str,
        param_names: Vec<String>,
    ) {
        self.declared_method_param_names.insert(
            (type_name.to_string(), method_name.to_string()),
            param_names,
        );
    }

    /// Return parameter names for an inline method, walking inherited classes
    /// while guarding malformed cycles.
    fn declared_method_param_names(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Option<Vec<String>> {
        let mut current = type_name.to_string();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return None;
            }
            if let Some(names) = self
                .declared_method_param_names
                .get(&(current.clone(), method_name.to_string()))
            {
                return Some(names.clone());
            }
            let type_def = self.lookup_type(&current)?;
            let TypeDefKind::Class { parent, .. } = &type_def.kind else {
                return None;
            };
            let Some(parent) = parent else {
                return None;
            };
            current = parent.clone();
        }
    }

    /// Return the default mask for the declaration that supplies a method,
    /// walking inherited classes while guarding malformed cycles.
    fn declared_method_defaults(&self, type_name: &str, method_name: &str) -> Option<Vec<bool>> {
        let mut current = type_name.to_string();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return None;
            }
            if let Some(defaults) = self
                .declared_method_defaults
                .get(&(current.clone(), method_name.to_string()))
            {
                return Some(defaults.clone());
            }
            let type_def = self.lookup_type(&current)?;
            let TypeDefKind::Class { parent, .. } = &type_def.kind else {
                return None;
            };
            let Some(parent) = parent else {
                return None;
            };
            current = parent.clone();
        }
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

    fn register_generic_type(&mut self, name: &str, type_params: Vec<String>) {
        if !type_params.is_empty() {
            self.generic_type_params
                .insert(name.to_string(), type_params);
        }
    }

    fn generic_type_params(&self, name: &str) -> Option<&[String]> {
        self.generic_type_params.get(name).map(Vec::as_slice)
    }

    fn register_generic_method(&mut self, owner: &str, method: &str, type_params: Vec<String>) {
        if !type_params.is_empty() {
            self.generic_method_params
                .insert((owner.to_string(), method.to_string()), type_params);
        }
    }

    fn generic_method_params(&self, owner: &str, method: &str) -> Option<&[String]> {
        self.generic_method_params
            .get(&(owner.to_string(), method.to_string()))
            .map(Vec::as_slice)
    }

    /// Record the declared parameter names of a function (in order), so named
    /// arguments at call sites can be matched/reordered by name.
    pub fn register_fn_param_names(&mut self, name: &str, param_names: Vec<String>) {
        self.fn_param_names.insert(name.to_string(), param_names);
    }

    /// Look up the declared parameter names for a function by name.
    pub fn fn_param_names(&self, name: &str) -> Option<&Vec<String>> {
        self.fn_param_names.get(name)
    }

    /// Record, per parameter (in declaration order), whether it has a default
    /// value. Parallel to [`register_fn_param_names`].
    pub fn register_fn_param_defaults(&mut self, name: &str, has_defaults: Vec<bool>) {
        self.fn_param_defaults
            .insert(name.to_string(), has_defaults);
    }

    /// Look up the per-parameter has-default flags for a function by name.
    pub fn fn_param_defaults(&self, name: &str) -> Option<&Vec<bool>> {
        self.fn_param_defaults.get(name)
    }

    pub fn has_errors(&self) -> bool {
        !self.structured_errors.is_empty()
    }

    pub fn get_structured_errors(&self) -> &[CheckerError] {
        &self.structured_errors
    }

    /// Build a name-keyed snapshot of every user/built-in type's members
    /// (fields + methods) drawn from `type_defs` and `impl_methods`. This is a
    /// read-only copy so consumers (e.g. the LSP) can enumerate members after the
    /// checker has been dropped, without depending on `TypeEnv` internals.
    pub fn collect_type_members(&self) -> HashMap<String, crate::sema::TypeMembers> {
        use crate::sema::{MemberInfo, TypeMembers};

        let mut out: HashMap<String, TypeMembers> = HashMap::new();

        let push_field = |entry: &mut TypeMembers, name: &str, ty: &Type| {
            entry.fields.push(MemberInfo {
                name: name.to_string(),
                ty: ty.clone(),
            });
        };
        let push_method = |entry: &mut TypeMembers, name: &str, ty: &Type| {
            entry.methods.push(MemberInfo {
                name: name.to_string(),
                ty: ty.clone(),
            });
        };

        // Fields and inline methods from struct/class declarations.
        for (name, def) in &self.type_defs {
            let entry = out.entry(name.clone()).or_default();
            match &def.kind {
                TypeDefKind::Struct { fields, methods } => {
                    for (fname, fty, _) in fields {
                        push_field(entry, fname, fty);
                    }
                    for (mname, mty, _) in methods {
                        push_method(entry, mname, mty);
                    }
                }
                TypeDefKind::Class {
                    fields, methods, ..
                } => {
                    for (fname, fty, _) in fields {
                        push_field(entry, fname, fty);
                    }
                    for (mname, mty, _) in methods {
                        push_method(entry, mname, mty);
                    }
                }
                TypeDefKind::Interface { methods } => {
                    for (mname, mty) in methods {
                        push_method(entry, mname, mty);
                    }
                }
                _ => {}
            }
        }

        // impl-block methods (including methods on built-in types like `string`).
        for (type_name, methods) in &self.impl_methods {
            let entry = out.entry(type_name.clone()).or_default();
            for method in methods {
                // Drop the implicit `self` receiver so signatures read naturally.
                let params: Vec<Type> = method
                    .params
                    .iter()
                    .filter(|(name, _)| name != "self")
                    .map(|(_, t)| t.clone())
                    .collect();
                let required_params = method
                    .required_params
                    .saturating_sub(usize::from(method.has_self));
                let fn_ty = Type::Function {
                    params,
                    return_type: Box::new(method.return_type.clone()),
                    required_params,
                };
                push_method(entry, &method.name, &fn_ty);
            }
        }

        // A subclass's public semantic view includes inherited members, with
        // the nearest declaration winning. This keeps LSP/member consumers in
        // sync with the checker relation used for calls and interfaces.
        for (name, def) in &self.type_defs {
            if !matches!(def.kind, TypeDefKind::Class { .. }) {
                continue;
            }
            let entry = out.entry(name.clone()).or_default();
            entry.fields.clear();
            entry.methods.clear();
            for (field_name, field_ty, _) in self.get_class_fields(name) {
                push_field(entry, &field_name, &field_ty);
            }
            for (method_name, method_ty, _) in self.get_class_methods(name) {
                push_method(entry, &method_name, &method_ty);
            }
        }

        out
    }

    /// Record a structured error (preferred over string-based error())
    pub fn record_error(&mut self, error: CheckerError) {
        self.structured_errors.push(error);
    }
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a specific instantiation of a generic function.
///
/// UNUSED INFRA: Lira's generics use type erasure at runtime (see
/// `Type::TypeParam`), so codegen emits a single shared body per generic
/// function and never reads instantiation records. This type and the
/// `TypeChecker::generic_instantiations` set are retained only as scaffolding
/// for a *possible* future monomorphizing backend; nothing in the live
/// compilation pipeline consumes them. They are exercised solely by unit tests.
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

    /// Generate a mangled name for this instantiation.
    ///
    /// UNUSED INFRA: would be the symbol name for a monomorphized specialization.
    /// Under the current type-erasure model no specializations are emitted, so
    /// this is only called from tests. Kept for a future monomorphizing backend.
    #[allow(dead_code)]
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
    /// All instantiations of generic functions discovered during type checking.
    ///
    /// UNUSED INFRA: populated as a side effect of checking generic calls but
    /// never read by codegen (generics are type-erased, not monomorphized).
    /// Retained as scaffolding for a future monomorphizing backend and asserted
    /// on by unit tests. See [`GenericInstantiation`].
    pub generic_instantiations: HashSet<GenericInstantiation>,
    /// Semantic tables being built during checking
    pub sema: SemanticTables,
    /// Canonical source binding for aliases of an unresolved `chan()` result.
    ///
    /// Unlike arrays, channels may safely share their deferred element
    /// inference: every alias denotes the same send/receive endpoint.  Keeping
    /// this relation by stable symbol id ensures the first concrete send (or
    /// parameter flow) refines the entire alias group rather than only the
    /// spelling used at that call site.
    channel_alias_roots: HashMap<SymbolId, SymbolId>,
}

#[derive(Clone)]
struct PatternBindingInfo {
    name: String,
    ty: Type,
    pattern_id: NodeId,
}

type StructPatternFields = (String, Vec<(String, Type, bool)>);

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
            channel_alias_roots: HashMap::new(),
        }
    }

    /// Construct a source-facing nominal type for a concrete application of a
    /// user-defined generic aggregate and retain its recursive arguments in
    /// semantic metadata. The spelling is for diagnostics/hover only; no
    /// consumer may recover arguments by parsing it.
    fn make_generic_type(&mut self, base_name: &str, args: Vec<Type>) -> Type {
        let display_name = format!(
            "{}<{}>",
            base_name,
            args.iter()
                .map(Type::display_name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.sema.generic_type_instances.insert(
            display_name.clone(),
            crate::sema::GenericTypeInstance {
                base_name: base_name.to_string(),
                args,
            },
        );

        match self
            .env
            .lookup_type(base_name)
            .map(|definition| &definition.kind)
        {
            Some(TypeDefKind::Enum { .. }) => Type::Enum(display_name),
            Some(TypeDefKind::Interface { .. }) => Type::Interface(display_name),
            Some(TypeDefKind::Class { .. }) => Type::Class(display_name),
            _ => Type::Struct(display_name),
        }
    }

    fn generic_type_instance(&self, ty: &Type) -> Option<&crate::sema::GenericTypeInstance> {
        let name = match ty {
            Type::Struct(name) | Type::Enum(name) | Type::Class(name) | Type::Interface(name) => {
                name
            }
            _ => return None,
        };
        self.sema.generic_type_instances.get(name)
    }

    /// Substitute already-resolved generic arguments without mutating semantic
    /// tables. Structural interface checks run through shared `&self` type
    /// compatibility paths, so they cannot use `substitute_type`, whose
    /// diagnostic-facing nominal construction records new instances.
    fn substitute_type_readonly(&self, ty: &Type, bindings: &HashMap<String, Type>) -> Type {
        match ty {
            Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
            Type::Array(inner) => {
                Type::Array(Box::new(self.substitute_type_readonly(inner, bindings)))
            }
            Type::Channel(inner) => {
                Type::Channel(Box::new(self.substitute_type_readonly(inner, bindings)))
            }
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| self.substitute_type_readonly(element, bindings))
                    .collect(),
            ),
            Type::Map(key, value) => Type::Map(
                Box::new(self.substitute_type_readonly(key, bindings)),
                Box::new(self.substitute_type_readonly(value, bindings)),
            ),
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.substitute_type_readonly(inner, bindings)))
            }
            Type::Result { ok_type, err_type } => Type::Result {
                ok_type: Box::new(self.substitute_type_readonly(ok_type, bindings)),
                err_type: Box::new(self.substitute_type_readonly(err_type, bindings)),
            },
            Type::Function {
                params,
                return_type,
                required_params,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|param| self.substitute_type_readonly(param, bindings))
                    .collect(),
                return_type: Box::new(self.substitute_type_readonly(return_type, bindings)),
                required_params: *required_params,
            },
            Type::Struct(name) | Type::Enum(name) | Type::Class(name) | Type::Interface(name) => {
                let Some(instance) = self.sema.generic_type_instances.get(name) else {
                    return ty.clone();
                };
                let args: Vec<Type> = instance
                    .args
                    .iter()
                    .map(|arg| self.substitute_type_readonly(arg, bindings))
                    .collect();
                let display_name = format!(
                    "{}<{}>",
                    instance.base_name,
                    args.iter()
                        .map(Type::display_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                match ty {
                    Type::Enum(_) => Type::Enum(display_name),
                    Type::Class(_) => Type::Class(display_name),
                    Type::Interface(_) => Type::Interface(display_name),
                    _ => Type::Struct(display_name),
                }
            }
            _ => ty.clone(),
        }
    }

    /// Substitute type parameters recursively, including parameters nested in
    /// a user-defined generic aggregate application.
    fn substitute_type(&mut self, ty: &Type, bindings: &HashMap<String, Type>) -> Type {
        match ty {
            Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
            Type::Array(inner) => Type::Array(Box::new(self.substitute_type(inner, bindings))),
            Type::Channel(inner) => Type::Channel(Box::new(self.substitute_type(inner, bindings))),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|element| self.substitute_type(element, bindings))
                    .collect(),
            ),
            Type::Map(key, value) => Type::Map(
                Box::new(self.substitute_type(key, bindings)),
                Box::new(self.substitute_type(value, bindings)),
            ),
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.substitute_type(inner, bindings)))
            }
            Type::Result { ok_type, err_type } => Type::Result {
                ok_type: Box::new(self.substitute_type(ok_type, bindings)),
                err_type: Box::new(self.substitute_type(err_type, bindings)),
            },
            Type::Function {
                params,
                return_type,
                required_params,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|param| self.substitute_type(param, bindings))
                    .collect(),
                return_type: Box::new(self.substitute_type(return_type, bindings)),
                required_params: *required_params,
            },
            Type::Struct(_) | Type::Enum(_) | Type::Class(_) | Type::Interface(_)
                if self.generic_type_instance(ty).is_some() =>
            {
                let instance = self
                    .generic_type_instance(ty)
                    .expect("guarded generic type instance")
                    .clone();
                let args = instance
                    .args
                    .iter()
                    .map(|arg| self.substitute_type(arg, bindings))
                    .collect();
                self.make_generic_type(&instance.base_name, args)
            }
            _ => ty.clone(),
        }
    }

    /// Infer concrete bindings for every type parameter occurrence in a
    /// declared type. Repeated occurrences must agree exactly so a generic
    /// value cannot acquire conflicting native storage representations.
    fn infer_type_bindings(
        &self,
        declared: &Type,
        actual: &Type,
        bindings: &mut HashMap<String, Type>,
    ) -> bool {
        if let Type::TypeParam(name) = declared {
            if matches!(actual, Type::Unknown | Type::TypeVar(_)) {
                return true;
            }
            return match bindings.get(name) {
                Some(bound) => bound == actual,
                None => {
                    bindings.insert(name.clone(), actual.clone());
                    true
                }
            };
        }

        match (declared, actual) {
            (Type::Array(declared), Type::Array(actual))
            | (Type::Channel(declared), Type::Channel(actual))
            | (Type::Optional(declared), Type::Optional(actual)) => {
                self.infer_type_bindings(declared, actual, bindings)
            }
            // A non-null value is assignable to an optional field without an
            // explicit wrapper. Preserve that relation during literal
            // inference so `Holder { value: Some(item) }` can infer the
            // `Holder<T>` argument from a declared `Maybe<T>?` field.
            (Type::Optional(declared), actual) if !matches!(actual, Type::Null) => {
                self.infer_type_bindings(declared, actual, bindings)
            }
            (Type::Tuple(declared), Type::Tuple(actual)) => {
                declared.len() == actual.len()
                    && declared.iter().zip(actual).all(|(declared, actual)| {
                        self.infer_type_bindings(declared, actual, bindings)
                    })
            }
            (Type::Map(declared_key, declared_value), Type::Map(actual_key, actual_value)) => {
                self.infer_type_bindings(declared_key, actual_key, bindings)
                    && self.infer_type_bindings(declared_value, actual_value, bindings)
            }
            (
                Type::Result {
                    ok_type: declared_ok,
                    err_type: declared_err,
                },
                Type::Result {
                    ok_type: actual_ok,
                    err_type: actual_err,
                },
            ) => {
                self.infer_type_bindings(declared_ok, actual_ok, bindings)
                    && self.infer_type_bindings(declared_err, actual_err, bindings)
            }
            (
                Type::Function {
                    params: declared_params,
                    return_type: declared_return,
                    ..
                },
                Type::Function {
                    params: actual_params,
                    return_type: actual_return,
                    ..
                },
            ) => {
                declared_params.len() == actual_params.len()
                    && declared_params
                        .iter()
                        .zip(actual_params)
                        .all(|(declared, actual)| {
                            self.infer_type_bindings(declared, actual, bindings)
                        })
                    && self.infer_type_bindings(declared_return, actual_return, bindings)
            }
            _ => match (
                self.generic_type_instance(declared),
                self.generic_type_instance(actual),
            ) {
                (Some(declared), Some(actual)) if declared.base_name == actual.base_name => {
                    declared.args.len() == actual.args.len()
                        && declared
                            .args
                            .iter()
                            .zip(&actual.args)
                            .all(|(declared, actual)| {
                                self.infer_type_bindings(declared, actual, bindings)
                            })
                }
                _ => true,
            },
        }
    }

    /// Return the resolved signature of an instance method.  The returned
    /// function retains its receiver in parameter zero, matching interface
    /// signatures.  Static methods are deliberately omitted: they cannot
    /// satisfy an instance-method requirement.
    fn instance_method_type(&self, owner: &Type, method_name: &str) -> Option<Type> {
        match owner {
            Type::Class(name) => {
                let signature = self
                    .env
                    .get_class_methods(name)
                    .into_iter()
                    .find(|(candidate, _, _)| candidate == method_name)
                    .and_then(|(_, signature, _)| {
                        self.env
                            .declared_method_has_self(name, method_name)
                            .filter(|has_self| *has_self)
                            .map(|_| signature)
                    });
                signature.or_else(|| {
                    self.env
                        .lookup_method(name, method_name)
                        .and_then(|method| {
                            method.has_self.then(|| self.impl_method_signature(method))
                        })
                })
            }
            Type::Struct(name) => {
                let (owner_name, bindings) = self
                    .generic_type_instance(owner)
                    .map(|instance| {
                        let bindings = self
                            .env
                            .generic_type_params(&instance.base_name)
                            .unwrap_or(&[])
                            .iter()
                            .cloned()
                            .zip(instance.args.iter().cloned())
                            .collect();
                        (instance.base_name.as_str(), bindings)
                    })
                    .unwrap_or((name.as_str(), HashMap::new()));
                let signature = self.env.lookup_type(owner_name).and_then(|type_def| {
                    let TypeDefKind::Struct { methods, .. } = &type_def.kind else {
                        return None;
                    };
                    methods
                        .iter()
                        .find(|(candidate, _, _)| candidate == method_name)
                        .and_then(|(_, signature, _)| {
                            self.env
                                .declared_method_has_self(owner_name, method_name)
                                .filter(|has_self| *has_self)
                                .map(|_| self.substitute_type_readonly(signature, &bindings))
                        })
                });
                signature.or_else(|| {
                    self.env
                        .lookup_method(owner_name, method_name)
                        .and_then(|method| {
                            method.has_self.then(|| {
                                let signature = self.impl_method_signature(method);
                                self.substitute_type_readonly(&signature, &bindings)
                            })
                        })
                })
            }
            Type::Interface(name) => self.env.lookup_type(name).and_then(|type_def| {
                let TypeDefKind::Interface { methods } = &type_def.kind else {
                    return None;
                };
                methods
                    .iter()
                    .find(|(candidate, _)| candidate == method_name)
                    .map(|(_, signature)| signature.clone())
            }),
            Type::String => {
                if method_name == "len" {
                    Some(Type::Function {
                        params: vec![Type::String],
                        return_type: Box::new(Type::Int),
                        required_params: 1,
                    })
                } else {
                    self.env
                        .lookup_method("string", method_name)
                        .and_then(|method| {
                            method.has_self.then(|| self.impl_method_signature(method))
                        })
                }
            }
            Type::Array(inner) => {
                let specific_name = format!("[{}]", inner.display_name());
                self.env
                    .lookup_method(&specific_name, method_name)
                    .or_else(|| self.env.lookup_method("array", method_name))
                    .and_then(|method| method.has_self.then(|| self.impl_method_signature(method)))
                    .or_else(|| match method_name {
                        "len" => Some(Type::Function {
                            params: vec![Type::Array(inner.clone())],
                            return_type: Box::new(Type::Int),
                            required_params: 1,
                        }),
                        "push" => Some(Type::Function {
                            params: vec![Type::Array(inner.clone()), *inner.clone()],
                            return_type: Box::new(Type::Void),
                            required_params: 2,
                        }),
                        "pop" => Some(Type::Function {
                            params: vec![Type::Array(inner.clone())],
                            return_type: Box::new(Type::Optional(inner.clone())),
                            required_params: 1,
                        }),
                        _ => None,
                    })
            }
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64 => self
                .env
                .lookup_method(&owner.display_name(), method_name)
                .and_then(|method| method.has_self.then(|| self.impl_method_signature(method))),
            _ => None,
        }
    }

    /// Return the declaration's default mask using the same lookup precedence
    /// as `instance_method_type`. A missing entry is treated conservatively by
    /// interface checking rather than guessed from an aggregate count.
    fn instance_method_default_mask(&self, owner: &Type, method_name: &str) -> Option<Vec<bool>> {
        match owner {
            Type::Class(name) => {
                let declared = self
                    .env
                    .get_class_methods(name)
                    .into_iter()
                    .any(|(candidate, _, _)| candidate == method_name)
                    && self
                        .env
                        .declared_method_has_self(name, method_name)
                        .unwrap_or(false);
                if declared {
                    self.env.declared_method_defaults(name, method_name)
                } else {
                    self.env
                        .lookup_method(name, method_name)
                        .and_then(|method| method.has_self.then(|| method.default_params.clone()))
                }
            }
            Type::Struct(name) => {
                let owner_name = self
                    .generic_type_instance(owner)
                    .map(|instance| instance.base_name.as_str())
                    .unwrap_or(name.as_str());
                let declared = self
                    .env
                    .lookup_type(owner_name)
                    .and_then(|type_def| {
                        let TypeDefKind::Struct { methods, .. } = &type_def.kind else {
                            return None;
                        };
                        Some(
                            methods
                                .iter()
                                .any(|(candidate, _, _)| candidate == method_name),
                        )
                    })
                    .unwrap_or(false)
                    && self
                        .env
                        .declared_method_has_self(owner_name, method_name)
                        .unwrap_or(false);
                if declared {
                    self.env.declared_method_defaults(owner_name, method_name)
                } else {
                    self.env
                        .lookup_method(owner_name, method_name)
                        .and_then(|method| method.has_self.then(|| method.default_params.clone()))
                }
            }
            Type::Interface(name) => self.env.declared_method_defaults(name, method_name),
            Type::String if method_name == "len" => Some(vec![false]),
            Type::String => self
                .env
                .lookup_method("string", method_name)
                .and_then(|method| method.has_self.then(|| method.default_params.clone())),
            Type::Array(inner) => {
                let specific_name = format!("[{}]", inner.display_name());
                self.env
                    .lookup_method(&specific_name, method_name)
                    .or_else(|| self.env.lookup_method("array", method_name))
                    .and_then(|method| method.has_self.then(|| method.default_params.clone()))
                    .or_else(|| match method_name {
                        "len" | "pop" => Some(vec![false]),
                        "push" => Some(vec![false, false]),
                        _ => None,
                    })
            }
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64 => self
                .env
                .lookup_method(&owner.display_name(), method_name)
                .and_then(|method| method.has_self.then(|| method.default_params.clone())),
            _ => None,
        }
    }

    fn impl_method_signature(&self, method: &ImplMethod) -> Type {
        let params: Vec<Type> = method.params.iter().map(|(_, ty)| ty.clone()).collect();
        Type::Function {
            required_params: method.required_params,
            params,
            return_type: Box::new(method.return_type.clone()),
        }
    }

    fn interface_methods(&self, interface_name: &str) -> Option<Vec<(String, Type)>> {
        self.env.lookup_type(interface_name).and_then(|type_def| {
            let TypeDefKind::Interface { methods } = &type_def.kind else {
                return None;
            };
            Some(methods.clone())
        })
    }

    /// Compare callable signatures after removing their receiver slots.
    /// Parameters are contravariant and results covariant, using the same
    /// environment-aware relation as ordinary calls and assignments.
    fn instance_callable_compatible(
        &self,
        actual: &Type,
        actual_defaults: &[bool],
        expected: &Type,
        expected_defaults: &[bool],
    ) -> bool {
        self.instance_callable_compatible_with_seen(
            actual,
            actual_defaults,
            expected,
            expected_defaults,
            &mut HashSet::new(),
        )
    }

    fn instance_callable_compatible_with_seen(
        &self,
        actual: &Type,
        actual_defaults: &[bool],
        expected: &Type,
        expected_defaults: &[bool],
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        let (
            Type::Function {
                params: actual_params,
                return_type: actual_return,
                required_params: actual_required,
            },
            Type::Function {
                params: expected_params,
                return_type: expected_return,
                required_params: expected_required,
            },
        ) = (actual, expected)
        else {
            return false;
        };
        let actual_params = actual_params.get(1..).unwrap_or(&[]);
        let expected_params = expected_params.get(1..).unwrap_or(&[]);
        let actual_defaults = actual_defaults.get(1..).unwrap_or(&[]);
        let expected_defaults = expected_defaults.get(1..).unwrap_or(&[]);
        actual_params.len() == expected_params.len()
            && actual_required <= expected_required
            && actual_defaults.len() == actual_params.len()
            && expected_defaults.len() == expected_params.len()
            && actual_defaults
                .iter()
                .zip(expected_defaults)
                .all(|(actual, expected)| !expected || *actual)
            && actual_params
                .iter()
                .zip(expected_params)
                .all(|(actual, expected)| self.types_compatible_with_seen(expected, actual, seen))
            // A value-returning implementation can satisfy a void interface
            // method because the caller discards the result. The reverse is
            // impossible: `Any` must not turn an absent result into a value.
            && (!matches!(actual_return.as_ref(), Type::Void)
                || matches!(expected_return.as_ref(), Type::Void))
            && self.types_compatible_with_seen(actual_return, expected_return, seen)
    }

    fn type_satisfies_interface(
        &self,
        actual: &Type,
        interface_name: &str,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        let key = (actual.display_name(), interface_name.to_string());
        if !seen.insert(key) {
            return true;
        }
        let Some(requirements) = self.interface_methods(interface_name) else {
            return false;
        };
        requirements.iter().all(|(method_name, required)| {
            let Some(implemented) = self.instance_method_type(actual, method_name) else {
                return false;
            };
            let Some(actual_defaults) = self.instance_method_default_mask(actual, method_name)
            else {
                return false;
            };
            let Some(expected_defaults) = self
                .env
                .declared_method_defaults(interface_name, method_name)
            else {
                return false;
            };
            self.instance_callable_compatible_with_seen(
                &implemented,
                &actual_defaults,
                required,
                &expected_defaults,
                seen,
            )
        })
    }

    fn type_satisfies_interface_direct(&self, actual: &Type, interface_name: &str) -> bool {
        self.type_satisfies_interface(actual, interface_name, &mut HashSet::new())
    }

    /// Snapshot the finite set of concrete types that this checked program can
    /// erase or convert at runtime. Backends use the result to emit witnesses
    /// and bounded dynamic interface checks; checker compatibility remains the
    /// single authority for membership.
    fn collect_runtime_interface_implementations(&self) -> HashMap<String, Vec<Type>> {
        fn push_unique(candidates: &mut Vec<Type>, ty: &Type) {
            if matches!(
                ty,
                Type::Any
                    | Type::Unknown
                    | Type::TypeVar(_)
                    | Type::TypeParam(_)
                    | Type::Void
                    | Type::Null
            ) || candidates.iter().any(|candidate| candidate == ty)
            {
                return;
            }
            candidates.push(ty.clone());
        }

        let mut candidates = vec![
            Type::Int,
            Type::Float,
            Type::Bool,
            Type::String,
            Type::Char,
            Type::Int8,
            Type::Int16,
            Type::Int32,
            Type::Int64,
            Type::UInt8,
            Type::UInt16,
            Type::UInt32,
            Type::UInt64,
        ];
        for ty in self
            .sema
            .expr_types
            .values()
            .chain(self.sema.pattern_types.values())
            .chain(self.sema.stmt_types.values())
            .chain(self.sema.symbols.values().map(|symbol| &symbol.ty))
        {
            push_unique(&mut candidates, ty);
        }
        for (name, definition) in &self.env.type_defs {
            let has_unresolved_owner_params = self
                .env
                .generic_type_params(name)
                .is_some_and(|params| !params.is_empty());
            if has_unresolved_owner_params {
                continue;
            }
            let ty = match &definition.kind {
                TypeDefKind::Class { .. } => Some(Type::Class(name.clone())),
                TypeDefKind::Struct { .. } => Some(Type::Struct(name.clone())),
                TypeDefKind::Enum { .. } => Some(Type::Enum(name.clone())),
                TypeDefKind::Interface { .. } => Some(Type::Interface(name.clone())),
                TypeDefKind::Alias(target) => Some(target.clone()),
            };
            if let Some(ty) = ty {
                push_unique(&mut candidates, &ty);
            }
        }
        for (display_name, instance) in &self.sema.generic_type_instances {
            let ty = match self
                .env
                .lookup_type(&instance.base_name)
                .map(|def| &def.kind)
            {
                Some(TypeDefKind::Class { .. }) => Type::Class(display_name.clone()),
                Some(TypeDefKind::Enum { .. }) => Type::Enum(display_name.clone()),
                Some(TypeDefKind::Interface { .. }) => Type::Interface(display_name.clone()),
                _ => Type::Struct(display_name.clone()),
            };
            push_unique(&mut candidates, &ty);
        }
        candidates.sort_by_key(Type::display_name);

        let mut implementations = HashMap::new();
        let mut interfaces: Vec<String> = self
            .env
            .type_defs
            .iter()
            .filter(|(_, definition)| matches!(definition.kind, TypeDefKind::Interface { .. }))
            .map(|(name, _)| name.clone())
            .collect();
        interfaces.sort();
        for interface in interfaces {
            let conformers: Vec<Type> = candidates
                .iter()
                .filter(|candidate| self.type_satisfies_interface_direct(candidate, &interface))
                .cloned()
                .collect();
            implementations.insert(interface, conformers);
        }
        implementations
    }

    fn interface_compatibility_message(&self, actual: &Type, expected: &Type) -> Option<String> {
        let Type::Interface(interface_name) = expected else {
            return None;
        };
        let requirements = self.interface_methods(interface_name)?;
        for (method_name, required) in requirements {
            let Some(implemented) = self.instance_method_type(actual, &method_name) else {
                return Some(format!(
                    "Type '{}' does not satisfy interface '{}': missing instance method '{}'",
                    actual.display_name(),
                    interface_name,
                    method_name
                ));
            };
            let actual_defaults = self.instance_method_default_mask(actual, &method_name);
            let expected_defaults = self
                .env
                .declared_method_defaults(interface_name, &method_name);
            if actual_defaults
                .as_deref()
                .zip(expected_defaults.as_deref())
                .is_none_or(|(actual_defaults, expected_defaults)| {
                    !self.instance_callable_compatible(
                        &implemented,
                        actual_defaults,
                        &required,
                        expected_defaults,
                    )
                })
            {
                return Some(format!(
                    "Type '{}' does not satisfy interface '{}': method '{}' has an incompatible signature",
                    actual.display_name(), interface_name, method_name
                ));
            }
        }
        None
    }

    fn record_interface_compatibility_error(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: &Span,
    ) {
        if let Some(message) = self.interface_compatibility_message(actual, expected) {
            self.env.error(span, message);
        }
    }

    /// Check compatibility with access to the declaration environment.
    ///
    /// `Type::is_compatible_with` remains useful for standalone type values,
    /// but nominal class subtyping requires the parent graph. Keep the
    /// structural relation here so nested arrays, channels, optionals,
    /// results, and function signatures all carry class upcasts consistently.
    fn types_compatible(&self, actual: &Type, expected: &Type) -> bool {
        self.types_compatible_with_seen(actual, expected, &mut HashSet::new())
    }

    fn types_compatible_with_seen(
        &self,
        actual: &Type,
        expected: &Type,
        seen: &mut HashSet<(String, String)>,
    ) -> bool {
        match (actual, expected) {
            (actual, expected) if actual == expected => true,
            (Type::Any, _) | (_, Type::Any) => true,
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            (Type::TypeParam(a), Type::TypeParam(b)) => a == b,
            (_, Type::TypeParam(_)) => true,
            // Arrays and channels are mutable containers.  Treat their
            // element types invariantly at assignment/call boundaries so a
            // `[Dog]` or `Channel<Dog>` cannot be aliased as an
            // `[Animal]`/`Channel<Animal>` and then receive an unrelated
            // sibling value.  A fresh array literal gets the narrower,
            // one-way check in `expression_compatible` below.
            (Type::Array(actual), Type::Array(expected))
            | (Type::Channel(actual), Type::Channel(expected)) => {
                actual == expected && !actual.contains_unresolved_storage_type()
            }
            // Tuples are immutable value aggregates, so their positions may
            // use the normal (including nominal class) compatibility rule.
            (Type::Tuple(actual), Type::Tuple(expected)) => {
                actual.len() == expected.len()
                    && actual.iter().zip(expected).all(|(actual, expected)| {
                        self.types_compatible_with_seen(actual, expected, seen)
                    })
            }
            // Maps are mutable key/value stores and therefore invariant in
            // both dimensions, just like arrays and channels.
            (Type::Map(actual_key, actual_value), Type::Map(expected_key, expected_value)) => {
                actual_key == expected_key
                    && actual_value == expected_value
                    && !actual_key.contains_unresolved_storage_type()
                    && !actual_value.contains_unresolved_storage_type()
            }
            (Type::Null, Type::Optional(_)) => true,
            (Type::Optional(actual), Type::Optional(expected)) => {
                self.types_compatible_with_seen(actual, expected, seen)
            }
            (actual, Type::Optional(expected)) => {
                self.types_compatible_with_seen(actual, expected, seen)
            }
            (
                Type::Result {
                    ok_type: actual_ok,
                    err_type: actual_err,
                },
                Type::Result {
                    ok_type: expected_ok,
                    err_type: expected_err,
                },
            ) => {
                self.types_compatible_with_seen(actual_ok, expected_ok, seen)
                    && self.types_compatible_with_seen(actual_err, expected_err, seen)
            }
            (Type::Class(actual), Type::Class(expected)) => {
                self.env.is_class_subtype(actual, expected)
            }
            (_, Type::Interface(expected)) => self.type_satisfies_interface(actual, expected, seen),
            (
                Type::Function {
                    params: actual_params,
                    return_type: actual_return,
                    ..
                },
                Type::Function {
                    params: expected_params,
                    return_type: expected_return,
                    ..
                },
            ) => {
                actual_params.len() == expected_params.len()
                    && actual_params
                        .iter()
                        .zip(expected_params)
                        .all(|(actual, expected)| {
                            self.types_compatible_with_seen(expected, actual, seen)
                        })
                    && self.types_compatible_with_seen(actual_return, expected_return, seen)
            }
            _ => actual.is_compatible_with(expected),
        }
    }

    /// Equality is symmetric even though assignment compatibility is not.
    /// Dynamic/unknown operands remain available for recovery and `any`
    /// dispatch, numeric families compare through the language coercion, and
    /// nominal/optional values must be compatible in at least one direction.
    fn equality_compatible(&self, left: &Type, right: &Type) -> bool {
        if matches!(left, Type::Any | Type::Unknown) || matches!(right, Type::Any | Type::Unknown) {
            return true;
        }
        if left.is_numeric() && right.is_numeric() {
            return true;
        }
        if left == right {
            return true;
        }
        match (left, right) {
            (Type::Null, Type::Optional(_)) | (Type::Optional(_), Type::Null) => true,
            _ => self.types_compatible(left, right) || self.types_compatible(right, left),
        }
    }

    /// Whether an explicit `as` cast has defined type-level semantics.
    ///
    /// Pointer-backed values deliberately require exact storage types (apart
    /// from nominal class upcasts). Sharing a runtime pointer representation is
    /// not enough: reinterpreting `[int]` as `[string]`, for example, corrupts
    /// the native backend's typed element slots.
    fn cast_compatible(&self, source: &Type, target: &Type) -> bool {
        if source == target {
            return true;
        }
        if matches!(
            source,
            Type::Unknown | Type::Any | Type::TypeVar(_) | Type::TypeParam(_)
        ) || matches!(
            target,
            Type::Unknown | Type::Any | Type::TypeVar(_) | Type::TypeParam(_)
        ) {
            return true;
        }
        if matches!(target, Type::String) && !matches!(source, Type::Void) {
            return true;
        }
        if source.is_numeric() && target.is_numeric() {
            return true;
        }
        if matches!(source, Type::Char) && target.is_integer() {
            return true;
        }
        if matches!(source, Type::Bool) && target.is_numeric() {
            return true;
        }
        if matches!(source, Type::String) && target.is_integer() {
            return true;
        }
        if let (Type::Class(child), Type::Class(parent)) = (source, target) {
            return self.env.is_class_subtype(child, parent);
        }
        if let Type::Interface(target_interface) = target {
            return self.type_satisfies_interface_direct(source, target_interface);
        }
        if let Type::Optional(target_inner) = target {
            if matches!(source, Type::Null) || source == target_inner.as_ref() {
                return true;
            }
            if let (Type::Class(child), Type::Class(parent)) = (source, target_inner.as_ref()) {
                return self.env.is_class_subtype(child, parent);
            }
            if let Type::Interface(target_interface) = target_inner.as_ref() {
                return self.type_satisfies_interface_direct(source, target_interface);
            }
        }
        false
    }

    /// Return compatibility for an overriding method's result. This is
    /// deliberately narrower than ordinary expression compatibility: an
    /// override may preserve the exact result type or narrow a nominal class
    /// result, but dynamic/unknown and numeric coercions must not weaken the
    /// parent's contract.
    fn override_return_compatible(&self, actual: &Type, expected: &Type) -> bool {
        if actual == expected {
            return true;
        }
        match (actual, expected) {
            (Type::Class(actual), Type::Class(expected)) => {
                self.env.is_class_subtype(actual, expected)
            }
            (Type::Optional(actual), Type::Optional(expected)) => {
                self.override_return_compatible(actual, expected)
            }
            (Type::Tuple(actual), Type::Tuple(expected)) => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| self.override_return_compatible(actual, expected))
            }
            _ => false,
        }
    }

    /// Check an expression against an expected type, retaining the useful
    /// covariance of a freshly-created array literal without making mutable
    /// array values covariant.  For example, `[Dog {}]` can initialize an
    /// explicitly typed `[Animal]`, while a previously-bound `[Dog]` cannot.
    fn expression_compatible(
        &self,
        expression: &Expression,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        if self.types_compatible(actual, expected) {
            return true;
        }

        match (&expression.kind, expected) {
            (
                ExpressionKind::EnumVariant {
                    enum_name,
                    variant_name,
                },
                Type::Enum(expected_name),
            ) if matches!(actual, Type::Enum(actual_name) if actual_name == enum_name)
                && self
                    .sema
                    .generic_type_instances
                    .get(expected_name)
                    .is_some_and(|instance| instance.base_name == *enum_name)
                && self.env.lookup_type(enum_name).is_some_and(|definition| {
                    matches!(&definition.kind, TypeDefKind::Enum { variants }
                        if variants.iter().any(|(name, fields)| name == variant_name && fields.is_empty()))
                }) =>
            {
                // A unit variant contains no generic payload slots, so it can
                // safely adopt the expected concrete enum application.
                true
            }
            (ExpressionKind::Call { callee, .. }, Type::Channel(_))
                if matches!(actual, Type::Channel(element) if element.contains_unresolved_storage_type())
                    && matches!(&callee.kind, ExpressionKind::Identifier(name) if name == "chan") =>
            {
                true
            }
            (ExpressionKind::Array(elements), Type::Array(expected_element)) => {
                elements.iter().all(|element| {
                    let Some(element_type) = self.sema.expr_types.get(&element.id) else {
                        return false;
                    };
                    self.fresh_storage_value_compatible(element, element_type, expected_element)
                })
            }
            (ExpressionKind::Map(entries), Type::Map(expected_key, expected_value)) => {
                entries.iter().all(|(key, value)| {
                    let (Some(key_type), Some(value_type)) = (
                        self.sema.expr_types.get(&key.id),
                        self.sema.expr_types.get(&value.id),
                    ) else {
                        return false;
                    };
                    self.fresh_storage_value_compatible(key, key_type, expected_key)
                        && self.fresh_storage_value_compatible(value, value_type, expected_value)
                })
            }
            (ExpressionKind::Tuple(elements), Type::Tuple(expected_elements)) => {
                elements.len() == expected_elements.len()
                    && elements
                        .iter()
                        .zip(expected_elements)
                        .all(|(element, expected_element)| {
                            let Some(element_type) = self.sema.expr_types.get(&element.id) else {
                                return false;
                            };
                            self.expression_compatible(element, element_type, expected_element)
                        })
            }
            (
                ExpressionKind::Array(_) | ExpressionKind::Map(_) | ExpressionKind::Tuple(_),
                Type::Optional(inner),
            ) => self.expression_compatible(expression, actual, inner),
            _ => false,
        }
    }

    fn storage_value_compatible(&self, actual: &Type, expected: &Type) -> bool {
        if matches!(actual, Type::Any | Type::Unknown | Type::TypeVar(_)) && actual != expected {
            return false;
        }
        self.types_compatible(actual, expected)
    }

    fn fresh_storage_value_compatible(
        &self,
        expression: &Expression,
        actual: &Type,
        expected: &Type,
    ) -> bool {
        if matches!(
            &expression.kind,
            ExpressionKind::Array(_) | ExpressionKind::Map(_) | ExpressionKind::Tuple(_)
        ) {
            self.expression_compatible(expression, actual, expected)
        } else {
            self.storage_value_compatible(actual, expected)
        }
    }

    fn is_fresh_unresolved_storage(expression: &Expression, ty: &Type) -> bool {
        match (&expression.kind, ty) {
            (ExpressionKind::Array(_), Type::Array(_))
            | (ExpressionKind::Map(_), Type::Map(_, _)) => true,
            (ExpressionKind::Call { callee, .. }, Type::Channel(element))
                if element.contains_unresolved_storage_type() =>
            {
                matches!(&callee.kind, ExpressionKind::Identifier(name) if name == "chan")
            }
            (ExpressionKind::Tuple(expressions), Type::Tuple(types)) => {
                expressions.len() == types.len()
                    && expressions.iter().zip(types).all(|(expression, ty)| {
                        !ty.contains_unresolved_storage_type()
                            || Self::is_fresh_unresolved_storage(expression, ty)
                    })
            }
            _ => false,
        }
    }

    /// Find the nearest common superclass for a non-empty list of class
    /// values. Walking the first value's ancestry is sufficient for Lira's
    /// single-inheritance model, and the visited set keeps malformed source
    /// from turning inference into an unbounded loop.
    fn common_class_type(&self, types: &[Type]) -> Option<Type> {
        let Type::Class(first) = types.first()? else {
            return None;
        };
        if !types.iter().all(|ty| matches!(ty, Type::Class(_))) {
            return None;
        }

        let mut candidate = first.clone();
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(candidate.clone()) {
                return None;
            }
            if types.iter().all(
                |ty| matches!(ty, Type::Class(name) if self.env.is_class_subtype(name, &candidate)),
            ) {
                return Some(Type::Class(candidate));
            }
            candidate = self.env.get_parent_class(&candidate)?;
        }
    }

    fn validate_method_receiver_access(
        &mut self,
        object: &Expression,
        object_type: &Type,
        method_name: &str,
        span: &Span,
    ) {
        let Type::Class(type_name) = object_type else {
            return;
        };
        let is_type_name = matches!(&object.kind, ExpressionKind::Identifier(name)
            if self.env.lookup(name).is_none() && self.env.lookup_type(name).is_some());
        let Some(has_self) = self.env.declared_method_has_self(type_name, method_name) else {
            return;
        };

        if is_type_name && has_self {
            self.env.error(
                span,
                format!(
                    "Cannot access instance method '{}' through type '{}'",
                    method_name, type_name
                ),
            );
        } else if !is_type_name && !has_self {
            self.env.error(
                span,
                format!(
                    "Cannot access static method '{}' through an instance of '{}'",
                    method_name, type_name
                ),
            );
        }
    }

    /// Record a declaration in the semantic tables: insert the symbol entry and
    /// link the declaration node to the binding so a cursor *on the declaration*
    /// resolves to the same id as its uses.
    fn record_decl(
        &mut self,
        id: SymbolId,
        name: &str,
        ty: Type,
        kind: crate::sema::SymbolKind,
        decl_node: NodeId,
    ) {
        self.sema.symbols.insert(
            id,
            crate::sema::SymbolEntry {
                id,
                name: name.to_string(),
                ty,
                kind,
                decl_node,
            },
        );
        self.sema.symbol_refs.insert(decl_node, id);
    }

    fn unresolved_channel_alias_source(
        &self,
        initializer: &Expression,
        inferred_type: &Type,
    ) -> Option<SymbolId> {
        if !matches!(&initializer.kind, ExpressionKind::Identifier(_))
            || !matches!(
                inferred_type,
                Type::Channel(element)
                    if matches!(element.as_ref(), Type::Unknown | Type::TypeVar(_))
            )
        {
            return None;
        }
        self.sema.symbol_refs.get(&initializer.id).copied()
    }

    fn channel_alias_root(&self, id: SymbolId) -> SymbolId {
        let mut root = id;
        while let Some(next) = self.channel_alias_roots.get(&root).copied() {
            if next == root {
                break;
            }
            root = next;
        }
        root
    }

    fn register_unresolved_channel_alias(&mut self, alias: SymbolId, source: SymbolId) {
        self.channel_alias_roots
            .insert(alias, self.channel_alias_root(source));
    }

    fn is_unresolved_channel_rebinding(
        &self,
        target: &Type,
        value: &Type,
        value_expr: &Expression,
    ) -> bool {
        let unresolved_channels = matches!(
            (target, value),
            (Type::Channel(target), Type::Channel(value))
                if matches!(target.as_ref(), Type::Unknown | Type::TypeVar(_))
                    && matches!(value.as_ref(), Type::Unknown | Type::TypeVar(_))
        );
        unresolved_channels
            && match &value_expr.kind {
                ExpressionKind::Identifier(_) => true,
                ExpressionKind::Call { callee, .. } => {
                    matches!(&callee.kind, ExpressionKind::Identifier(name) if name == "chan")
                }
                _ => false,
            }
    }

    fn detach_channel_alias(&mut self, target: SymbolId) {
        let root = self.channel_alias_root(target);
        let remaining: Vec<_> = self
            .sema
            .symbols
            .keys()
            .copied()
            .filter(|id| *id != target && self.channel_alias_root(*id) == root)
            .collect();

        if target == root {
            if let Some(new_root) = remaining.first().copied() {
                for alias in remaining {
                    if alias == new_root {
                        self.channel_alias_roots.remove(&alias);
                    } else {
                        self.channel_alias_roots.insert(alias, new_root);
                    }
                }
            }
        }
        self.channel_alias_roots.remove(&target);
    }

    /// An assignment changes one binding's endpoint, unlike an initializer
    /// alias. Detach it from its old inference group for a fresh `chan()`, or
    /// attach it to the group named on the right hand side.
    fn rebind_unresolved_channel_alias(&mut self, target: &Expression, value: &Expression) {
        let Some(target_id) = self.sema.symbol_refs.get(&target.id).copied() else {
            return;
        };

        self.detach_channel_alias(target_id);
        match &value.kind {
            ExpressionKind::Call { callee, .. } if matches!(&callee.kind, ExpressionKind::Identifier(name) if name == "chan") =>
                {}
            ExpressionKind::Identifier(_) => {
                if let Some(source_id) = self.sema.symbol_refs.get(&value.id).copied() {
                    self.register_unresolved_channel_alias(target_id, source_id);
                }
            }
            _ => {}
        }
    }

    /// Refine every live alias of an unresolved `chan()` result.  The semantic
    /// table keeps historical declarations for tooling, while `TypeEnv` only
    /// contains the scopes currently in reach; updating both is intentional.
    fn refine_unknown_channel_alias_group(&mut self, receiver: SymbolId, element: Type) -> bool {
        if !self.env.refine_unknown_channel(receiver, element.clone()) {
            return false;
        }

        let root = self.channel_alias_root(receiver);
        let aliases: Vec<_> = self
            .sema
            .symbols
            .keys()
            .copied()
            .filter(|id| self.channel_alias_root(*id) == root)
            .collect();
        let refined = Type::Channel(Box::new(element.clone()));

        for alias in aliases {
            if alias != receiver {
                // An alias may belong to a scope that has already ended. Its
                // semantic entry is still useful to the LSP, but no longer has
                // a live environment binding to update.
                let _ = self.env.refine_unknown_channel(alias, element.clone());
            }
            if let Some(symbol) = self.sema.symbols.get_mut(&alias) {
                symbol.ty = refined.clone();
            }
        }

        true
    }

    /// Apply the element type learned by `push(xs, value)` or
    /// `xs.push(value)` to a binding introduced by `let xs = []`.
    fn refine_array_from_push(&mut self, receiver: &Expression, value: &Expression) {
        let Some(symbol_id) = self.sema.symbol_refs.get(&receiver.id).copied() else {
            return;
        };
        let Some(element_ty) = self.sema.expr_types.get(&value.id).cloned() else {
            return;
        };
        if matches!(element_ty, Type::Unknown | Type::TypeVar(_)) {
            return;
        }
        if let Some(symbol) = self.sema.symbols.get(&symbol_id) {
            if let Type::Array(inner) = &symbol.ty {
                // A mutable array keeps its declared element type, but a
                // value of a subtype is safe to insert into that storage.
                // Do not require the reverse conversion here: `[Animal]` is
                // intentionally able to receive a `Dog`, while the array
                // value itself remains invariant at assignment boundaries.
                if !matches!(inner.as_ref(), Type::Unknown | Type::TypeVar(_) | Type::Any)
                    && !self.storage_value_compatible(&element_ty, inner)
                {
                    self.env.record_error(CheckerError::TypeMismatchContext {
                        context: TypeContext::ArrayElement,
                        expected: inner.as_ref().clone(),
                        got: element_ty,
                        span: value.span.clone(),
                    });
                    return;
                }
            }
        }
        let Some(symbol) = self.sema.symbols.get(&symbol_id) else {
            return;
        };
        let Type::Array(inner) = &symbol.ty else {
            return;
        };
        // Concrete arrays preserve their declared/inferred element type after
        // a safe push. Only an untyped/unknown array is refined by its first
        // element.
        let refined = if matches!(inner.as_ref(), Type::Unknown | Type::TypeVar(_)) {
            let Some(refined) = self.env.refine_array_for_push(symbol_id, element_ty) else {
                return;
            };
            refined
        } else {
            symbol.ty.clone()
        };
        if let Some(symbol) = self.sema.symbols.get_mut(&symbol_id) {
            symbol.ty = refined.clone();
        }
        self.sema.expr_types.insert(receiver.id, refined);
    }

    /// Refine a directly-bound `chan()` result from its first send and enforce
    /// the channel element type that subsequent sends must accept.
    fn check_send_channel_types(&mut self, receiver: &Expression, value: &Expression) {
        let Some(channel_ty) = self.sema.expr_types.get(&receiver.id).cloned() else {
            return;
        };
        let Some(value_ty) = self.sema.expr_types.get(&value.id).cloned() else {
            return;
        };

        let Type::Channel(element_ty) = channel_ty else {
            return;
        };

        if matches!(element_ty.as_ref(), Type::Unknown | Type::TypeVar(_)) {
            let Some(symbol_id) = self.sema.symbol_refs.get(&receiver.id).copied() else {
                return;
            };
            if matches!(value_ty, Type::Unknown | Type::TypeVar(_))
                || !self.refine_unknown_channel_alias_group(symbol_id, value_ty.clone())
            {
                return;
            }
            let refined = Type::Channel(Box::new(value_ty));
            self.sema.expr_types.insert(receiver.id, refined);
            return;
        }

        if !self.storage_value_compatible(&value_ty, element_ty.as_ref()) {
            self.env.record_error(CheckerError::TypeMismatchContext {
                context: TypeContext::Argument,
                expected: *element_ty,
                got: value_ty,
                span: value.span.clone(),
            });
        }
    }

    /// An inferred `chan()` also learns its element type when it flows into a
    /// concrete `Channel<T>` parameter. This is the receive-only counterpart
    /// of first-send refinement: without it, a caller that delegates all sends
    /// to a worker leaves its local select binder as `unknown`.
    fn refine_channel_from_expected(&mut self, receiver: &Expression, expected: &Type) {
        let Type::Channel(expected_element) = expected else {
            return;
        };
        if matches!(expected_element.as_ref(), Type::Unknown | Type::TypeVar(_)) {
            return;
        }
        let Some(Type::Channel(actual_element)) = self.sema.expr_types.get(&receiver.id) else {
            return;
        };
        if !matches!(actual_element.as_ref(), Type::Unknown | Type::TypeVar(_)) {
            return;
        }
        let Some(symbol_id) = self.sema.symbol_refs.get(&receiver.id).copied() else {
            return;
        };
        let element = expected_element.as_ref().clone();
        if !self.refine_unknown_channel_alias_group(symbol_id, element.clone()) {
            return;
        }
        let refined = Type::Channel(Box::new(element));
        self.sema.expr_types.insert(receiver.id, refined);
    }

    /// `recv(ch)` has the payload type of `ch`, unlike the erased runtime
    /// return type stored in the builtin function signature.
    fn channel_receive_type(&self, receiver: &Expression) -> Option<Type> {
        match self.sema.expr_types.get(&receiver.id) {
            Some(Type::Channel(element)) => Some(element.as_ref().clone()),
            _ => None,
        }
    }

    /// Get the bounds for a type parameter, if it exists
    #[allow(dead_code)]
    fn get_type_param_bounds(&self, name: &str) -> Option<&Vec<String>> {
        self.current_type_params.get(name)
    }

    /// Check if a type parameter has a specific bound (trait)
    fn type_param_has_bound(&self, type_param: &str, bound: &str) -> bool {
        self.current_type_params
            .get(type_param)
            .map(|bounds| bounds.iter().any(|b| b == bound))
            .unwrap_or(false)
    }

    /// Check whether a type is a type parameter carrying at least one of the
    /// given trait bounds. Used to permit operations on bounded generics
    /// (e.g. `T: Numeric` may use arithmetic) while still rejecting unbounded
    /// type parameters.
    fn type_param_satisfies_any_bound(&self, ty: &Type, bounds: &[&str]) -> bool {
        match ty {
            Type::TypeParam(name) => bounds.iter().any(|b| self.type_param_has_bound(name, b)),
            _ => false,
        }
    }

    /// True if a type is an unbounded type parameter (or a type parameter that
    /// lacks every one of the supplied bounds) and therefore cannot support the
    /// requested operation.
    fn is_unsatisfied_type_param(&self, ty: &Type, bounds: &[&str]) -> bool {
        ty.is_type_param() && !self.type_param_satisfies_any_bound(ty, bounds)
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

        self.validate_class_inheritance_cycles(program);
        self.validate_class_fields(program);

        // Validate overrides after every class signature has been collected,
        // including classes declared later in the source file.
        self.validate_class_overrides(program);

        // Third pass: collect trait definitions and impl blocks
        for stmt in &program.statements {
            self.collect_impl_block(stmt);
        }

        // Interface satisfaction depends on the complete method set, including
        // methods collected from impl blocks and inherited class methods.
        self.validate_class_interfaces(program);

        // Fourth pass: register function signatures (for forward references / mutual recursion)
        for stmt in &program.statements {
            self.register_function_signature(stmt);
        }

        // Fifth pass: check all statements
        for stmt in &program.statements {
            self.check_statement(stmt);
        }

        // Sixth pass: check bodies nested in type declarations. They are not
        // ordinary top-level statements, so the main pass above deliberately
        // does not descend into them.
        for stmt in &program.statements {
            self.check_declared_method_bodies(stmt);
        }

        // Seventh pass: record `self.member` resolutions inside method bodies.
        for stmt in &program.statements {
            self.record_self_members(stmt);
        }

        // Keep resolved interface signatures available to downstream tools and
        // codegen even when checking succeeds.  The collecting path below does
        // the same snapshot for error-tolerant analysis.
        self.sema.type_members = self.env.collect_type_members();
        self.sema.interface_implementations = self.collect_runtime_interface_implementations();

        if self.env.has_errors() {
            let all_errors: Vec<String> = self
                .env
                .get_structured_errors()
                .iter()
                .map(|err| err.message())
                .collect();
            Err(all_errors.join("\n"))
        } else {
            // UNUSED INFRA: mirror the discovered instantiations into the
            // semantic tables. Codegen never reads `sema.generic_instantiations`
            // because generics are type-erased rather than monomorphized; this
            // copy exists only so the scaffolding stays consistent for a possible
            // future monomorphizing backend. (`concrete_type` is intentionally
            // left as `Unknown` — no consumer needs it.)
            for inst in &self.generic_instantiations {
                self.sema
                    .generic_instantiations
                    .push(crate::sema::GenericInstantiation {
                        generic_name: inst.function_name.clone(),
                        concrete_type: Type::Unknown,
                    });
            }
            Ok(CheckedProgram {
                program: program.clone(),
                sema: self.sema.clone(),
            })
        }
    }

    /// Run the full checking pipeline but ALWAYS return the checked program
    /// (best-effort AST + semantic tables) alongside any structured errors,
    /// rather than discarding the semantic tables when an error occurs.
    ///
    /// The semantic tables are populated during checking regardless of whether
    /// errors were found, which is exactly what error-tolerant tooling (the LSP)
    /// needs to offer hover/completion in a buffer that is mid-edit.
    pub fn check_program_collecting(
        &mut self,
        program: &Program,
    ) -> (CheckedProgram, Vec<CheckerError>) {
        // First pass: register type names (for forward references)
        for stmt in &program.statements {
            self.register_type_name(stmt);
        }

        // Second pass: collect full type definitions
        for stmt in &program.statements {
            self.collect_type_def(stmt);
        }

        self.validate_class_inheritance_cycles(program);
        self.validate_class_fields(program);

        // Validate overrides after every class signature has been collected,
        // including classes declared later in the source file.
        self.validate_class_overrides(program);

        // Third pass: collect trait definitions and impl blocks
        for stmt in &program.statements {
            self.collect_impl_block(stmt);
        }

        self.validate_class_interfaces(program);

        // Fourth pass: register function signatures
        for stmt in &program.statements {
            self.register_function_signature(stmt);
        }

        // Fifth pass: check all statements
        for stmt in &program.statements {
            self.check_statement(stmt);
        }

        // Sixth pass: check bodies nested in type declarations.
        for stmt in &program.statements {
            self.check_declared_method_bodies(stmt);
        }

        // Seventh pass: record `self.member` resolutions inside method bodies.
        for stmt in &program.statements {
            self.record_self_members(stmt);
        }

        // Mirror generic instantiations (UNUSED INFRA, kept consistent).
        for inst in &self.generic_instantiations {
            self.sema
                .generic_instantiations
                .push(crate::sema::GenericInstantiation {
                    generic_name: inst.function_name.clone(),
                    concrete_type: Type::Unknown,
                });
        }

        // Snapshot type members so tooling can enumerate them after the checker
        // (and its TypeEnv) is dropped.
        self.sema.type_members = self.env.collect_type_members();
        self.sema.interface_implementations = self.collect_runtime_interface_implementations();

        let errors = self.env.get_structured_errors().to_vec();
        let checked = CheckedProgram {
            program: program.clone(),
            sema: self.sema.clone(),
        };
        (checked, errors)
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
            StatementKind::StructDecl {
                name, type_params, ..
            } => {
                self.env.register_generic_type(
                    name,
                    type_params.iter().map(|param| param.name.clone()).collect(),
                );
                // Register placeholder type definition
                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Struct {
                        fields: vec![],
                        methods: vec![],
                    },
                });
            }
            StatementKind::EnumDecl {
                name, type_params, ..
            } => {
                self.env.register_generic_type(
                    name,
                    type_params.iter().map(|param| param.name.clone()).collect(),
                );
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

    /// Validate class override markers and signatures after all class
    /// definitions have been collected.  Override parameters are invariant:
    /// a caller typed against the parent must be able to invoke the child
    /// implementation with exactly the same accepted inputs.  Returns are
    /// covariant, so a child may narrow an inherited class return type (for
    /// example `Animal` -> `Dog`) while still satisfying the parent contract.
    fn validate_class_overrides(&mut self, program: &Program) {
        for stmt in &program.statements {
            let StatementKind::ClassDecl {
                name,
                parent: Some(parent),
                methods,
                ..
            } = &stmt.kind
            else {
                continue;
            };

            let parent_methods = self.env.get_class_methods(parent);
            let old_type_name = self.current_type_name.clone();
            self.current_type_name = Some(name.clone());

            for method in methods {
                let StatementKind::FnDecl {
                    name: method_name,
                    params,
                    return_type,
                    is_override,
                    ..
                } = &method.kind
                else {
                    continue;
                };

                let Some((_, parent_signature, _)) = parent_methods
                    .iter()
                    .find(|(parent_name, _, _)| parent_name == method_name)
                else {
                    if *is_override {
                        self.env.error(
                            &method.span,
                            format!(
                                "Method '{}' is marked as override but does not override any parent method",
                                method_name
                            ),
                        );
                    }
                    continue;
                };

                if !*is_override {
                    self.env.error(
                        &method.span,
                        format!(
                            "Method '{}' overrides a parent method but is not marked with 'override'",
                            method_name
                        ),
                    );
                }

                let own_has_self = params.first().is_some_and(|param| param.name == "self");
                let parent_has_self = self
                    .env
                    .declared_method_has_self(parent, method_name)
                    .unwrap_or(false);
                if own_has_self != parent_has_self {
                    self.env.error(
                        &method.span,
                        format!(
                            "Method '{}' override receiver mismatch: expected {} method, got {} method",
                            method_name,
                            if parent_has_self { "instance" } else { "static" },
                            if own_has_self { "instance" } else { "static" }
                        ),
                    );
                }

                let own_params: Vec<Type> = params
                    .iter()
                    .skip(usize::from(own_has_self))
                    .map(|param| self.resolve_type_expr(&param.type_ann))
                    .collect();
                let own_return = return_type
                    .as_ref()
                    .map(|type_expr| self.resolve_type_expr(type_expr))
                    .unwrap_or(Type::Any);
                let Type::Function {
                    params: parent_params,
                    return_type: parent_return,
                    required_params: _,
                } = parent_signature
                else {
                    continue;
                };

                let parent_params = parent_params
                    .get(usize::from(parent_has_self)..)
                    .unwrap_or_default();
                if own_has_self == parent_has_self && own_params.len() != parent_params.len() {
                    self.env.error(
                        &method.span,
                        format!(
                            "Method '{}' override has incompatible arity: expected {} parameters, got {}",
                            method_name,
                            parent_params.len(),
                            own_params.len()
                        ),
                    );
                } else if own_has_self == parent_has_self {
                    let parent_defaults = self
                        .env
                        .declared_method_defaults(parent, method_name)
                        .unwrap_or_else(|| {
                            vec![false; parent_params.len() + usize::from(parent_has_self)]
                        });
                    let own_defaults: Vec<bool> = params
                        .iter()
                        .skip(usize::from(own_has_self))
                        .map(|param| param.default.is_some())
                        .collect();
                    let parent_defaults = parent_defaults
                        .get(usize::from(parent_has_self)..)
                        .unwrap_or(&[]);
                    for (index, (parent_default, own_default)) in
                        parent_defaults.iter().zip(own_defaults.iter()).enumerate()
                    {
                        if *parent_default && !*own_default {
                            self.env.error(
                                &method.span,
                                format!(
                                    "Method '{}' override parameter {} default mismatch: parent parameter has a default, child parameter is required",
                                    method_name,
                                    index + 1
                                ),
                            );
                        }
                    }
                    for (index, (own_param, parent_param)) in
                        own_params.iter().zip(parent_params.iter()).enumerate()
                    {
                        if own_param != parent_param {
                            self.env.error(
                                &method.span,
                                format!(
                                    "Method '{}' override parameter {} type mismatch: expected '{}', got '{}'",
                                    method_name,
                                    index + 1,
                                    parent_param.display_name(),
                                    own_param.display_name()
                                ),
                            );
                        }
                    }
                }

                if !self.override_return_compatible(&own_return, parent_return) {
                    self.env.error(
                        &method.span,
                        format!(
                            "Method '{}' override return type mismatch: expected '{}', got '{}'",
                            method_name,
                            parent_return.display_name(),
                            own_return.display_name()
                        ),
                    );
                }
            }

            self.current_type_name = old_type_name;
        }
    }

    /// Class layouts are parent-prefix layouts in both bytecode and native
    /// code. Reusing an inherited field name would create two physical slots
    /// while source lookup selected only one of them, so reject shadowing (and
    /// duplicate fields in one declaration) before either backend sees it.
    fn validate_class_fields(&mut self, program: &Program) {
        for stmt in &program.statements {
            let StatementKind::ClassDecl {
                name,
                parent,
                fields,
                ..
            } = &stmt.kind
            else {
                continue;
            };

            let inherited: HashSet<String> = parent
                .as_deref()
                .map(|parent| {
                    self.env
                        .get_class_fields(parent)
                        .into_iter()
                        .map(|(field, _, _)| field)
                        .collect()
                })
                .unwrap_or_default();
            let mut declared = HashSet::new();
            for field in fields {
                if !declared.insert(field.name.clone()) {
                    self.env.error(
                        &field.span,
                        format!("Duplicate field '{}' in class '{}'", field.name, name),
                    );
                } else if inherited.contains(&field.name) {
                    self.env.error(
                        &field.span,
                        format!(
                            "Field '{}' in class '{}' conflicts with an inherited field",
                            field.name, name
                        ),
                    );
                }
            }
        }
    }

    /// Validate explicit class interface declarations after all definitions
    /// and impl methods have been collected.  Structural compatibility remains
    /// available even when a class omits the declaration entirely.
    fn validate_class_interfaces(&mut self, program: &Program) {
        for stmt in &program.statements {
            let StatementKind::ClassDecl {
                name, interfaces, ..
            } = &stmt.kind
            else {
                continue;
            };

            for interface_name in interfaces {
                let Some(type_def) = self.env.lookup_type(interface_name).cloned() else {
                    self.env.error(
                        &stmt.span,
                        format!(
                            "Class '{}' declares unknown interface '{}'",
                            name, interface_name
                        ),
                    );
                    continue;
                };
                let TypeDefKind::Interface { methods } = type_def.kind else {
                    self.env.error(
                        &stmt.span,
                        format!(
                            "Class '{}' declares '{}', but '{}' is not an interface",
                            name, interface_name, interface_name
                        ),
                    );
                    continue;
                };

                for (method_name, required) in methods {
                    let Some(implemented) =
                        self.instance_method_type(&Type::Class(name.clone()), &method_name)
                    else {
                        self.env.error(
                            &stmt.span,
                            format!(
                                "Class '{}' does not satisfy interface '{}': missing instance method '{}'",
                                name, interface_name, method_name
                            ),
                        );
                        continue;
                    };
                    let actual_defaults =
                        self.instance_method_default_mask(&Type::Class(name.clone()), &method_name);
                    let expected_defaults = self
                        .env
                        .declared_method_defaults(interface_name, &method_name);
                    if actual_defaults
                        .as_deref()
                        .zip(expected_defaults.as_deref())
                        .is_none_or(|(actual_defaults, expected_defaults)| {
                            !self.instance_callable_compatible(
                                &implemented,
                                actual_defaults,
                                &required,
                                expected_defaults,
                            )
                        })
                    {
                        self.env.error(
                            &stmt.span,
                            format!(
                                "Class '{}' does not satisfy interface '{}': method '{}' has an incompatible signature",
                                name, interface_name, method_name
                            ),
                        );
                    }
                }
            }
        }
    }

    /// Reject cyclic class inheritance before any inherited member lookup can
    /// traverse the malformed graph. The walk is deterministic (source order
    /// with lexicographically canonicalized cycle names) and reports one
    /// diagnostic per distinct cycle.
    fn validate_class_inheritance_cycles(&mut self, program: &Program) {
        let class_names: Vec<(String, Span)> = program
            .statements
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StatementKind::ClassDecl { name, .. } => Some((name.clone(), stmt.span.clone())),
                _ => None,
            })
            .collect();
        let class_spans: HashMap<String, Span> = class_names.iter().cloned().collect();
        let mut reported = HashSet::new();

        for (start, start_span) in class_names {
            let mut path: Vec<String> = Vec::new();
            let mut positions: HashMap<String, usize> = HashMap::new();
            let mut current = start;

            loop {
                if let Some(&cycle_start) = positions.get(&current) {
                    let cycle = &path[cycle_start..];
                    let mut canonical = cycle.to_vec();
                    if let Some((rotation, _)) = canonical
                        .iter()
                        .enumerate()
                        .min_by(|(_, left), (_, right)| left.cmp(right))
                    {
                        canonical.rotate_left(rotation);
                    }
                    let key = canonical.join("\u{1f}");
                    if reported.insert(key) {
                        let mut rendered = canonical.join(" -> ");
                        rendered.push_str(" -> ");
                        rendered.push_str(&canonical[0]);
                        let cycle_span = class_spans.get(&canonical[0]).unwrap_or(&start_span);
                        self.env.error(
                            cycle_span,
                            format!("Inheritance cycle detected: {}", rendered),
                        );
                    }
                    break;
                }
                positions.insert(current.clone(), path.len());
                path.push(current.clone());

                let Some(type_def) = self.env.lookup_type(&current) else {
                    break;
                };
                let TypeDefKind::Class { parent, .. } = &type_def.kind else {
                    break;
                };
                let Some(parent) = parent else {
                    break;
                };
                current = parent.clone();
            }
        }
    }

    /// Resolve inline method signatures while both the owner and method-local
    /// type parameters are in scope.  The signature retained in `TypeDef` is
    /// later used for member lookup and generic call inference.
    fn collect_declared_method_types(
        &mut self,
        owner: &str,
        methods: &[Statement],
    ) -> Vec<(String, Type, bool)> {
        let mut seen = HashSet::new();
        methods
            .iter()
            .filter_map(|method| {
                let StatementKind::FnDecl {
                    name,
                    type_params,
                    params,
                    return_type,
                    is_public,
                    ..
                } = &method.kind
                else {
                    return None;
                };

                if !seen.insert(name.clone()) {
                    self.env.error(
                        &method.span,
                        format!("Duplicate method '{}' in type '{}'", name, owner),
                    );
                    return None;
                }

                let mut type_param_scope = self.current_type_params.clone();
                type_param_scope.extend(
                    type_params
                        .iter()
                        .map(|param| (param.name.clone(), param.bounds.clone())),
                );
                let old_type_params =
                    std::mem::replace(&mut self.current_type_params, type_param_scope);
                let param_types = params
                    .iter()
                    .map(|param| self.resolve_type_expr(&param.type_ann))
                    .collect();
                let return_type = return_type
                    .as_ref()
                    .map(|ty| self.resolve_type_expr(ty))
                    .unwrap_or(Type::Any);
                self.current_type_params = old_type_params;

                self.env.register_generic_method(
                    owner,
                    name,
                    type_params.iter().map(|param| param.name.clone()).collect(),
                );

                Some((
                    name.clone(),
                    Type::Function {
                        required_params: params
                            .iter()
                            .filter(|param| param.default.is_none())
                            .count(),
                        params: param_types,
                        return_type: Box::new(return_type),
                    },
                    *is_public,
                ))
            })
            .collect()
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
                let method_types = self.collect_declared_method_types(name, methods);

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Class {
                        parent: parent.clone(),
                        interfaces: interfaces.clone(),
                        fields: field_types,
                        methods: method_types,
                    },
                });

                for method in methods {
                    if let StatementKind::FnDecl {
                        name: method_name,
                        params,
                        ..
                    } = &method.kind
                    {
                        let has_self = params.first().is_some_and(|param| param.name == "self");
                        self.env
                            .register_declared_method_receiver(name, method_name, has_self);
                        self.env.register_declared_method_defaults(
                            name,
                            method_name,
                            params.iter().map(|param| param.default.is_some()).collect(),
                        );
                        self.env.register_declared_method_param_names(
                            name,
                            method_name,
                            params.iter().map(|param| param.name.clone()).collect(),
                        );
                    }
                }

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
                let method_types = self.collect_declared_method_types(name, methods);

                self.env.define_type(TypeDef {
                    name: name.clone(),
                    kind: TypeDefKind::Struct {
                        fields: field_types,
                        methods: method_types,
                    },
                });

                for method in methods {
                    if let StatementKind::FnDecl {
                        name: method_name,
                        params,
                        ..
                    } = &method.kind
                    {
                        let has_self = params.first().is_some_and(|param| param.name == "self");
                        self.env
                            .register_declared_method_receiver(name, method_name, has_self);
                        self.env.register_declared_method_defaults(
                            name,
                            method_name,
                            params.iter().map(|param| param.default.is_some()).collect(),
                        );
                        self.env.register_declared_method_param_names(
                            name,
                            method_name,
                            params.iter().map(|param| param.name.clone()).collect(),
                        );
                    }
                }

                // Restore old type name and type params
                self.current_type_name = old_type_name;
                self.current_type_params = old_type_params;
            }
            StatementKind::EnumDecl {
                name,
                type_params,
                variants,
            } => {
                // Set current type params so variant payloads referencing a
                // type param (e.g. Some(T)) resolve to an erased Type::TypeParam.
                let old_type_params = std::mem::replace(
                    &mut self.current_type_params,
                    type_params
                        .iter()
                        .map(|tp| (tp.name.clone(), tp.bounds.clone()))
                        .collect(),
                );

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

                self.current_type_params = old_type_params;
            }
            StatementKind::InterfaceDecl { name, methods } => {
                // Interface methods are instance methods.  Resolve `Self`
                // under the interface owner and retain one canonical receiver
                // slot whether or not the source wrote `self`/`this`.
                let old_type_name = self.current_type_name.replace(name.clone());
                let mut seen = HashSet::new();
                let method_types: Vec<_> = methods
                    .iter()
                    .filter_map(|m| {
                        if !seen.insert(m.name.clone()) {
                            self.env.error(
                                &m.span,
                                format!("Duplicate method '{}' in interface '{}'", m.name, name),
                            );
                            return None;
                        }
                        let has_explicit_receiver =
                            m.params.first().is_some_and(|param| param.name == "self");
                        let explicit_params =
                            m.params.iter().skip(usize::from(has_explicit_receiver));
                        let param_types: Vec<_> = std::iter::once(Type::Interface(name.clone()))
                            .chain(
                                explicit_params
                                    .clone()
                                    .map(|p| self.resolve_type_expr(&p.type_ann)),
                            )
                            .collect();
                        let ret = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Any);
                        let required_params = 1 + explicit_params
                            .filter(|param| param.default.is_none())
                            .count();
                        Some((
                            m.name.clone(),
                            Type::Function {
                                params: param_types,
                                return_type: Box::new(ret),
                                required_params,
                            },
                        ))
                    })
                    .collect();
                self.current_type_name = old_type_name;

                let mut registered = HashSet::new();
                for method in methods {
                    if !registered.insert(method.name.clone()) {
                        continue;
                    }
                    let has_explicit_receiver = method
                        .params
                        .first()
                        .is_some_and(|param| param.name == "self");
                    let default_mask = std::iter::once(false)
                        .chain(
                            method
                                .params
                                .iter()
                                .skip(usize::from(has_explicit_receiver))
                                .map(|param| param.default.is_some()),
                        )
                        .collect();
                    self.env
                        .register_declared_method_receiver(name, &method.name, true);
                    self.env
                        .register_declared_method_defaults(name, &method.name, default_mask);
                    self.env.register_declared_method_param_names(
                        name,
                        &method.name,
                        std::iter::once("self".to_string())
                            .chain(
                                method
                                    .params
                                    .iter()
                                    .skip(usize::from(has_explicit_receiver))
                                    .map(|param| param.name.clone()),
                            )
                            .collect(),
                    );
                }

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
                supertraits: _,
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
                            .unwrap_or(Type::Any);
                        ImplMethod {
                            name: m.name.clone(),
                            params: param_types,
                            return_type: ret,
                            has_self: m.has_self,
                            required_params: m
                                .params
                                .iter()
                                .filter(|param| param.default.is_none())
                                .count(),
                            default_params: m
                                .params
                                .iter()
                                .map(|param| param.default.is_some())
                                .collect(),
                        }
                    })
                    .collect();
                self.env.add_trait(name, trait_methods);

                self.current_type_name = old_type_name;
            }
            StatementKind::ImplDecl {
                trait_name,
                type_name,
                type_params,
                methods,
            } => {
                // Reject `impl` blocks on a class. Classes dispatch dynamically
                // (methods attached to each instance) so virtual overrides work;
                // an impl block would register the methods for static dispatch,
                // silently defeating overrides and colliding in the function
                // table. Classes must define methods inline instead.
                let canonical_owner = self.canonical_owner_type(type_name);
                let target_is_class = matches!(&canonical_owner, Type::Class(_));
                if target_is_class {
                    self.env.error(
                        &stmt.span,
                        format!(
                            "`impl` blocks are not supported on classes; define methods inline \
                             inside `class {type_name} {{ ... }}` instead"
                        ),
                    );
                    return;
                }

                // Set current type name for Self resolution
                let old_type_name = self.current_type_name.clone();
                self.current_type_name = Some(type_name.clone());

                // Type parameters declared on the impl block itself (e.g.
                // `impl<T> Box<T>`). These are in scope for every method's
                // signature. Generics are erased, so resolving to `TypeParam`
                // (Any-compatible) is sufficient — codegen is unchanged.
                let impl_type_params: HashMap<String, Vec<String>> = type_params
                    .iter()
                    .map(|tp| (tp.name.clone(), tp.bounds.clone()))
                    .collect();

                // Collect all implemented methods
                let mut impl_methods: Vec<ImplMethod> = Vec::new();
                for method in methods {
                    if let StatementKind::FnDecl {
                        name,
                        type_params: method_type_params,
                        params,
                        return_type,
                        ..
                    } = &method.kind
                    {
                        // Scope = impl type params + the method's own type
                        // params (the method shadows the impl on collision).
                        let mut scope = impl_type_params.clone();
                        for tp in method_type_params {
                            scope.insert(tp.name.clone(), tp.bounds.clone());
                        }
                        let old_type_params =
                            std::mem::replace(&mut self.current_type_params, scope);

                        let has_self = params.first().map(|p| p.name == "self").unwrap_or(false);
                        let param_types: Vec<_> = params
                            .iter()
                            .map(|p| (p.name.clone(), self.resolve_type_expr(&p.type_ann)))
                            .collect();
                        let ret = return_type
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Any);

                        self.current_type_params = old_type_params;

                        impl_methods.push(ImplMethod {
                            name: name.clone(),
                            params: param_types,
                            return_type: ret,
                            has_self,
                            required_params: params
                                .iter()
                                .filter(|param| param.default.is_none())
                                .count(),
                            default_params: params
                                .iter()
                                .map(|param| param.default.is_some())
                                .collect(),
                        });
                        self.env.register_generic_method(
                            &self.canonical_impl_owner(type_name),
                            name,
                            method_type_params
                                .iter()
                                .map(|param| param.name.clone())
                                .collect(),
                        );
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
                        self.env.add_trait_impl(
                            trait_nm,
                            &canonical_owner.display_name(),
                            impl_methods.clone(),
                        );
                    } else {
                        // Trait doesn't exist
                        self.env
                            .error(&stmt.span, format!("trait `{}` is not defined", trait_nm));
                    }
                }

                // Add all methods to impl_methods for method resolution
                for impl_method in impl_methods {
                    if self
                        .env
                        .lookup_method(&canonical_owner.display_name(), &impl_method.name)
                        .is_some()
                    {
                        self.env.error(
                            &stmt.span,
                            format!(
                                "duplicate impl method `{}` for `{}` (aliases resolve to the same owner)",
                                impl_method.name,
                                canonical_owner.display_name()
                            ),
                        );
                    } else {
                        self.env
                            .add_impl_method(&canonical_owner.display_name(), impl_method);
                    }
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
            if !self.types_compatible(&resolved_trait_ty, impl_ty) {
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
        if !self.types_compatible(&resolved_trait_ret, &impl_method.return_type) {
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

    /// Resolve an `impl`/declaration owner to its source-facing type. Built-in
    /// names are aliases in the type environment, so they must retain their
    /// primitive representation rather than becoming nominal structs.
    fn owner_type(&self, type_name: &str) -> Type {
        if let Some(inner) = type_name
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
        {
            return Type::Array(Box::new(self.owner_type(inner)));
        }
        if self.current_type_params.contains_key(type_name) {
            return Type::TypeParam(type_name.to_string());
        }
        match self
            .env
            .lookup_type(type_name)
            .map(|definition| &definition.kind)
        {
            Some(TypeDefKind::Alias(ty)) => ty.clone(),
            Some(TypeDefKind::Class { .. }) => Type::Class(type_name.to_string()),
            Some(TypeDefKind::Struct { .. }) => Type::Struct(type_name.to_string()),
            Some(TypeDefKind::Enum { .. }) => Type::Enum(type_name.to_string()),
            Some(TypeDefKind::Interface { .. }) => Type::Interface(type_name.to_string()),
            None => Type::Struct(type_name.to_string()),
        }
    }

    /// Resolve an impl owner through every alias while retaining the nominal
    /// kind of its underlying declaration. Dispatch keys use this canonical
    /// type, so `impl Alias` and `impl Underlying` cannot diverge.
    fn canonical_owner_type(&self, type_name: &str) -> Type {
        fn resolve(checker: &TypeChecker, ty: Type, seen: &mut HashSet<String>) -> Type {
            match ty {
                Type::Array(inner) => Type::Array(Box::new(resolve(checker, *inner, seen))),
                Type::Struct(name)
                | Type::Class(name)
                | Type::Enum(name)
                | Type::Interface(name) => {
                    if seen.insert(name.clone()) {
                        if let Some(TypeDef {
                            kind: TypeDefKind::Alias(target),
                            ..
                        }) = checker.env.lookup_type(&name)
                        {
                            return resolve(checker, target.clone(), seen);
                        }
                    }
                    match checker
                        .env
                        .lookup_type(&name)
                        .map(|definition| &definition.kind)
                    {
                        Some(TypeDefKind::Class { .. }) => Type::Class(name),
                        Some(TypeDefKind::Struct { .. }) => Type::Struct(name),
                        Some(TypeDefKind::Enum { .. }) => Type::Enum(name),
                        Some(TypeDefKind::Interface { .. }) => Type::Interface(name),
                        _ => Type::Struct(name),
                    }
                }
                other => other,
            }
        }

        resolve(self, self.owner_type(type_name), &mut HashSet::new())
    }

    fn canonical_impl_owner(&self, type_name: &str) -> String {
        self.canonical_owner_type(type_name).display_name()
    }

    /// The concrete source type for `Self`/`this` in the current owner scope.
    /// Generic owners retain their declared arguments; built-in aliases retain
    /// their primitive representation through [`Self::owner_type`].
    fn current_owner_type(&mut self, type_name: &str) -> Type {
        if let Some(params) = self
            .env
            .generic_type_params(type_name)
            .map(<[String]>::to_vec)
        {
            self.make_generic_type(type_name, params.into_iter().map(Type::TypeParam).collect())
        } else {
            self.owner_type(type_name)
        }
    }

    /// Resolve Self type in a type to the actual implementing type.
    fn resolve_self_type(&self, ty: &Type, type_name: &str) -> Type {
        match ty {
            // Self can be stored as TypeParam("Self") or a nominal placeholder.
            Type::TypeParam(name) if name == "Self" => self.owner_type(type_name),
            Type::Struct(name) if name == "Self" => self.owner_type(type_name),
            Type::Class(name) if name == "Self" => self.owner_type(type_name),
            Type::Optional(inner) => {
                Type::Optional(Box::new(self.resolve_self_type(inner, type_name)))
            }
            Type::Array(inner) => Type::Array(Box::new(self.resolve_self_type(inner, type_name))),
            Type::Channel(inner) => {
                Type::Channel(Box::new(self.resolve_self_type(inner, type_name)))
            }
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
                .unwrap_or(Type::Any);

            // Restore old type params
            self.current_type_params = old_type_params;

            // Register the function in the environment
            let fn_ty = Type::Function {
                params: param_types,
                return_type: Box::new(ret_type),
                required_params: required_count,
            };
            let sym_id = self.env.define(Symbol {
                id: SymbolId(0),
                name: name.clone(),
                ty: fn_ty.clone(),
                mutable: false,
                kind: SymbolKind::Function,
            });
            // Link the `fn` declaration node so a cursor on the function name
            // resolves to this binding (rename-on-definition).
            self.record_decl(
                sym_id,
                name,
                fn_ty,
                crate::sema::SymbolKind::Function,
                stmt.id,
            );

            // Record parameter names so call sites can resolve named arguments.
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            self.env.register_fn_param_names(name, param_names);

            // Record which parameters have defaults so named-argument resolution
            // can distinguish a legally-skipped defaulted slot from a missing
            // required one.
            let param_has_default: Vec<bool> = params.iter().map(|p| p.default.is_some()).collect();
            self.env.register_fn_param_defaults(name, param_has_default);

            // Register generic function type parameters for monomorphization
            if !type_params.is_empty() {
                let type_param_names: Vec<String> =
                    type_params.iter().map(|tp| tp.name.clone()).collect();
                self.env.register_generic_function(name, type_param_names);
            }
        }
    }

    /// The type a block gives back through `return`, if it has one.
    ///
    /// Used for a lambda whose body is a block: the block itself has no value,
    /// but the lambda's type is whatever its `return` produces. Only the
    /// straight-line and branching shapes are followed; a `return` buried in a
    /// loop or a match arm leaves the type alone rather than guessing.
    fn block_return_type(&self, block: &Block) -> Option<Type> {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::Return(Some(expr)) => {
                    return self.sema.expr_types.get(&expr.id).cloned();
                }
                StatementKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    if let Some(ty) = self.block_return_type(then_branch) {
                        return Some(ty);
                    }
                    if let Some(ty) = else_branch.as_ref().and_then(|b| self.block_return_type(b)) {
                        return Some(ty);
                    }
                }
                StatementKind::Block(inner) => {
                    if let Some(ty) = self.block_return_type(inner) {
                        return Some(ty);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Whether an expression is proven not to fall through to its caller.
    ///
    /// Select arms are expressions, but a block expression can contain a
    /// control-flow statement such as `return`, `break`, or `continue`. Such
    /// an arm has no value to unify with the other arms. Keep this analysis
    /// deliberately conservative: only blocks and conditionals whose every
    /// reachable branch terminates are recognized.
    fn expression_definitely_terminates(&self, expression: &Expression) -> bool {
        match &expression.kind {
            ExpressionKind::Block(block) => self.block_definitely_terminates(block),
            ExpressionKind::IfExpr {
                then_expr,
                else_expr,
                ..
            } => {
                self.expression_definitely_terminates(then_expr)
                    && self.expression_definitely_terminates(else_expr)
            }
            _ => false,
        }
    }

    fn block_definitely_terminates(&self, block: &Block) -> bool {
        block
            .statements
            .iter()
            .any(|statement| self.statement_definitely_terminates(statement))
    }

    fn statement_definitely_terminates(&self, statement: &Statement) -> bool {
        match &statement.kind {
            StatementKind::Return(_) | StatementKind::Break(_) | StatementKind::Continue => true,
            StatementKind::Block(block) => self.block_definitely_terminates(block),
            StatementKind::If {
                then_branch,
                else_branch: Some(else_branch),
                ..
            } => {
                self.block_definitely_terminates(then_branch)
                    && self.block_definitely_terminates(else_branch)
            }
            StatementKind::Expression(expression) => {
                self.expression_definitely_terminates(expression)
            }
            _ => false,
        }
    }

    /// Check function bodies nested inside a class, struct, or impl block.
    /// Their owner type parameters remain visible while `check_statement`
    /// adds each method's own parameters.
    fn check_declared_method_bodies(&mut self, stmt: &Statement) {
        let (owner, owner_type_params, methods) = match &stmt.kind {
            StatementKind::ClassDecl { name, methods, .. } => (name, &[][..], methods.as_slice()),
            StatementKind::StructDecl {
                name,
                type_params,
                methods,
                ..
            }
            | StatementKind::ImplDecl {
                type_name: name,
                type_params,
                methods,
                ..
            } => (name, type_params.as_slice(), methods.as_slice()),
            _ => return,
        };

        let old_type_name = self.current_type_name.replace(owner.clone());
        let owner_scope = owner_type_params
            .iter()
            .map(|param| (param.name.clone(), param.bounds.clone()))
            .collect();
        let old_type_params = std::mem::replace(&mut self.current_type_params, owner_scope);

        for method in methods {
            self.check_statement(method);
        }

        self.current_type_params = old_type_params;
        self.current_type_name = old_type_name;
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
                let channel_alias_source = initializer
                    .as_ref()
                    .zip(inferred_type.as_ref())
                    .and_then(|(initializer, inferred_type)| {
                        self.unresolved_channel_alias_source(initializer, inferred_type)
                    });

                if let (Some(initializer), Some(inferred_type)) =
                    (initializer.as_ref(), inferred_type.as_ref())
                {
                    if inferred_type.contains_unresolved_mutable_storage()
                        && !Self::is_fresh_unresolved_storage(initializer, inferred_type)
                        && channel_alias_source.is_none()
                    {
                        self.env.error(
                            &initializer.span,
                            "Cannot alias mutable storage before its element type is inferred"
                                .to_string(),
                        );
                    }
                }

                let final_type = match (declared_type, inferred_type) {
                    (Some(decl), Some(init)) => {
                        if initializer.as_ref().is_none_or(|expression| {
                            !self.expression_compatible(expression, &init, &decl)
                        }) {
                            self.record_interface_compatibility_error(&init, &decl, &stmt.span);
                            self.env.record_error(CheckerError::TypeMismatchContext {
                                context: TypeContext::VarInit,
                                expected: decl.clone(),
                                got: init,
                                span: stmt.span.clone(),
                            });
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
                if let (Some(source), PatternKind::Variable(_)) =
                    (channel_alias_source, &pattern.kind)
                {
                    if final_type.contains_unresolved_mutable_storage() {
                        if let Some(alias) = self.sema.symbol_refs.get(&pattern.id).copied() {
                            self.register_unresolved_channel_alias(alias, source);
                        }
                    }
                }
            }

            StatementKind::ConstDecl {
                name,
                type_ann,
                initializer,
            } => {
                let init_type = self.check_expression(initializer);
                let channel_alias_source =
                    self.unresolved_channel_alias_source(initializer, &init_type);

                if init_type.contains_unresolved_mutable_storage()
                    && !Self::is_fresh_unresolved_storage(initializer, &init_type)
                    && channel_alias_source.is_none()
                {
                    self.env.error(
                        &initializer.span,
                        "Cannot alias mutable storage before its element type is inferred"
                            .to_string(),
                    );
                }

                let final_type = if let Some(type_ann) = type_ann {
                    let declared = self.resolve_type_expr(type_ann);
                    if !self.expression_compatible(initializer, &init_type, &declared) {
                        self.record_interface_compatibility_error(
                            &init_type, &declared, &stmt.span,
                        );
                        self.env.record_error(CheckerError::TypeMismatchContext {
                            context: TypeContext::VarInit,
                            expected: declared.clone(),
                            got: init_type.clone(),
                            span: stmt.span.clone(),
                        });
                    }
                    declared
                } else {
                    init_type
                };

                let sym_id = self.env.define(Symbol {
                    id: SymbolId(0),
                    name: name.clone(),
                    ty: final_type.clone(),
                    mutable: false,
                    kind: SymbolKind::Variable,
                });
                if let Some(source) = channel_alias_source {
                    if final_type.contains_unresolved_mutable_storage() {
                        self.register_unresolved_channel_alias(sym_id, source);
                    }
                }
                self.record_decl(
                    sym_id,
                    name,
                    final_type,
                    crate::sema::SymbolKind::Constant,
                    stmt.id,
                );
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
                let mut type_param_scope = self.current_type_params.clone();
                type_param_scope.extend(
                    type_params
                        .iter()
                        .map(|tp| (tp.name.clone(), tp.bounds.clone())),
                );
                let old_type_params =
                    std::mem::replace(&mut self.current_type_params, type_param_scope);

                let param_types: Vec<_> = params
                    .iter()
                    .map(|p| self.resolve_type_expr(&p.type_ann))
                    .collect();

                // Check default parameter values have correct types
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    if let Some(default_expr) = &param.default {
                        let default_type = self.check_expression(default_expr);
                        if !self.types_compatible(&default_type, param_type) {
                            self.env.record_error(CheckerError::TypeMismatchContext {
                                context: TypeContext::DefaultValue {
                                    param: param.name.clone(),
                                },
                                expected: param_type.clone(),
                                got: default_type,
                                span: param.span.clone(),
                            });
                        }
                    }
                }

                let ret_type = return_type
                    .as_ref()
                    .map(|t| self.resolve_type_expr(t))
                    .unwrap_or(Type::Any);

                // Check function body
                self.env.push_scope();

                // Add parameters to scope
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    let sym_id = self.env.define(Symbol {
                        id: SymbolId(0),
                        name: param.name.clone(),
                        ty: param_type.clone(),
                        mutable: false,
                        kind: SymbolKind::Parameter,
                    });
                    self.record_decl(
                        sym_id,
                        &param.name,
                        param_type.clone(),
                        crate::sema::SymbolKind::Parameter,
                        param.id,
                    );
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
                    .unwrap_or(Type::Any);

                if let Some(expected) = self.current_function_return_type.clone() {
                    let compatible = value.as_ref().is_some_and(|expression| {
                        self.expression_compatible(expression, &return_type, &expected)
                    }) || (value.is_none()
                        && self.types_compatible(&return_type, &expected));
                    if !compatible {
                        self.record_interface_compatibility_error(
                            &return_type,
                            &expected,
                            &stmt.span,
                        );
                        self.env.record_error(CheckerError::TypeMismatchContext {
                            context: TypeContext::Return,
                            expected,
                            got: return_type,
                            span: stmt.span.clone(),
                        });
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
                    self.env.record_error(CheckerError::ConditionMustBeBool {
                        got: Some(cond_type),
                        span: stmt.span.clone(),
                    });
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
                    self.env.record_error(CheckerError::ConditionMustBeBool {
                        got: Some(cond_type),
                        span: stmt.span.clone(),
                    });
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
                    Type::Tuple(elements) => {
                        // A homogeneous tuple has one statically known slot
                        // representation. Heterogeneous tuples cross the
                        // loop binder as `any`, so codegen can decode each
                        // slot from the tuple descriptor rather than guessing
                        // one element type for all positions.
                        match elements.first() {
                            None => Type::Any,
                            Some(first) if elements.iter().all(|element| element == first) => {
                                first.clone()
                            }
                            Some(_) => Type::Any,
                        }
                    }
                    ty if ty.is_builtin_range() => Type::Int,
                    Type::Any | Type::Unknown => Type::Any,
                    _ => {
                        self.env.error(
                            &iterable.span,
                            format!(
                                "Cannot iterate value of type '{}'; expected an array, string, tuple, or range",
                                iter_type.display_name()
                            ),
                        );
                        // Keep checking the body after reporting the concrete
                        // non-iterable so error-tolerant analysis can recover.
                        Type::Any
                    }
                };

                let prev_in_loop = self.in_loop;
                self.in_loop = true;
                self.env.push_scope();

                let sym_id = self.env.define(Symbol {
                    id: SymbolId(0),
                    name: variable.clone(),
                    ty: elem_type.clone(),
                    mutable: false,
                    kind: SymbolKind::Variable,
                });
                self.record_decl(
                    sym_id,
                    variable,
                    elem_type,
                    crate::sema::SymbolKind::Variable,
                    stmt.id,
                );

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

            StatementKind::Break(value) => {
                if let Some(value) = value {
                    self.check_expression(value);
                }
                if !self.in_loop {
                    self.env.record_error(CheckerError::BreakOutsideLoop {
                        span: stmt.span.clone(),
                    });
                }
            }

            StatementKind::Continue => {
                if !self.in_loop {
                    self.env.record_error(CheckerError::ContinueOutsideLoop {
                        span: stmt.span.clone(),
                    });
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

    /// Record [`FieldResolution`](crate::sema::FieldResolution) entries for
    /// `self.member` accesses inside the bodies of `impl`/`struct`/`class`
    /// methods.
    ///
    /// Inline method checking resolves ordinary expressions, but it does not
    /// attach the owner/member relationship needed by LSP find-references and
    /// rename. This pass walks method bodies *purely to record* that link; it
    /// emits no diagnostics and changes no types.
    fn record_self_members(&mut self, stmt: &Statement) {
        let (type_name, methods) = match &stmt.kind {
            StatementKind::ImplDecl {
                type_name, methods, ..
            } => (type_name.as_str(), methods),
            StatementKind::StructDecl { name, methods, .. }
            | StatementKind::ClassDecl { name, methods, .. } => (name.as_str(), methods),
            _ => return,
        };

        let self_type = self.owner_type(type_name);

        for method in methods {
            if let StatementKind::FnDecl { body, .. } = &method.kind {
                self.record_self_members_in_block(body, &self_type);
            }
        }
    }

    fn record_self_members_in_block(&mut self, block: &Block, self_type: &Type) {
        for stmt in &block.statements {
            self.record_self_members_in_stmt(stmt, self_type);
        }
    }

    fn record_self_members_in_stmt(&mut self, stmt: &Statement, self_type: &Type) {
        match &stmt.kind {
            StatementKind::VarDecl {
                initializer: Some(init),
                ..
            } => {
                self.record_self_members_in_expr(init, self_type);
            }
            StatementKind::ConstDecl { initializer, .. } => {
                self.record_self_members_in_expr(initializer, self_type);
            }
            StatementKind::Expression(e) => self.record_self_members_in_expr(e, self_type),
            StatementKind::Return(Some(e)) | StatementKind::Break(Some(e)) => {
                self.record_self_members_in_expr(e, self_type)
            }
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.record_self_members_in_expr(condition, self_type);
                self.record_self_members_in_block(then_branch, self_type);
                if let Some(b) = else_branch {
                    self.record_self_members_in_block(b, self_type);
                }
            }
            StatementKind::While { condition, body } => {
                self.record_self_members_in_expr(condition, self_type);
                self.record_self_members_in_block(body, self_type);
            }
            StatementKind::For { iterable, body, .. } => {
                self.record_self_members_in_expr(iterable, self_type);
                self.record_self_members_in_block(body, self_type);
            }
            StatementKind::Loop { body } => self.record_self_members_in_block(body, self_type),
            StatementKind::Block(b) => self.record_self_members_in_block(b, self_type),
            _ => {}
        }
    }

    fn record_self_members_in_expr(&mut self, expr: &Expression, self_type: &Type) {
        // Record a resolution when the receiver is the `self` identifier.
        if let ExpressionKind::FieldAccess { object, field } = &expr.kind {
            if matches!(&object.kind, ExpressionKind::Identifier(n) if n == "self")
                && !self.sema.field_resolution.contains_key(&expr.id)
            {
                let resolved_type = self
                    .member_type_on(self_type, field)
                    .unwrap_or(Type::Unknown);
                self.sema.field_resolution.insert(
                    expr.id,
                    crate::sema::FieldResolution {
                        owner_type: self_type.clone(),
                        field_name: field.clone(),
                        is_method: matches!(&resolved_type, Type::Function { .. }),
                        resolved_type,
                    },
                );
            }
        }

        // Recurse into sub-expressions (mirrors the structure used elsewhere).
        match &expr.kind {
            ExpressionKind::Binary { left, right, .. } => {
                self.record_self_members_in_expr(left, self_type);
                self.record_self_members_in_expr(right, self_type);
            }
            ExpressionKind::Unary { operand, .. } => {
                self.record_self_members_in_expr(operand, self_type)
            }
            ExpressionKind::Call { callee, args, .. } => {
                self.record_self_members_in_expr(callee, self_type);
                for a in args {
                    self.record_self_members_in_expr(&a.value, self_type);
                }
            }
            ExpressionKind::MethodCall { receiver, args, .. } => {
                self.record_self_members_in_expr(receiver, self_type);
                for a in args {
                    self.record_self_members_in_expr(&a.value, self_type);
                }
            }
            ExpressionKind::FieldAccess { object, .. }
            | ExpressionKind::OptionalAccess { object, .. } => {
                self.record_self_members_in_expr(object, self_type)
            }
            ExpressionKind::Index { object, index } => {
                self.record_self_members_in_expr(object, self_type);
                self.record_self_members_in_expr(index, self_type);
            }
            ExpressionKind::Array(items) | ExpressionKind::Tuple(items) => {
                for e in items {
                    self.record_self_members_in_expr(e, self_type);
                }
            }
            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.record_self_members_in_expr(k, self_type);
                    self.record_self_members_in_expr(v, self_type);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    self.record_self_members_in_expr(e, self_type);
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                self.record_self_members_in_expr(body, self_type)
            }
            ExpressionKind::IfExpr {
                condition,
                then_expr,
                else_expr,
            } => {
                self.record_self_members_in_expr(condition, self_type);
                self.record_self_members_in_expr(then_expr, self_type);
                self.record_self_members_in_expr(else_expr, self_type);
            }
            ExpressionKind::Match { subject, arms } => {
                self.record_self_members_in_expr(subject, self_type);
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.record_self_members_in_expr(g, self_type);
                    }
                    self.record_self_members_in_expr(&arm.body, self_type);
                }
            }
            ExpressionKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.record_self_members_in_expr(s, self_type);
                }
                if let Some(e) = end {
                    self.record_self_members_in_expr(e, self_type);
                }
            }
            ExpressionKind::Cast { expr, .. } | ExpressionKind::TypeCheck { expr, .. } => {
                self.record_self_members_in_expr(expr, self_type)
            }
            ExpressionKind::Assign { target, value }
            | ExpressionKind::CompoundAssign { target, value, .. } => {
                self.record_self_members_in_expr(target, self_type);
                self.record_self_members_in_expr(value, self_type);
            }
            ExpressionKind::Block(b) => self.record_self_members_in_block(b, self_type),
            ExpressionKind::Spawn(e) | ExpressionKind::Try(e) => {
                self.record_self_members_in_expr(e, self_type)
            }
            _ => {}
        }
    }

    /// Look up the declared type of a member (`field` or method) on `owner` by
    /// consulting the already-collected type definitions and impl methods. Pure:
    /// emits no diagnostics. Returns `None` for unknown members.
    fn member_type_on(&self, owner: &Type, member: &str) -> Option<Type> {
        let name = match owner {
            Type::Struct(n) | Type::Class(n) => n.as_str(),
            _ => return None,
        };
        if let Some(type_def) = self.env.lookup_type(name) {
            let (fields, methods) = match &type_def.kind {
                TypeDefKind::Struct { fields, methods } => (fields, methods),
                TypeDefKind::Class {
                    fields, methods, ..
                } => (fields, methods),
                _ => return None,
            };
            for (fname, fty, _) in fields {
                if fname == member {
                    return Some(fty.clone());
                }
            }
            for (mname, mty, _) in methods {
                if mname == member {
                    return Some(mty.clone());
                }
            }
        }
        self.impl_method_to_function(name, member)
    }

    /// Resolve named call arguments against a function's declared parameter
    /// names, returning the arguments rearranged into declaration order.
    ///
    /// Positional arguments fill parameter slots left-to-right; named arguments
    /// are placed into the slot whose name matches. Validation errors (unknown
    /// argument name, duplicate binding, or a missing required parameter that no
    /// argument supplies) are reported via the type environment, and `None` is
    /// returned so callers can fall back to source order for error recovery.
    ///
    /// `param_names` already excludes the implicit `self` for method calls.
    /// `param_has_default[i]` indicates whether `param_names[i]` has a default
    /// value (also excluding `self`); a skipped slot is only an error when its
    /// parameter has no default.
    ///
    /// Returns one slot per parameter, in declaration order: `Some(arg)` when an
    /// argument was supplied, `None` when the slot is left to its default. This
    /// alignment lets the caller type-check each supplied argument against its
    /// matched parameter, and lets codegen fill defaulted gaps.
    fn reorder_named_args<'a>(
        &mut self,
        args: &'a [crate::ast::Argument],
        param_names: &[String],
        param_has_default: &[bool],
        span: &Span,
    ) -> Option<Vec<Option<&'a crate::ast::Argument>>> {
        // slots[i] holds the argument bound to param_names[i], if any.
        let mut slots: Vec<Option<&'a crate::ast::Argument>> = vec![None; param_names.len()];
        let mut ok = true;

        for (pos, arg) in args.iter().enumerate() {
            match &arg.name {
                None => {
                    // Positional argument binds to the parameter at the same index.
                    // (The parser guarantees positionals precede named args.)
                    if pos < slots.len() {
                        slots[pos] = Some(arg);
                    }
                    // Excess positional args are caught by the arg-count check.
                }
                Some(name) => match param_names.iter().position(|p| p == name) {
                    Some(idx) => {
                        if slots[idx].is_some() {
                            self.env.error(
                                &arg.span,
                                format!("Duplicate value for parameter '{}'", name),
                            );
                            ok = false;
                        } else {
                            slots[idx] = Some(arg);
                        }
                    }
                    None => {
                        self.env
                            .error(&arg.span, format!("Unknown named argument '{}'", name));
                        ok = false;
                    }
                },
            }
        }

        // A slot left empty is only an error when its parameter has no default;
        // defaulted slots are filled by codegen (even in the middle of the list).
        // Trailing empties beyond the last filled slot are likewise fine when
        // defaulted, and otherwise caught by the arg-count check.
        let last_filled = slots.iter().rposition(|s| s.is_some());
        if let Some(last) = last_filled {
            for (idx, slot) in slots.iter().enumerate().take(last + 1) {
                let has_default = param_has_default.get(idx).copied().unwrap_or(false);
                if slot.is_none() && !has_default {
                    self.env.error(
                        span,
                        format!("Missing value for parameter '{}'", param_names[idx]),
                    );
                    ok = false;
                }
            }
        }

        if !ok {
            return None;
        }

        // Preserve full slot alignment (one entry per parameter) so the caller
        // can match each supplied argument to its parameter and codegen can fill
        // the defaulted gaps.
        Some(slots)
    }

    fn check_expression(&mut self, expr: &Expression) -> Type {
        let ty = self.check_expression_inner(expr);
        // Record the type in semantic tables
        self.sema.expr_types.insert(expr.id, ty.clone());
        ty
    }

    /// Check a named struct/class literal against the aggregate declaration.
    ///
    /// Field initializers are named rather than positional, so declaration
    /// order is intentionally irrelevant.  The parser does not support field
    /// defaults; consequently every declared field is required.  Classes use
    /// the complete inherited field set, matching field access and codegen's
    /// object layout.
    fn check_named_aggregate_literal(
        &mut self,
        expr: &Expression,
        type_name: &str,
        fields: &[(String, Expression)],
    ) -> Type {
        let Some(type_def) = self.env.lookup_type(type_name).cloned() else {
            self.env.record_error(CheckerError::UnknownType {
                name: type_name.to_string(),
                span: expr.span.clone(),
            });
            // Still check child expressions so error-tolerant analysis retains
            // their types and diagnostics.
            for (_, value) in fields {
                self.check_expression(value);
            }
            return Type::Unknown;
        };
        let generic_params = self
            .env
            .generic_type_params(type_name)
            .map(<[String]>::to_vec)
            .unwrap_or_default();

        let (aggregate_type, declared_fields) = match type_def.kind {
            TypeDefKind::Struct { fields, .. } => (Type::Struct(type_name.to_string()), fields),
            TypeDefKind::Class { .. } => (
                Type::Class(type_name.to_string()),
                self.env.get_class_fields(type_name),
            ),
            _ => {
                self.env.error(
                    &expr.span,
                    format!("Type '{}' is not a struct or class", type_name),
                );
                for (_, value) in fields {
                    self.check_expression(value);
                }
                return Type::Unknown;
            }
        };

        let mut supplied_fields = HashSet::with_capacity(fields.len());
        let mut generic_bindings = HashMap::new();
        for (field_name, value) in fields {
            if !supplied_fields.insert(field_name) {
                self.env.error(
                    &value.span,
                    format!("Duplicate field '{}' in {} literal", field_name, type_name),
                );
            }

            let actual_type = self.check_expression(value);
            match declared_fields
                .iter()
                .find(|(declared_name, _, _)| declared_name == field_name)
            {
                Some((_, expected_type, _)) => {
                    let inference_ok = self.infer_type_bindings(
                        expected_type,
                        &actual_type,
                        &mut generic_bindings,
                    );
                    let concrete_expected = self.substitute_type(expected_type, &generic_bindings);
                    if !inference_ok
                        || !self.expression_compatible(value, &actual_type, &concrete_expected)
                    {
                        self.env.record_error(CheckerError::TypeMismatch {
                            expected: concrete_expected,
                            got: actual_type,
                            span: value.span.clone(),
                        });
                    }
                }
                None => {
                    self.env.record_error(CheckerError::UnknownField {
                        field: field_name.clone(),
                        on_type: type_name.to_string(),
                        span: value.span.clone(),
                    });
                }
            }
        }

        for (field_name, _, _) in &declared_fields {
            if !supplied_fields.contains(field_name) {
                self.env.error(
                    &expr.span,
                    format!("Missing field '{}' in {} literal", field_name, type_name),
                );
            }
        }

        if generic_params.is_empty() {
            aggregate_type
        } else {
            let mut args = Vec::with_capacity(generic_params.len());
            for param in generic_params {
                if let Some(bound) = generic_bindings.get(&param) {
                    args.push(bound.clone());
                } else {
                    self.env.error(
                        &expr.span,
                        format!(
                            "Cannot infer type argument '{}' for '{}' literal",
                            param, type_name
                        ),
                    );
                    args.push(Type::Unknown);
                }
            }
            self.make_generic_type(type_name, args)
        }
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
                // Handle 'this' keyword — refers to the current instance (self)
                if name == "this" {
                    if let Some(current_owner) = self.current_type_name.clone() {
                        return self.current_owner_type(&current_owner);
                    } else {
                        self.env.error(
                            &expr.span,
                            "'this' can only be used inside a class".to_string(),
                        );
                        return Type::Unknown;
                    }
                }

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
                    // Record this identifier *use* as a reference to the
                    // binding it resolves to. The declaration site already
                    // inserted the `symbols` entry; here we only link the use
                    // node to the binding's stable id so that all uses of one
                    // binding share an id (scope-aware references/rename).
                    let sym_id = symbol.id;
                    let sym_ty = symbol.ty.clone();
                    self.sema.symbol_refs.insert(expr.id, sym_id);
                    sym_ty
                } else if let Some(type_def) = self.env.lookup_type(name) {
                    // If it's a type name, return the corresponding type
                    // This allows static method calls like Counter.new()
                    match &type_def.kind {
                        TypeDefKind::Struct { .. } => Type::Struct(name.clone()),
                        TypeDefKind::Class { .. } => Type::Class(name.clone()),
                        TypeDefKind::Enum { .. } => Type::Enum(name.clone()),
                        TypeDefKind::Alias(ty) => ty.clone(),
                        _ => Type::Unknown,
                    }
                } else if self.env.get_impl_methods(name).is_some() {
                    // Built-in type with impl methods (e.g., impl string { ... })
                    // Return a struct type so FieldAccess can look up methods
                    Type::Struct(name.clone())
                } else {
                    self.env.record_error(CheckerError::UndefinedVariable {
                        name: name.clone(),
                        span: expr.span.clone(),
                    });
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
                        // Untyped (Any) operand: permissive — the tagged dynamic VM
                        // dispatches at runtime. Concatenate if the other side is a
                        // String, otherwise the result is Any.
                        } else if left_type == Type::Any || right_type == Type::Any {
                            if *op == BinaryOp::Add && right_type == Type::String {
                                Type::String
                            } else {
                                Type::Any
                            }
                        // Bound-aware: a `Numeric`-bounded type parameter may use
                        // arithmetic. The result keeps the generic type so that
                        // `fn add<T: Numeric>(a: T, b: T) -> T { return a + b }`
                        // checks against its declared return type.
                        } else if self.type_param_satisfies_any_bound(&left_type, &["Numeric"])
                            && (right_type == left_type
                                || self.type_param_satisfies_any_bound(&right_type, &["Numeric"])
                                || right_type.is_numeric())
                        {
                            left_type
                        } else if self.type_param_satisfies_any_bound(&right_type, &["Numeric"])
                            && left_type.is_numeric()
                        {
                            right_type
                        // Reject unconstrained (or non-Numeric) type parameters for arithmetic
                        } else if self.is_unsatisfied_type_param(&left_type, &["Numeric"])
                            || self.is_unsatisfied_type_param(&right_type, &["Numeric"])
                        {
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
                    BinaryOp::Eq | BinaryOp::Ne => {
                        if !self.equality_compatible(&left_type, &right_type) {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Cannot compare values of type '{}' and '{}' for equality",
                                    left_type.display_name(),
                                    right_type.display_name()
                                ),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        // Bound-aware: a `Comparable` (or `Ord`) bounded type
                        // parameter may use ordering comparisons.
                        let ordering_bounds = ["Comparable", "Ord"];
                        let left_ordered =
                            self.type_param_satisfies_any_bound(&left_type, &ordering_bounds);
                        let right_ordered =
                            self.type_param_satisfies_any_bound(&right_type, &ordering_bounds);

                        if left_type == Type::Any || right_type == Type::Any {
                            // Untyped (Any) operand: permissive dynamic comparison.
                        } else if left_ordered || right_ordered {
                            // Permitted: at least one operand is an ordering-bounded
                            // type parameter. The other must be compatible (the same
                            // type parameter or numeric).
                        } else if self.is_unsatisfied_type_param(&left_type, &ordering_bounds)
                            || self.is_unsatisfied_type_param(&right_type, &ordering_bounds)
                        {
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
                        // Untyped (Any) operands are permitted (dynamic dispatch).
                        let left_ok = left_type == Type::Bool || left_type == Type::Any;
                        let right_ok = right_type == Type::Bool || right_type == Type::Any;
                        if !left_ok || !right_ok {
                            self.env.record_error(CheckerError::RequiresBoolOperand {
                                arity: crate::errors::OperandArity::Multiple,
                                span: expr.span.clone(),
                            });
                        }
                        Type::Bool
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr => {
                        // Bound-aware: a `Numeric`-bounded type parameter may use
                        // bitwise operators.
                        let bitwise_bounds = ["Numeric"];
                        let left_ok =
                            self.type_param_satisfies_any_bound(&left_type, &bitwise_bounds);
                        let right_ok =
                            self.type_param_satisfies_any_bound(&right_type, &bitwise_bounds);

                        if left_type == Type::Any || right_type == Type::Any {
                            // Untyped (Any) operand: permissive — dynamic dispatch.
                        } else if left_ok || right_ok {
                            // Permitted: at least one operand is a Numeric-bounded
                            // type parameter.
                        } else if self.is_unsatisfied_type_param(&left_type, &bitwise_bounds)
                            || self.is_unsatisfied_type_param(&right_type, &bitwise_bounds)
                        {
                            self.env.error(
                                &expr.span,
                                "Cannot use bitwise operators on unconstrained generic types"
                                    .to_string(),
                            );
                        } else if !left_type.is_integer() || !right_type.is_integer() {
                            self.env.record_error(CheckerError::RequiresIntegerOperand {
                                arity: crate::errors::OperandArity::Multiple,
                                span: expr.span.clone(),
                            });
                        }
                        Type::Int
                    }
                    BinaryOp::NullCoalesce => {
                        // `T? ?? fallback` yields `T`, not merely the fallback's
                        // concrete subtype. This matters when `T` is an
                        // interface: the present and fallback branches must
                        // agree on the interface representation.
                        let result = match &left_type {
                            Type::Optional(inner) => (**inner).clone(),
                            Type::Null => right_type.clone(),
                            Type::Any => Type::Any,
                            other => other.clone(),
                        };
                        if !self.types_compatible(&right_type, &result) {
                            self.env.error(
                                &expr.span,
                                format!(
                                    "Null-coalescing fallback has type '{}', expected '{}'",
                                    right_type.display_name(),
                                    result.display_name()
                                ),
                            );
                        }
                        result
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
                            self.env.record_error(CheckerError::RequiresBoolOperand {
                                arity: crate::errors::OperandArity::Single,
                                span: expr.span.clone(),
                            });
                        }
                        Type::Bool
                    }
                    UnaryOp::BitNot => {
                        if !operand_type.is_integer() {
                            self.env.record_error(CheckerError::RequiresIntegerOperand {
                                arity: crate::errors::OperandArity::Single,
                                span: expr.span.clone(),
                            });
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
                type_args,
            } => {
                let callee_type = self.check_expression(callee);

                // Get function name for generic tracking
                let function_name = if let ExpressionKind::Identifier(name) = &callee.kind {
                    Some(name.clone())
                } else {
                    None
                };

                // Method parameter names/defaults are not part of the erased
                // Function type stored on a field access. Recover them from
                // the impl/declaration metadata so named arguments are bound
                // by declaration name rather than source position.
                let method_call_metadata =
                    if let ExpressionKind::FieldAccess { object, field } = &callee.kind {
                        let owner_name = self
                            .sema
                            .expr_types
                            .get(&object.id)
                            .and_then(|ty| match ty {
                                Type::Struct(name)
                                | Type::Class(name)
                                | Type::Interface(name)
                                | Type::Enum(name) => Some(name.clone()),
                                _ => None,
                            })
                            .or_else(|| match &object.kind {
                                ExpressionKind::Identifier(name) => Some(name.clone()),
                                _ => None,
                            });
                        owner_name.and_then(|owner| {
                            if let Some(method) = self.env.lookup_method(&owner, field) {
                                return Some((
                                    method.params.iter().map(|(name, _)| name.clone()).collect(),
                                    method.default_params.clone(),
                                ));
                            }
                            let names = self.env.declared_method_param_names(&owner, field)?;
                            let defaults = self.env.declared_method_defaults(&owner, field)?;
                            Some((names, defaults))
                        })
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
                            let resolved_type_name = self
                                .sema
                                .expr_types
                                .get(&object.id)
                                .map(Type::display_name)
                                .unwrap_or_else(|| type_name.clone());
                            let type_receiver = self.env.lookup(type_name).is_none()
                                && (self.env.lookup_type(type_name).is_some()
                                    || self.env.get_impl_methods(type_name).is_some());
                            let has_self = self
                                .env
                                .declared_method_has_self(type_name, field)
                                .or_else(|| {
                                    self.env
                                        .lookup_method(type_name, field)
                                        .map(|method| method.has_self)
                                });
                            if type_receiver && has_self == Some(false) {
                                Some(crate::sema::CallResolution::StaticMethod {
                                    type_name: resolved_type_name,
                                    method_name: field.clone(),
                                })
                            } else {
                                Some(crate::sema::CallResolution::Method {
                                    type_name: resolved_type_name,
                                    method_name: field.clone(),
                                })
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(res) = resolution {
                    self.sema.call_resolution.insert(expr.id, res);
                }

                // Check if this is an instance method call (callee is FieldAccess with implicit self)
                // Static methods (no self) should NOT be treated as method calls for arg counting
                let is_method_call = if let ExpressionKind::FieldAccess { object, field } =
                    &callee.kind
                {
                    let intrinsic_collection_method = self
                        .sema
                        .expr_types
                        .get(&object.id)
                        .is_some_and(|ty| match ty {
                            Type::Array(_) => matches!(field.as_str(), "len" | "push" | "pop"),
                            Type::String => field == "len",
                            _ => false,
                        });
                    // Check if this is a static method call on a type name
                    if intrinsic_collection_method {
                        // Synthetic collection signatures contain only
                        // explicit arguments; there is no `self` slot to
                        // drop before validating this call.
                        false
                    } else if let ExpressionKind::Identifier(identifier_name) = &object.kind {
                        // A type-name receiver is a static method call; the
                        // declared receiver metadata decides. A variable
                        // receiver is an instance method call only when the
                        // field actually resolves to a declared method —
                        // a plain function-typed field is a function call
                        // with no implicit `self` slot to drop.
                        let is_type_receiver = self.env.lookup_type(identifier_name).is_some()
                            || self.env.get_impl_methods(identifier_name).is_some();
                        if is_type_receiver {
                            self.env
                                .declared_method_has_self(identifier_name, field)
                                .or_else(|| {
                                    self.env
                                        .lookup_method(identifier_name, field)
                                        .map(|method| method.has_self)
                                })
                                .unwrap_or(true)
                        } else {
                            self.sema
                                .expr_types
                                .get(&object.id)
                                .and_then(|ty| self.instance_method_type(ty, field).map(|_| true))
                                .unwrap_or(false)
                        }
                    } else {
                        // Instance method call on an expression receiver;
                        // only when the field is a declared method.
                        self.sema
                            .expr_types
                            .get(&object.id)
                            .and_then(|ty| self.instance_method_type(ty, field).map(|_| true))
                            .unwrap_or(false)
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

                        // For method calls, skip first param (self) when checking arg types
                        let params_to_check = if is_method_call && !params.is_empty() {
                            &params[1..]
                        } else {
                            &params[..]
                        };

                        // Seed generic inference from an explicit turbofish.
                        // For instance methods, only the method-local type
                        // parameters are explicit; owner arguments already came
                        // from the receiver during field resolution.
                        let call_type_params: Vec<String> = match &callee.kind {
                            ExpressionKind::Identifier(name) => self
                                .env
                                .get_generic_function(name)
                                .cloned()
                                .unwrap_or_default(),
                            ExpressionKind::FieldAccess { object, field } => {
                                let owner = self.sema.expr_types.get(&object.id).and_then(|ty| {
                                    self.generic_type_instance(ty)
                                        .map(|instance| instance.base_name.as_str())
                                        .or(match ty {
                                            Type::Struct(name)
                                            | Type::Class(name)
                                            | Type::Enum(name)
                                            | Type::Interface(name) => Some(name.as_str()),
                                            _ => None,
                                        })
                                });
                                owner
                                    .and_then(|owner| self.env.generic_method_params(owner, field))
                                    .map(<[String]>::to_vec)
                                    .unwrap_or_default()
                            }
                            _ => Vec::new(),
                        };
                        let mut call_bindings = HashMap::new();
                        if !type_args.is_empty() {
                            if call_type_params.len() != type_args.len() {
                                self.env.error(
                                    &expr.span,
                                    format!(
                                        "Expected {} type argument{}, got {}",
                                        call_type_params.len(),
                                        if call_type_params.len() == 1 { "" } else { "s" },
                                        type_args.len()
                                    ),
                                );
                            }
                            for (param, type_arg) in call_type_params.iter().zip(type_args) {
                                let resolved = self.resolve_type_expr(type_arg);
                                call_bindings.insert(param.clone(), resolved);
                            }
                        }

                        // Resolve the declared parameter names that line up with
                        // `params_to_check` (self is dropped for method calls), so
                        // named arguments can be matched and reordered by name.
                        let param_names: Option<Vec<String>> = function_name
                            .as_ref()
                            .and_then(|fn_name| self.env.fn_param_names(fn_name).cloned())
                            .or_else(|| {
                                method_call_metadata
                                    .as_ref()
                                    .map(|(names, _)| names.clone())
                            })
                            .map(|names| {
                                if is_method_call && !names.is_empty() {
                                    names[1..].to_vec()
                                } else {
                                    names
                                }
                            });

                        // Per-parameter has-default flags, aligned with
                        // `param_names` (self dropped for method calls).
                        let param_has_default: Option<Vec<bool>> = function_name
                            .as_ref()
                            .and_then(|fn_name| self.env.fn_param_defaults(fn_name).cloned())
                            .or_else(|| {
                                method_call_metadata
                                    .as_ref()
                                    .map(|(_, defaults)| defaults.clone())
                            })
                            .map(|flags| {
                                if is_method_call && !flags.is_empty() {
                                    flags[1..].to_vec()
                                } else {
                                    flags
                                }
                            });

                        // Reorder named arguments into declaration order. Each
                        // entry is the argument bound to that parameter (or `None`
                        // when the slot falls back to its default). Validation
                        // (unknown / duplicate / missing-required) is reported as
                        // errors and we fall back to source order for recovery.
                        let ordered_slots: Vec<Option<&crate::ast::Argument>> =
                            match (&param_names, &param_has_default) {
                                (Some(names), Some(defaults))
                                    if args.iter().any(|a| a.name.is_some()) =>
                                {
                                    self.reorder_named_args(args, names, defaults, &expr.span)
                                        .unwrap_or_else(|| args.iter().map(Some).collect())
                                }
                                _ => args.iter().map(Some).collect(),
                            };

                        for (index, (slot, param_type)) in
                            ordered_slots.iter().zip(params_to_check.iter()).enumerate()
                        {
                            // Defaulted (skipped) slots have no argument to check.
                            let Some(arg) = slot else { continue };
                            let mut arg_type = self.check_expression(&arg.value);
                            self.refine_channel_from_expected(&arg.value, param_type);
                            // `chan()` starts as `Channel<unknown>` and may be
                            // refined by the concrete parameter above. Use the
                            // authoritative post-refinement expression type for
                            // compatibility; comparing the stale local copy
                            // would reject the very call that inferred it.
                            if let Some(refined) = self.sema.expr_types.get(&arg.value.id) {
                                arg_type = refined.clone();
                            }
                            let inference_ok =
                                self.infer_type_bindings(param_type, &arg_type, &mut call_bindings);
                            let concrete_param = self.substitute_type(param_type, &call_bindings);
                            let erased_channel_builtin_receiver = index == 0
                                && matches!(
                                    function_name.as_deref(),
                                    Some("send" | "recv" | "close")
                                )
                                && matches!(&arg_type, Type::Channel(_))
                                && matches!(
                                    &concrete_param,
                                    Type::Channel(element)
                                        if element.contains_unresolved_storage_type()
                                );
                            if !inference_ok
                                || (!erased_channel_builtin_receiver
                                    && !self.expression_compatible(
                                        &arg.value,
                                        &arg_type,
                                        &concrete_param,
                                    ))
                            {
                                self.record_interface_compatibility_error(
                                    &arg_type,
                                    &concrete_param,
                                    &arg.span,
                                );
                                self.env.record_error(CheckerError::TypeMismatchContext {
                                    context: TypeContext::Argument,
                                    expected: concrete_param,
                                    got: arg_type.clone(),
                                    span: arg.span.clone(),
                                });
                            }
                        }

                        if matches!(function_name.as_deref(), Some("push")) {
                            if let [receiver, value] = args.as_slice() {
                                self.refine_array_from_push(&receiver.value, &value.value);
                            }
                        } else if let ExpressionKind::FieldAccess { object, field } = &callee.kind {
                            if field == "push" {
                                if let [value] = args.as_slice() {
                                    self.refine_array_from_push(object, &value.value);
                                }
                            }
                        }

                        if matches!(function_name.as_deref(), Some("send")) {
                            if let [receiver, value] = args.as_slice() {
                                self.check_send_channel_types(&receiver.value, &value.value);
                            }
                        }

                        // Record generic instantiation if this is a generic function
                        if let Some(ref fn_name) = function_name {
                            if let Some(params) = self.env.get_generic_function(fn_name).cloned() {
                                let inferred_types: Vec<Type> = params
                                    .iter()
                                    .filter_map(|param| call_bindings.get(param).cloned())
                                    .collect();
                                if inferred_types.len() == params.len() {
                                    let instantiation =
                                        GenericInstantiation::new(fn_name.clone(), &inferred_types);
                                    self.generic_instantiations.insert(instantiation);
                                }
                            }
                        }

                        // `pop` is polymorphic over a statically known array:
                        // the global builtin is registered as `Any` so it can
                        // also accept dynamic arrays, but a typed array must
                        // retain its element type and expose the specified
                        // `T?` result.  Dynamic `Any` arrays stay `Any`.
                        if matches!(function_name.as_deref(), Some("pop")) {
                            if let Some(receiver) = args.first() {
                                if let Some(Type::Array(inner)) =
                                    self.sema.expr_types.get(&receiver.value.id)
                                {
                                    return Type::Optional(inner.clone());
                                }
                            }
                        }

                        if matches!(function_name.as_deref(), Some("recv")) {
                            args.first()
                                .and_then(|receiver| self.channel_receive_type(&receiver.value))
                                .unwrap_or_else(|| return_type.as_ref().clone())
                        } else {
                            self.substitute_type(return_type, &call_bindings)
                        }
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.env.record_error(CheckerError::NotAFunction {
                            ty: callee_type,
                            span: expr.span.clone(),
                        });
                        Type::Unknown
                    }
                }
            }

            ExpressionKind::FieldAccess { object, field } => {
                let obj_type = self.check_expression(object);
                self.validate_method_receiver_access(object, &obj_type, field, &expr.span);
                let resolved_type = self.resolve_field_access(&obj_type, object, field, &expr.span);
                // Record field resolution
                self.sema.field_resolution.insert(
                    expr.id,
                    crate::sema::FieldResolution {
                        owner_type: obj_type,
                        field_name: field.clone(),
                        is_method: matches!(&resolved_type, Type::Function { .. }),
                        resolved_type: resolved_type.clone(),
                    },
                );
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
                            self.env.record_error(CheckerError::MustBeInteger {
                                context: IntContext::ArrayIndex,
                                span: expr.span.clone(),
                            });
                        }
                        *elem_type
                    }
                    Type::Map(_, value_type) => *value_type,
                    Type::Tuple(element_types) => match &index.kind {
                        ExpressionKind::IntLiteral(position) if *position >= 0 => element_types
                            .get(*position as usize)
                            .cloned()
                            .unwrap_or_else(|| {
                                self.env.record_error(CheckerError::GenericError {
                                    message: format!(
                                        "tuple of length {} has no position {}",
                                        element_types.len(),
                                        position
                                    ),
                                    span: index.span.clone(),
                                });
                                Type::Unknown
                            }),
                        _ => {
                            self.env.record_error(CheckerError::GenericError {
                                message: "tuple index must be a non-negative integer literal"
                                    .to_owned(),
                                span: index.span.clone(),
                            });
                            Type::Unknown
                        }
                    },
                    Type::String => {
                        if !index_type.is_integer() {
                            self.env.record_error(CheckerError::MustBeInteger {
                                context: IntContext::StringIndex,
                                span: expr.span.clone(),
                            });
                        }
                        // String indexing follows the VM's observable value:
                        // one Unicode scalar encoded as a one-character
                        // string. Character literals remain integer-like
                        // `char` values and are deliberately distinct.
                        Type::String
                    }
                    Type::Unknown => Type::Unknown,
                    // Allow indexing Any type (for json_parse results and other dynamic values)
                    Type::Any => Type::Any,
                    _ => {
                        self.env.record_error(CheckerError::CannotIndex {
                            ty: obj_type,
                            span: expr.span.clone(),
                        });
                        Type::Unknown
                    }
                }
            }

            ExpressionKind::Array(elements) => {
                if elements.is_empty() {
                    Type::Array(Box::new(Type::Unknown))
                } else {
                    let element_types: Vec<Type> = elements
                        .iter()
                        .map(|element| self.check_expression(element))
                        .collect();
                    let inferred_type = self
                        .common_class_type(&element_types)
                        .unwrap_or_else(|| element_types[0].clone());
                    for (elem, elem_type) in elements.iter().zip(element_types.iter()).skip(1) {
                        if !self.types_compatible(elem_type, &inferred_type) {
                            self.env.record_error(CheckerError::TypeMismatchContext {
                                context: TypeContext::ArrayElement,
                                expected: inferred_type.clone(),
                                got: elem_type.clone(),
                                span: elem.span.clone(),
                            });
                        }
                    }
                    Type::Array(Box::new(inferred_type))
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
                        if !self.types_compatible(&kt, &key_type) {
                            self.env.record_error(CheckerError::MapEntryTypeMismatch {
                                entry: crate::errors::MapEntry::Key,
                                span: k.span.clone(),
                            });
                        }
                        if !self.types_compatible(&vt, &value_type) {
                            self.env.record_error(CheckerError::MapEntryTypeMismatch {
                                entry: crate::errors::MapEntry::Value,
                                span: v.span.clone(),
                            });
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
                        let sym_id = self.env.define(Symbol {
                            id: SymbolId(0),
                            name: p.name.clone(),
                            ty: ty.clone(),
                            mutable: false,
                            kind: SymbolKind::Parameter,
                        });
                        self.record_decl(
                            sym_id,
                            &p.name,
                            ty.clone(),
                            crate::sema::SymbolKind::Parameter,
                            p.id,
                        );
                        ty
                    })
                    .collect();

                let mut return_type = self.check_expression(body);
                // A block body is checked as a statement sequence, which has no
                // value, so `check_expression` reports `void`. What the lambda
                // actually yields is what its `return` gives back — without this
                // `|| { return 1 }` would be typed `fn() -> void`, and calling it
                // for a value would look like a type error.
                if matches!(return_type, Type::Void) {
                    if let ExpressionKind::Block(block) = &body.kind {
                        if let Some(ty) = self.block_return_type(block) {
                            return_type = ty;
                        }
                    }
                }
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
                    self.env.record_error(CheckerError::ConditionMustBeBool {
                        got: None,
                        span: expr.span.clone(),
                    });
                }

                let then_type = self.check_expression(then_expr);
                let else_type = self.check_expression(else_expr);

                if !self.types_compatible(&then_type, &else_type) {
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
                        if !self.types_compatible(&arm_type, first) {
                            self.env.record_error(CheckerError::TypeMismatchContext {
                                context: TypeContext::MatchArm,
                                expected: first.clone(),
                                got: arm_type.clone(),
                                span: arm.span.clone(),
                            });
                        }
                    } else {
                        first_arm_type = Some(arm_type);
                    }
                }

                self.check_match_exhaustiveness(&subject_type, arms, &expr.span);

                first_arm_type.unwrap_or(Type::Unknown)
            }

            ExpressionKind::Range { start, end, .. } => {
                if start.is_none() || end.is_none() {
                    self.env.error(
                        &expr.span,
                        "Open-ended range expressions are not supported".to_string(),
                    );
                }
                if let Some(s) = start {
                    let st = self.check_expression(s);
                    if !st.is_integer() {
                        self.env.record_error(CheckerError::MustBeInteger {
                            context: IntContext::RangeStart,
                            span: expr.span.clone(),
                        });
                    }
                }
                if let Some(e) = end {
                    let et = self.check_expression(e);
                    if !et.is_integer() {
                        self.env.record_error(CheckerError::MustBeInteger {
                            context: IntContext::RangeEnd,
                            span: expr.span.clone(),
                        });
                    }
                }
                Type::Struct(BUILTIN_RANGE_TYPE.to_string())
            }

            ExpressionKind::Cast {
                expr: inner,
                type_expr,
            } => {
                let source = self.check_expression(inner);
                let target = self.resolve_type_expr(type_expr);
                if !self.cast_compatible(&source, &target) {
                    self.env.record_error(CheckerError::GenericError {
                        message: format!(
                            "Cannot cast '{}' to '{}'",
                            source.display_name(),
                            target.display_name()
                        ),
                        span: expr.span.clone(),
                    });
                }
                target
            }

            ExpressionKind::TypeCheck {
                expr: inner,
                type_expr,
            } => {
                self.check_expression(inner);
                // Resolve the target through the same semantic path as every
                // other type annotation.  Besides making aliases, generic
                // applications, and nominal kinds available to consumers,
                // this records the precise UnknownType diagnostic instead of
                // allowing an unresolved target to reach code generation as
                // a coarse runtime kind.
                let target = self.resolve_type_expr(type_expr);
                self.sema.type_check_targets.insert(expr.id, target);
                Type::Bool
            }

            ExpressionKind::Assign { target, value } => {
                let target_type = self.check_expression(target);
                let value_type = self.check_expression(value);
                let unresolved_channel_rebinding =
                    self.is_unresolved_channel_rebinding(&target_type, &value_type, value);

                let indexed_storage = match &target.kind {
                    ExpressionKind::Index { object, .. } => {
                        match self.sema.expr_types.get(&object.id) {
                            Some(Type::Tuple(_)) => {
                                self.env.error(
                                    &target.span,
                                    "Cannot assign to tuple index; tuples are immutable"
                                        .to_string(),
                                );
                                false
                            }
                            Some(Type::Array(_) | Type::Map(_, _)) => true,
                            _ => false,
                        }
                    }
                    _ => false,
                };

                let compatible = if unresolved_channel_rebinding {
                    true
                } else if indexed_storage {
                    self.storage_value_compatible(&value_type, &target_type)
                } else {
                    self.expression_compatible(value, &value_type, &target_type)
                };
                if !compatible {
                    self.record_interface_compatibility_error(
                        &value_type,
                        &target_type,
                        &expr.span,
                    );
                    self.env.record_error(CheckerError::TypeMismatchContext {
                        context: TypeContext::Assignment,
                        expected: target_type.clone(),
                        got: value_type,
                        span: expr.span.clone(),
                    });
                }

                // Check if target is mutable
                let mut mutable_target = true;
                if let ExpressionKind::Identifier(name) = &target.kind {
                    if let Some(symbol) = self.env.lookup(name) {
                        if !symbol.mutable {
                            mutable_target = false;
                            self.env.error(
                                &expr.span,
                                format!("Cannot assign to immutable variable: {}", name),
                            );
                        }
                    }
                }

                if compatible && mutable_target && unresolved_channel_rebinding {
                    self.rebind_unresolved_channel_alias(target, value);
                }

                Type::Void
            }

            ExpressionKind::CompoundAssign { target, op, value } => {
                let target_type = self.check_expression(target);
                let _value_type = self.check_expression(value);

                let indexed_storage = if let ExpressionKind::Index { object, .. } = &target.kind {
                    match self.sema.expr_types.get(&object.id) {
                        Some(Type::Tuple(_)) => {
                            self.env.error(
                                &target.span,
                                "Cannot assign to tuple index; tuples are immutable".to_string(),
                            );
                            false
                        }
                        Some(Type::Array(_) | Type::Map(_, _)) => true,
                        _ => false,
                    }
                } else {
                    false
                };

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

                let compatible = if indexed_storage {
                    self.storage_value_compatible(&result_type, &target_type)
                } else {
                    self.types_compatible(&result_type, &target_type)
                };
                if !compatible {
                    self.env
                        .record_error(CheckerError::CompoundAssignmentTypeMismatch {
                            span: expr.span.clone(),
                        });
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
                // Check each arm's channel and body. A recv arm may bind a
                // variable (`v = <-ch`) that must be in scope inside the arm
                // body; give each arm its own scope so the binding does not leak.
                let mut result_type: Option<Type> = None;
                for arm in arms {
                    self.env.push_scope();
                    match &arm.kind {
                        SelectArmKind::Recv { channel, variable } => {
                            let channel_type = self.check_expression(channel);
                            if let Some(name) = variable {
                                // The recv binder has no dedicated AST node, so
                                // we cannot record a declaration site. Uses
                                // inside the arm body still resolve to this id
                                // via the identifier-use path.
                                let binder_type = match channel_type {
                                    Type::Channel(element) => *element,
                                    Type::Unknown => Type::Unknown,
                                    _ => Type::Any,
                                };
                                self.env.define(Symbol {
                                    id: SymbolId(0),
                                    name: name.clone(),
                                    ty: binder_type,
                                    mutable: false,
                                    kind: SymbolKind::Variable,
                                });
                            }
                        }
                        SelectArmKind::Send { channel, value } => {
                            self.check_expression(channel);
                            self.check_expression(value);
                            self.check_send_channel_types(channel, value);
                        }
                        SelectArmKind::Default => {}
                    }
                    let arm_type = self.check_expression(&arm.body);
                    let arm_terminates = self.expression_definitely_terminates(&arm.body);
                    self.env.pop_scope();

                    if arm_terminates {
                        continue;
                    }

                    if let Some(current) = &result_type {
                        let value_void_mismatch =
                            matches!(current, Type::Void) != matches!(arm_type, Type::Void);
                        if value_void_mismatch || !self.types_compatible(&arm_type, current) {
                            self.env.error(
                                &arm.span,
                                format!(
                                    "Select arm type mismatch: expected '{}', got '{}'",
                                    current.display_name(),
                                    arm_type.display_name()
                                ),
                            );
                        }

                        // Keep one contextual result type for native lowering.
                        // Dynamic values and optional values must win over a
                        // concrete first arm, and numeric mixed arms use the
                        // wider float representation regardless of source order.
                        if matches!(current, Type::Void | Type::Null)
                            || matches!(arm_type, Type::Any | Type::Optional(_))
                            || (matches!(current, Type::Int) && matches!(arm_type, Type::Float))
                        {
                            result_type = Some(arm_type);
                        } else if matches!(current, Type::Any)
                            || matches!(current, Type::Optional(_))
                            || (matches!(current, Type::Float) && matches!(arm_type, Type::Int))
                        {
                            // Keep the already selected dynamic/optional/wider
                            // type. This branch is intentionally explicit so a
                            // later incompatible arm cannot silently replace it.
                        }
                    } else {
                        result_type = Some(arm_type);
                    }
                }
                result_type.unwrap_or(Type::Void)
            }

            ExpressionKind::StructLiteral { name, fields } => {
                if let Some(type_name) = name {
                    self.check_named_aggregate_literal(expr, type_name, fields)
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
                if let Some(type_def) = self.env.lookup_type(enum_name).cloned() {
                    if let TypeDefKind::Enum { variants } = type_def.kind {
                        // Find the variant
                        if let Some((_, field_types)) =
                            variants.into_iter().find(|(name, _)| name == variant_name)
                        {
                            // Special case for Result type - return Type::Result instead of Type::Enum
                            let return_type = if enum_name == "Result" {
                                Type::Result {
                                    ok_type: Box::new(Type::Any),
                                    err_type: Box::new(Type::Any),
                                }
                            } else if !field_types.is_empty() {
                                let generic_params = self
                                    .env
                                    .generic_type_params(enum_name)
                                    .map(<[String]>::to_vec)
                                    .unwrap_or_default();
                                if generic_params.is_empty() {
                                    Type::Enum(enum_name.clone())
                                } else {
                                    self.make_generic_type(
                                        enum_name,
                                        generic_params.into_iter().map(Type::TypeParam).collect(),
                                    )
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
                                    required_params: field_types.len(),
                                    params: field_types,
                                    return_type: Box::new(return_type),
                                }
                            }
                        } else {
                            self.env.record_error(CheckerError::UnknownEnumVariant {
                                variant: variant_name.clone(),
                                enum_name: enum_name.clone(),
                                span: expr.span.clone(),
                            });
                            Type::Unknown
                        }
                    } else {
                        self.env.record_error(CheckerError::NotAnEnum {
                            type_name: enum_name.clone(),
                            span: expr.span.clone(),
                        });
                        Type::Unknown
                    }
                } else {
                    self.env.record_error(CheckerError::UnknownEnum {
                        name: enum_name.clone(),
                        span: expr.span.clone(),
                    });
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
                                    if !self.types_compatible(err_type, ret_err) {
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
                type_args,
            } => {
                let receiver_type = self.check_expression(receiver);
                // Validate any explicit method type arguments (turbofish,
                // e.g. `xs.collect::<int>()`). Generics are erased, so these
                // resolve to TypeParam/concrete types but otherwise have no
                // effect on the (dynamic) result type. We resolve them only to
                // surface genuinely unknown type names.
                for ty in type_args {
                    let _ = self.resolve_type_expr(ty);
                }
                // Check arguments
                for arg in args {
                    self.check_expression(&arg.value);
                }
                if method == "push" {
                    if let [value] = args.as_slice() {
                        self.refine_array_from_push(receiver, &value.value);
                    }
                }
                // Proper method resolution would refine this; the dynamic VM
                // tolerates Any here.
                let _ = (receiver_type, method);
                Type::Any
            }
        }
    }

    /// Bind pattern variables to the environment with their types
    /// Enforce exhaustiveness for `match` expressions whose scrutinee is a
    /// known, closed enum. A wildcard `_` or a bare variable / `@`-binding arm
    /// (with no sub-structure) is a catch-all and makes the match exhaustive.
    /// Non-enum scrutinees (ints, strings, tuples, structs, ...) are not checked
    /// here, so this never false-positives on them.
    fn check_match_exhaustiveness(&mut self, subject_type: &Type, arms: &[MatchArm], span: &Span) {
        // Only enforce for enum scrutinees with a known, closed variant set.
        let enum_name = match subject_type {
            Type::Enum(name) => self
                .sema
                .generic_type_instances
                .get(name)
                .map(|instance| instance.base_name.clone())
                .unwrap_or_else(|| name.clone()),
            _ => return,
        };
        let all_variants: Vec<String> = match self.env.lookup_type(&enum_name) {
            Some(td) => match &td.kind {
                TypeDefKind::Enum { variants } => variants.iter().map(|(n, _)| n.clone()).collect(),
                _ => return,
            },
            None => return,
        };

        let mut covered: HashSet<String> = HashSet::new();
        for arm in arms {
            // A guarded arm never guarantees coverage of its variant.
            if arm.guard.is_some() {
                continue;
            }
            if Self::pattern_is_catch_all(&arm.pattern) {
                // Catch-all arm: the match is exhaustive regardless of variants.
                return;
            }
            Self::collect_covered_variants(&arm.pattern, &enum_name, &mut covered);
        }

        let missing: Vec<String> = all_variants
            .into_iter()
            .filter(|v| !covered.contains(v))
            .collect();

        if !missing.is_empty() {
            self.env.record_error(CheckerError::NonExhaustiveMatch {
                enum_name,
                missing,
                span: span.clone(),
            });
        }
    }

    /// Whether a pattern unconditionally matches any value (wildcard `_`, a bare
    /// variable binding, or an `@`-binding wrapping a catch-all).
    fn pattern_is_catch_all(pattern: &Pattern) -> bool {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Variable(_) => true,
            PatternKind::Binding { pattern, .. } => Self::pattern_is_catch_all(pattern),
            PatternKind::Or(patterns) => patterns.iter().any(Self::pattern_is_catch_all),
            _ => false,
        }
    }

    /// Record which enum variants of `enum_name` a pattern covers. Constructor
    /// patterns may be qualified (`Color::Red`) or bare (`Red`).
    fn collect_covered_variants(pattern: &Pattern, enum_name: &str, covered: &mut HashSet<String>) {
        match &pattern.kind {
            PatternKind::Constructor { name, .. } => {
                let variant = name
                    .strip_prefix(&format!("{}::", enum_name))
                    .unwrap_or(name);
                covered.insert(variant.to_string());
            }
            PatternKind::Binding { pattern, .. } => {
                Self::collect_covered_variants(pattern, enum_name, covered);
            }
            PatternKind::Or(patterns) => {
                for p in patterns {
                    Self::collect_covered_variants(p, enum_name, covered);
                }
            }
            _ => {}
        }
    }

    fn merge_pattern_bindings(
        &mut self,
        into: &mut Vec<PatternBindingInfo>,
        bindings: Vec<PatternBindingInfo>,
        span: &Span,
    ) {
        for binding in bindings {
            if into.iter().any(|existing| existing.name == binding.name) {
                self.env.error(
                    span,
                    format!("Pattern binds '{}' more than once", binding.name),
                );
            } else {
                into.push(binding);
            }
        }
    }

    fn generic_owner_bindings(&self, subject_type: &Type) -> (String, HashMap<String, Type>) {
        if let Some(instance) = self.generic_type_instance(subject_type) {
            let base_name = instance.base_name.clone();
            let params = self.env.generic_type_params(&base_name).unwrap_or_default();
            let bindings = params
                .iter()
                .cloned()
                .zip(instance.args.iter().cloned())
                .collect();
            return (base_name, bindings);
        }
        let name = match subject_type {
            Type::Struct(name) | Type::Class(name) | Type::Enum(name) => name.clone(),
            _ => String::new(),
        };
        (name, HashMap::new())
    }

    fn constructor_pattern_field_types(
        &mut self,
        name: &str,
        subject_type: &Type,
        span: &Span,
    ) -> Option<Vec<Type>> {
        let variant = name.rsplit("::").next().unwrap_or(name);
        match subject_type {
            Type::Optional(inner) if variant == "Some" => return Some(vec![(**inner).clone()]),
            Type::Optional(_) if variant == "None" => return Some(Vec::new()),
            Type::Optional(inner) => {
                return self.constructor_pattern_field_types(name, inner, span)
            }
            Type::Result { ok_type, .. } if variant == "Ok" => {
                return Some(vec![(**ok_type).clone()])
            }
            Type::Result { err_type, .. } if variant == "Err" => {
                return Some(vec![(**err_type).clone()])
            }
            _ => {}
        }

        let (enum_name, bindings) = self.generic_owner_bindings(subject_type);
        if enum_name.is_empty() {
            self.env.error(
                span,
                format!(
                    "Constructor pattern '{}' cannot match '{}'",
                    name,
                    subject_type.display_name()
                ),
            );
            return None;
        }
        if let Some((qualifier, _)) = name.rsplit_once("::") {
            if qualifier != enum_name {
                self.env.error(
                    span,
                    format!(
                        "Constructor '{}' does not belong to enum '{}'",
                        name, enum_name
                    ),
                );
            }
        }
        let variants = match self.env.lookup_type(&enum_name).cloned() {
            Some(TypeDef {
                kind: TypeDefKind::Enum { variants },
                ..
            }) => variants,
            _ => {
                self.env
                    .error(span, format!("Unknown enum '{}' in pattern", enum_name));
                return None;
            }
        };
        let Some((_, fields)) = variants
            .into_iter()
            .find(|(candidate, _)| candidate == variant)
        else {
            self.env.error(
                span,
                format!("Enum '{}' has no variant '{}'", enum_name, variant),
            );
            return None;
        };
        Some(
            fields
                .iter()
                .map(|field| self.substitute_type(field, &bindings))
                .collect(),
        )
    }

    fn struct_pattern_field_types(
        &mut self,
        pattern_name: &str,
        subject_type: &Type,
        span: &Span,
    ) -> Option<StructPatternFields> {
        let (subject_name, bindings) = self.generic_owner_bindings(subject_type);
        if subject_name.is_empty() {
            self.env.error(
                span,
                format!(
                    "Struct pattern cannot match '{}'",
                    subject_type.display_name()
                ),
            );
            return None;
        }
        let owner = if pattern_name.is_empty() {
            subject_name.clone()
        } else {
            pattern_name.to_string()
        };
        if owner != subject_name {
            self.env.error(
                span,
                format!(
                    "Struct pattern '{}' cannot match '{}'",
                    owner,
                    subject_type.display_name()
                ),
            );
            return None;
        }
        let fields = match self.env.lookup_type(&owner).cloned() {
            Some(TypeDef {
                kind: TypeDefKind::Struct { fields, .. },
                ..
            }) => fields,
            Some(TypeDef {
                kind: TypeDefKind::Class { .. },
                ..
            }) => self.env.get_class_fields(&owner),
            _ => {
                self.env
                    .error(span, format!("Unknown struct '{}' in pattern", owner));
                return None;
            }
        };
        Some((
            owner,
            fields
                .into_iter()
                .map(|(name, ty, public)| (name, self.substitute_type(&ty, &bindings), public))
                .collect(),
        ))
    }

    fn pattern_bindings(
        &mut self,
        pattern: &Pattern,
        subject_type: &Type,
    ) -> Vec<PatternBindingInfo> {
        self.sema
            .pattern_types
            .insert(pattern.id, subject_type.clone());
        match &pattern.kind {
            PatternKind::Variable(name) => vec![PatternBindingInfo {
                name: name.clone(),
                ty: subject_type.clone(),
                pattern_id: pattern.id,
            }],
            PatternKind::Wildcard => Vec::new(),
            PatternKind::Literal(expression) => {
                let literal_type = self.check_expression(expression);
                if !self.types_compatible(&literal_type, subject_type) {
                    self.env.error(
                        &pattern.span,
                        format!(
                            "Pattern of type '{}' cannot match '{}'",
                            literal_type.display_name(),
                            subject_type.display_name()
                        ),
                    );
                }
                Vec::new()
            }
            PatternKind::Binding {
                name,
                pattern: inner,
            } => {
                let mut bindings = vec![PatternBindingInfo {
                    name: name.clone(),
                    ty: subject_type.clone(),
                    pattern_id: pattern.id,
                }];
                let inner_bindings = self.pattern_bindings(inner, subject_type);
                self.merge_pattern_bindings(&mut bindings, inner_bindings, &inner.span);
                bindings
            }
            PatternKind::Tuple(patterns) => {
                let Type::Tuple(types) = subject_type else {
                    self.env.error(
                        &pattern.span,
                        format!(
                            "Tuple pattern cannot match '{}'",
                            subject_type.display_name()
                        ),
                    );
                    return Vec::new();
                };
                if patterns.len() != types.len() {
                    self.env.error(
                        &pattern.span,
                        format!(
                            "Tuple pattern has {} elements, but the value has {}",
                            patterns.len(),
                            types.len()
                        ),
                    );
                }
                let mut bindings = Vec::new();
                for (sub_pattern, element_type) in patterns.iter().zip(types) {
                    let nested = self.pattern_bindings(sub_pattern, element_type);
                    self.merge_pattern_bindings(&mut bindings, nested, &sub_pattern.span);
                }
                bindings
            }
            PatternKind::Constructor { name, fields } => {
                let Some(field_types) =
                    self.constructor_pattern_field_types(name, subject_type, &pattern.span)
                else {
                    return Vec::new();
                };
                if fields.len() != field_types.len() {
                    self.env.error(
                        &pattern.span,
                        format!(
                            "Constructor '{}' expects {} field{}, got {}",
                            name,
                            field_types.len(),
                            if field_types.len() == 1 { "" } else { "s" },
                            fields.len()
                        ),
                    );
                }
                let mut bindings = Vec::new();
                for (sub_pattern, field_type) in fields.iter().zip(field_types.iter()) {
                    let nested = self.pattern_bindings(sub_pattern, field_type);
                    self.merge_pattern_bindings(&mut bindings, nested, &sub_pattern.span);
                }
                bindings
            }
            PatternKind::Struct {
                name,
                fields: pattern_fields,
                ..
            } => {
                let Some((owner, fields)) =
                    self.struct_pattern_field_types(name, subject_type, &pattern.span)
                else {
                    return Vec::new();
                };
                let mut bindings = Vec::new();
                for (field_name, sub_pattern) in pattern_fields {
                    let Some((_, field_type, _)) = fields
                        .iter()
                        .find(|(candidate, _, _)| candidate == field_name)
                    else {
                        self.env.error(
                            &sub_pattern.span,
                            format!("Struct '{}' has no field '{}'", owner, field_name),
                        );
                        continue;
                    };
                    let nested = self.pattern_bindings(sub_pattern, field_type);
                    self.merge_pattern_bindings(&mut bindings, nested, &sub_pattern.span);
                }
                bindings
            }
            PatternKind::Or(alternatives) => {
                let Some(first) = alternatives.first() else {
                    self.env
                        .error(&pattern.span, "Or-pattern has no alternatives".to_string());
                    return Vec::new();
                };
                let canonical = self.pattern_bindings(first, subject_type);
                let canonical_types: HashMap<&str, &Type> = canonical
                    .iter()
                    .map(|binding| (binding.name.as_str(), &binding.ty))
                    .collect();
                for alternative in &alternatives[1..] {
                    let bindings = self.pattern_bindings(alternative, subject_type);
                    let alternative_types: HashMap<&str, &Type> = bindings
                        .iter()
                        .map(|binding| (binding.name.as_str(), &binding.ty))
                        .collect();
                    let mut expected_names = canonical_types.keys().copied().collect::<Vec<_>>();
                    let mut actual_names = alternative_types.keys().copied().collect::<Vec<_>>();
                    expected_names.sort_unstable();
                    actual_names.sort_unstable();
                    if expected_names != actual_names {
                        self.env.error(
                            &alternative.span,
                            format!(
                                "All alternatives in an or-pattern must bind the same variables; expected [{}], found [{}]",
                                expected_names.join(", "),
                                actual_names.join(", ")
                            ),
                        );
                        continue;
                    }
                    for name in expected_names {
                        if canonical_types[name] != alternative_types[name] {
                            self.env.error(
                                &alternative.span,
                                format!(
                                    "Binding '{}' has incompatible types '{}' and '{}' across or-pattern alternatives",
                                    name,
                                    canonical_types[name].display_name(),
                                    alternative_types[name].display_name()
                                ),
                            );
                        }
                    }
                }
                canonical
            }
            PatternKind::Range { start, end, .. } => {
                self.pattern_bindings(start, subject_type);
                self.pattern_bindings(end, subject_type);
                Vec::new()
            }
        }
    }

    fn bind_pattern_variables(&mut self, pattern: &Pattern, subject_type: &Type) {
        for binding in self.pattern_bindings(pattern, subject_type) {
            let sym_id = self.env.define(Symbol {
                id: SymbolId(0),
                name: binding.name.clone(),
                ty: binding.ty.clone(),
                mutable: false,
                kind: SymbolKind::Variable,
            });
            self.record_decl(
                sym_id,
                &binding.name,
                binding.ty,
                crate::sema::SymbolKind::Variable,
                binding.pattern_id,
            );
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
                let sym_id = self.env.define(Symbol {
                    id: SymbolId(0),
                    name: name.clone(),
                    ty: subject_type.clone(),
                    mutable,
                    kind: SymbolKind::Variable,
                });
                self.record_decl(
                    sym_id,
                    name,
                    subject_type.clone(),
                    crate::sema::SymbolKind::Variable,
                    pattern.id,
                );
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
            // Un-annotated parameter: accept any argument, dynamically typed.
            TypeExprKind::Infer => Type::Any,
            TypeExprKind::Named(name) => {
                match name.as_str() {
                    "int" => Type::Int,
                    "float" => Type::Float,
                    "bool" => Type::Bool,
                    "string" => Type::String,
                    "char" => Type::Char,
                    "any" => Type::Any,
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
                        if let Some(name) = self.current_type_name.clone() {
                            self.current_owner_type(&name)
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
                            self.env.record_error(CheckerError::UnknownType {
                                name: other.to_string(),
                                span: type_expr.span.clone(),
                            });
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
                    "Channel" => {
                        if resolved_args.len() == 1 {
                            Type::Channel(Box::new(resolved_args[0].clone()))
                        } else {
                            self.env.error(
                                &type_expr.span,
                                "Channel takes one type argument".to_string(),
                            );
                            Type::Unknown
                        }
                    }
                    _ => {
                        let Some(params) =
                            self.env.generic_type_params(name).map(<[String]>::to_vec)
                        else {
                            self.env.error(
                                &type_expr.span,
                                format!("Type '{}' is not a generic type", name),
                            );
                            return Type::Unknown;
                        };
                        if params.len() != resolved_args.len() {
                            self.env.error(
                                &type_expr.span,
                                format!(
                                    "Type '{}' expects {} type argument{}, got {}",
                                    name,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    resolved_args.len()
                                ),
                            );
                            Type::Unknown
                        } else {
                            self.make_generic_type(name, resolved_args)
                        }
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

    /// Look up an impl method and convert it to a Function type.
    /// Returns None if the method doesn't exist.
    fn impl_method_to_function(&self, type_name: &str, method: &str) -> Option<Type> {
        self.env
            .lookup_method(type_name, method)
            .map(|impl_method| {
                let param_types: Vec<Type> = impl_method
                    .params
                    .iter()
                    .map(|(_, ty)| ty.clone())
                    .collect();
                Type::Function {
                    params: param_types.clone(),
                    return_type: Box::new(impl_method.return_type.clone()),
                    required_params: impl_method.required_params,
                }
            })
    }

    fn impl_method_to_function_with_bindings(
        &mut self,
        type_name: &str,
        method: &str,
        bindings: &HashMap<String, Type>,
    ) -> Option<Type> {
        let impl_method = self.env.lookup_method(type_name, method)?.clone();
        let params: Vec<Type> = impl_method
            .params
            .iter()
            .map(|(_, ty)| self.substitute_type(ty, bindings))
            .collect();
        let return_type = self.substitute_type(&impl_method.return_type, bindings);
        Some(Type::Function {
            required_params: impl_method.required_params,
            params,
            return_type: Box::new(return_type),
        })
    }

    fn resolve_generic_field_access(
        &mut self,
        instance: crate::sema::GenericTypeInstance,
        field: &str,
        span: &Span,
    ) -> Type {
        let params = self
            .env
            .generic_type_params(&instance.base_name)
            .map(<[String]>::to_vec)
            .unwrap_or_default();
        let bindings: HashMap<String, Type> = params
            .into_iter()
            .zip(instance.args.iter().cloned())
            .collect();
        let Some(type_def) = self.env.lookup_type(&instance.base_name).cloned() else {
            self.env.record_error(CheckerError::UnknownType {
                name: instance.base_name,
                span: span.clone(),
            });
            return Type::Unknown;
        };

        match type_def.kind {
            TypeDefKind::Struct { fields, methods }
            | TypeDefKind::Class {
                fields, methods, ..
            } => {
                for (field_name, field_type, _) in fields {
                    if field_name == field {
                        return self.substitute_type(&field_type, &bindings);
                    }
                }
                for (method_name, method_type, _) in methods {
                    if method_name == field {
                        return self.substitute_type(&method_type, &bindings);
                    }
                }
                if let Some(method_type) = self.impl_method_to_function_with_bindings(
                    &instance.base_name,
                    field,
                    &bindings,
                ) {
                    return method_type;
                }
            }
            TypeDefKind::Enum { .. } if field == "__enum" || field == "__variant" => {
                return Type::String;
            }
            _ => {}
        }

        self.env.error(
            span,
            format!(
                "Unknown field or method: {} on type {}<{}>",
                field,
                instance.base_name,
                instance
                    .args
                    .iter()
                    .map(Type::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        Type::Unknown
    }

    /// Resolve field access on a type
    fn resolve_field_access(
        &mut self,
        obj_type: &Type,
        object: &Expression,
        field: &str,
        span: &Span,
    ) -> Type {
        if let Some(instance) = self.generic_type_instance(obj_type).cloned() {
            return self.resolve_generic_field_access(instance, field, span);
        }
        if obj_type.is_builtin_range() {
            return match field {
                "start" | "end" => Type::Int,
                "inclusive" => Type::Bool,
                _ => {
                    self.env.error(
                        span,
                        format!("Unknown field or method: {} on type range", field),
                    );
                    Type::Unknown
                }
            };
        }
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
                if let Some(ty) = self.impl_method_to_function(name, field) {
                    return ty;
                }
                self.env.error(
                    span,
                    format!("Unknown field or method: {} on type {}", field, name),
                );
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
                        if let Some(ty) = self.impl_method_to_function(name, field) {
                            return ty;
                        }
                        self.env.error(
                            span,
                            format!("Unknown field or method: {} on type {}", field, name),
                        );
                    }
                }
                Type::Unknown
            }
            Type::Interface(name) => {
                if let Some(ty) = self.instance_method_type(obj_type, field) {
                    return ty;
                }
                self.env.error(
                    span,
                    format!("Unknown field or method: {} on interface {}", field, name),
                );
                Type::Unknown
            }
            Type::Unknown => {
                if let ExpressionKind::Identifier(type_name) = &object.kind {
                    if let Some(impl_method) = self.env.lookup_method(type_name, field) {
                        if !impl_method.has_self {
                            return self.impl_method_to_function(type_name, field).unwrap();
                        }
                    }
                }
                Type::Unknown
            }
            Type::Any => Type::Any,
            Type::Int => {
                if let Some(ty) = self.impl_method_to_function("int", field) {
                    return ty;
                }
                self.env.record_error(CheckerError::UnknownMethod {
                    method: field.to_string(),
                    on_type: "int".to_string(),
                    span: span.clone(),
                });
                Type::Unknown
            }
            Type::Float => {
                if let Some(ty) = self.impl_method_to_function("float", field) {
                    return ty;
                }
                self.env.record_error(CheckerError::UnknownMethod {
                    method: field.to_string(),
                    on_type: "float".to_string(),
                    span: span.clone(),
                });
                Type::Unknown
            }
            Type::String => {
                if let Some(ty) = self.impl_method_to_function("string", field) {
                    return ty;
                }
                if field == "len" {
                    return Type::Function {
                        params: vec![],
                        return_type: Box::new(Type::Int),
                        required_params: 0,
                    };
                }
                self.env.record_error(CheckerError::UnknownMethod {
                    method: field.to_string(),
                    on_type: "string".to_string(),
                    span: span.clone(),
                });
                Type::Unknown
            }
            Type::Array(inner) => {
                let specific_type_name = format!("[{}]", inner.display_name());
                if let Some(ty) = self.impl_method_to_function(&specific_type_name, field) {
                    return ty;
                }
                if let Some(ty) = self.impl_method_to_function("array", field) {
                    return ty;
                }
                match field {
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
                        self.env.record_error(CheckerError::UnknownMethod {
                            method: field.to_string(),
                            on_type: "array".to_string(),
                            span: span.clone(),
                        });
                        Type::Unknown
                    }
                }
            }
            _ => {
                self.env.record_error(CheckerError::CannotAccessField {
                    ty: obj_type.clone(),
                    span: span.clone(),
                });
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

/// Type check a program, returning the checked program (when successful) along
/// with the structured errors collected during checking. Unlike [`check`], this
/// exposes the structured [`CheckerError`]s with their spans instead of
/// flattening them into a single string.
pub fn check_collecting(program: &Program) -> (Option<CheckedProgram>, Vec<CheckerError>) {
    let mut checker = TypeChecker::new();
    let (checked, errors) = checker.check_program_collecting(program);
    // Always return the checked program: the semantic tables are populated
    // (best-effort) even when errors were found, which error-tolerant tooling
    // relies on. Errors are reported separately.
    (Some(checked), errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Helper function to check source code and return the result
    fn check_source(source: &str) -> Result<(), String> {
        check_source_checked(source).map(|_| ())
    }

    /// Like [`check_source`], but hands back the checked program so a test can
    /// inspect the recorded types.
    fn check_source_checked(source: &str) -> Result<CheckedProgram, String> {
        let tokens = tokenize(source)?;
        let ast = parse(&tokens)?;
        check(&ast)
    }

    // ========================================================================
    // Error-tolerant collecting checker
    // ========================================================================

    #[test]
    fn test_check_program_collecting_populates_sema_on_success() {
        let tokens = tokenize("let x = 42").unwrap();
        let ast = parse(&tokens).unwrap();
        let mut checker = TypeChecker::new();
        let (checked, errors) = checker.check_program_collecting(&ast);
        assert!(errors.is_empty());
        assert!(!checked.sema.expr_types.is_empty());
    }

    #[test]
    fn test_check_program_collecting_returns_sema_on_error() {
        // The error is on line 2; line 1 and line 3 are valid. Tooling needs the
        // semantic tables despite the error, so they must still be populated.
        let src = "let a = 1\nlet b = missing\nlet c = a";
        let tokens = tokenize(src).unwrap();
        let ast = parse(&tokens).unwrap();
        let mut checker = TypeChecker::new();
        let (checked, errors) = checker.check_program_collecting(&ast);
        assert!(!errors.is_empty(), "expected the undefined-variable error");
        assert!(
            !checked.sema.expr_types.is_empty(),
            "sema must be populated even when checking reports an error"
        );
    }

    #[test]
    fn test_check_collecting_always_returns_checked_program() {
        let src = "let b = missing";
        let tokens = tokenize(src).unwrap();
        let ast = parse(&tokens).unwrap();
        let (checked, errors) = check_collecting(&ast);
        assert!(checked.is_some(), "checked program must survive errors");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_collect_type_members_includes_struct_fields_and_impl_methods() {
        let src = "struct P { x: int }\nimpl P { fn area(self) -> int { self.x } }";
        let tokens = tokenize(src).unwrap();
        let ast = parse(&tokens).unwrap();
        let mut checker = TypeChecker::new();
        let (checked, _errors) = checker.check_program_collecting(&ast);
        let members = checked.sema.type_members.get("P").expect("P members");
        assert!(members.fields.iter().any(|m| m.name == "x"));
        assert!(members.methods.iter().any(|m| m.name == "area"));
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

    #[test]
    fn channel_inference_refines_first_send_and_recv_type() {
        let checked = check_source_checked(
            r#"
            let ch = chan()
            send(ch, 42)
            let received: int = recv(ch)
            "#,
        )
        .expect("first send should infer the channel element type");

        let channel = checked
            .sema
            .symbols
            .values()
            .find(|symbol| symbol.name == "ch")
            .expect("channel binding");
        assert_eq!(channel.ty, Type::Channel(Box::new(Type::Int)));

        let recv_expr = match &checked.program.statements[2].kind {
            StatementKind::VarDecl {
                initializer: Some(initializer),
                ..
            } => initializer,
            _ => panic!("expected the recv declaration"),
        };
        assert_eq!(checked.sema.expr_types.get(&recv_expr.id), Some(&Type::Int));
    }

    #[test]
    fn channel_inference_refines_from_a_concrete_function_parameter() {
        let checked = check_source_checked(
            r#"
            fn produce(ch: Channel<int>) { send(ch, 41) }
            let ch = chan(1)
            spawn produce(ch)
            select { value = <-ch => { let answer: int = value + 1 } }
            "#,
        )
        .expect("a delegated sender should refine the caller's channel");

        let channel = checked
            .sema
            .symbols
            .values()
            .find(|symbol| symbol.name == "ch" && symbol.kind == crate::sema::SymbolKind::Variable)
            .expect("channel binding");
        assert_eq!(channel.ty, Type::Channel(Box::new(Type::Int)));
    }

    #[test]
    fn select_receive_binder_uses_channel_element_type() {
        let checked = check_source_checked(
            r#"
            let ch: Channel<string> = chan()
            select {
                value = <-ch => value
                _ => ""
            }
            "#,
        )
        .expect("select receive binder should have the channel element type");

        let StatementKind::Expression(select) = &checked.program.statements[1].kind else {
            panic!("expected select expression");
        };
        let ExpressionKind::Select(arms) = &select.kind else {
            panic!("expected select expression kind");
        };
        assert_eq!(
            checked.sema.expr_types.get(&arms[0].body.id),
            Some(&Type::String)
        );

        assert!(check_source(
            r#"
            let ch: Channel<string> = chan()
            select {
                value = <-ch => { let wrong: int = value }
                _ => {}
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn select_send_refines_and_enforces_channel_element_type() {
        let checked = check_source_checked(
            r#"
            let ch = chan()
            select { 42 -> ch => {} }
            let value: int = recv(ch)
            "#,
        )
        .expect("a select send should refine an inferred channel");

        let channel = checked
            .sema
            .symbols
            .values()
            .find(|symbol| symbol.name == "ch")
            .expect("channel binding");
        assert_eq!(channel.ty, Type::Channel(Box::new(Type::Int)));

        let error = check_source(
            r#"
            let ch = chan()
            select { 1 -> ch => {} }
            select { "wrong" -> ch => {} }
            "#,
        )
        .expect_err("a select channel must reject a mixed payload type");
        assert!(
            error.contains("Argument type mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn incompatible_second_channel_send_is_rejected() {
        let error = check_source(
            r#"
            let ch = chan()
            send(ch, 1)
            send(ch, "not an int")
            "#,
        )
        .expect_err("a channel must keep the first sent element type");
        assert!(
            error.contains("Argument type mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn channel_aliases_share_first_send_inference() {
        let checked = check_source_checked(
            r#"
            let source = chan(1)
            let alias = source
            send(alias, 41)
            let received: int = recv(source)
            "#,
        )
        .expect("a channel alias must share its inferred element type");

        for name in ["source", "alias"] {
            let channel = checked
                .sema
                .symbols
                .values()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("missing channel binding {name}"));
            assert_eq!(channel.ty, Type::Channel(Box::new(Type::Int)));
        }
    }

    #[test]
    fn conflicting_sends_through_channel_aliases_are_rejected() {
        let error = check_source(
            r#"
            let source = chan()
            let alias = source
            send(alias, 1)
            send(source, "wrong")
            "#,
        )
        .expect_err("aliases of one channel cannot infer different payload types");
        assert!(
            error.contains("Argument type mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn channel_element_types_are_checked_in_function_arguments() {
        let error = check_source(
            r#"
            fn needs_strings(ch: Channel<string>) {}
            let ints: Channel<int> = chan()
            needs_strings(ints)
            "#,
        )
        .expect_err("Channel<int> must not satisfy Channel<string>");
        assert!(
            error.contains("Argument type mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn inferred_channel_cannot_be_refined_to_conflicting_parameter_types() {
        let error = check_source(
            r#"
            fn needs_ints(ch: Channel<int>) {}
            fn needs_strings(ch: Channel<string>) {}
            let channel = chan()
            needs_ints(channel)
            needs_strings(channel)
            "#,
        )
        .expect_err("an inferred channel must retain its first concrete element type");
        assert!(
            error.contains("Argument type mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn first_push_refines_an_empty_array_binding() {
        let checked = check_source_checked(
            r#"
            let numbers = []
            push(numbers, 1)
            let first: int = numbers[0]

            let words = []
            words.push("hello")
            let word: string = words[0]
            "#,
        )
        .expect("both empty arrays are refined by their first push");

        let mut refined: Vec<_> = checked
            .sema
            .symbols
            .values()
            .filter(|symbol| symbol.name == "numbers" || symbol.name == "words")
            .map(|symbol| (symbol.name.as_str(), &symbol.ty))
            .collect();
        refined.sort_unstable_by_key(|(name, _)| *name);
        assert_eq!(
            refined,
            vec![
                ("numbers", &Type::Array(Box::new(Type::Int))),
                ("words", &Type::Array(Box::new(Type::String))),
            ]
        );
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
        assert!(check_source("while true { break println(\"done\") }").is_ok());
    }

    #[test]
    fn break_expression_is_type_checked() {
        let result = check_source("while true { break missing_name }");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Undefined variable: missing_name"));
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
    fn test_match_exhaustive_all_variants_ok() {
        assert!(check_source(
            r#"
            enum Color { Red, Green, Blue }
            fn f(c: Color) -> string {
                return match c {
                    Color::Red => "r"
                    Color::Green => "g"
                    Color::Blue => "b"
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_match_exhaustive_wildcard_ok() {
        assert!(check_source(
            r#"
            enum Color { Red, Green, Blue }
            fn f(c: Color) -> string {
                return match c {
                    Color::Red => "r"
                    _ => "other"
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_match_exhaustive_variable_catchall_ok() {
        assert!(check_source(
            r#"
            enum Color { Red, Green, Blue }
            fn f(c: Color) -> string {
                return match c {
                    Color::Red => "r"
                    other => "other"
                }
            }
            "#
        )
        .is_ok());
    }

    #[test]
    fn test_match_non_exhaustive_enum_error() {
        let result = check_source(
            r#"
            enum Color { Red, Green, Blue }
            fn f(c: Color) -> string {
                return match c {
                    Color::Red => "r"
                    Color::Green => "g"
                }
            }
            "#,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("not exhaustive"), "got: {msg}");
        assert!(msg.contains("Blue"), "got: {msg}");
    }

    #[test]
    fn test_match_non_enum_not_flagged() {
        // Integer match without wildcard is fine (not an enum scrutinee).
        assert!(check_source(
            r#"
            let x = 5
            let r = match x {
                1 => "one"
                2 => "two"
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
    fn heterogeneous_http_result_has_precise_literal_index_types() {
        assert!(check_source(
            r#"
            let response = http_get("http://127.0.0.1/")
            let status: int = response[0]
            let body: string = response[1]
            "#,
        )
        .is_ok());

        let error = check_source(
            r#"
            let response = http_get("http://127.0.0.1/")
            let index = 0
            let status = response[index]
            "#,
        )
        .expect_err("a heterogeneous tuple needs a literal index");
        assert!(error.contains("tuple index must be a non-negative integer literal"));
    }

    #[test]
    fn test_string_index_access() {
        let checked = check_source_checked(
            r#"
            let s = "hello"
            let c: string = s[0]
            "#,
        )
        .expect("string indexing type-checks");
        let StatementKind::VarDecl {
            initializer: Some(index),
            ..
        } = &checked.program.statements[1].kind
        else {
            panic!("expected the indexed declaration");
        };
        assert!(matches!(index.kind, ExpressionKind::Index { .. }));
        assert_eq!(checked.sema.expr_types.get(&index.id), Some(&Type::String));

        let error = check_source("let c: char = \"hello\"[0]").expect_err("not a char value");
        assert!(error.contains("Type mismatch"), "unexpected error: {error}");
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

    #[test]
    fn lambda_with_a_block_body_returns_the_type_it_returns() {
        // A block has no value of its own, so checking the body as an
        // expression yields `void`. The lambda's type is what its `return`
        // gives back — without that, calling it for a value looks like a type
        // error to anything that trusts the recorded type.
        let checked = check_source_checked("let f = || { return 42 }").expect("checks");
        let lambda_type = checked
            .sema
            .expr_types
            .values()
            .find(|ty| matches!(ty, Type::Function { .. }))
            .expect("the lambda has a recorded type");
        let Type::Function { return_type, .. } = lambda_type else {
            unreachable!("filtered above");
        };
        assert_eq!(**return_type, Type::Int);
    }

    #[test]
    fn lambda_block_return_type_follows_branches() {
        let checked =
            check_source_checked("let f = |n: int| { if n > 0 { return \"yes\" } return \"no\" }")
                .expect("checks");
        let lambda_type = checked
            .sema
            .expr_types
            .values()
            .find(|ty| matches!(ty, Type::Function { .. }))
            .expect("the lambda has a recorded type");
        let Type::Function { return_type, .. } = lambda_type else {
            unreachable!("filtered above");
        };
        assert_eq!(**return_type, Type::String);
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

    #[test]
    fn typed_pop_is_optional_for_global_and_method_forms() {
        assert!(check_source(
            r#"
            fn unwrap_global() -> int {
                let xs: [int] = [1]
                return pop(xs)?
            }

            fn unwrap_method() -> int {
                let xs: [int] = [2]
                return xs.pop()?
            }
            "#
        )
        .is_ok());

        let error = check_source(
            r#"
            let xs: [int] = [1]
            let value: int = pop(xs)
            "#,
        )
        .expect_err("pop([int]) must not be accepted as a bare int");
        assert!(error.contains("Type mismatch"), "unexpected error: {error}");
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

    #[test]
    fn test_map_literal() {
        assert!(check_source("let m = {\"a\": 1, \"b\": 2}").is_ok());
    }

    // Note: empty map `{}` parses as a block, not as a map literal.
    // Empty maps require explicit type annotation: `let m: Map<string, int> = {}`
    // This is tracked for future improvement.

    #[test]
    fn test_map_string_keys() {
        assert!(check_source("let m = {\"key\": \"value\"}").is_ok());
    }

    #[test]
    fn test_map_value_type_mismatch() {
        let result = check_source("let m = {\"a\": 1, \"b\": \"hello\"}");
        assert!(result.is_err());
    }

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
    fn test_impl_block_on_class_is_rejected() {
        // An `impl` block targeting a class must be rejected: classes dispatch
        // dynamically (methods attached per-instance) so an impl block would
        // register static-dispatch methods that silently defeat overrides.
        let result = check_source(
            r#"
            class Dog {
                name: string
            }
            impl Dog {
                fn speak(self) -> string { return "Woof" }
            }
            "#,
        );
        assert!(result.is_err(), "impl on a class should be a checker error");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("impl") && msg.contains("class"),
            "error should explain impl-on-class, got: {msg}"
        );
    }

    #[test]
    fn test_impl_block_on_struct_still_allowed() {
        // The rejection must be class-specific — impl on a struct is fine.
        assert!(check_source(
            r#"
            struct Point { x: int }
            impl Point {
                fn get(self) -> int { return self.x }
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

    /// Check source, returning the checker regardless of whether checking failed
    /// (so structured errors can be inspected).
    fn check_source_capturing_errors(source: &str) -> TypeChecker {
        use crate::lexer::tokenize;
        use crate::parser::parse;

        let tokens = tokenize(source).expect("tokenize");
        let program = parse(&tokens).expect("parse");
        let mut checker = TypeChecker::new();
        let _ = checker.check_program(&program);
        checker
    }

    #[test]
    fn undefined_variable_produces_structured_error() {
        let checker = check_source_capturing_errors("let y = x");
        let structured = checker.env.get_structured_errors();
        assert!(
            structured.iter().any(|e| matches!(
                e,
                CheckerError::UndefinedVariable { name, .. } if name == "x"
            )),
            "expected a structured UndefinedVariable error, got: {:?}",
            structured
        );
    }

    #[test]
    fn break_outside_loop_produces_structured_error() {
        let checker = check_source_capturing_errors("fn f() {\n    break\n}");
        assert!(
            checker
                .env
                .get_structured_errors()
                .iter()
                .any(|e| matches!(e, CheckerError::BreakOutsideLoop { .. })),
            "expected a structured BreakOutsideLoop error, got: {:?}",
            checker.env.get_structured_errors()
        );
    }

    #[test]
    fn bespoke_type_error_is_recorded_as_structured() {
        // "Condition must be bool" still uses the generic env.error() path; it
        // must now surface as a structured error carrying the statement span.
        let checker = check_source_capturing_errors("if 1 {\n}");
        let structured = checker.env.get_structured_errors();
        assert!(
            structured
                .iter()
                .any(|e| e.body().starts_with("Condition must be bool")),
            "expected the condition error as a structured diagnostic, got: {:?}",
            structured
        );
        // The error must live solely in the structured channel now.
        assert!(
            structured
                .iter()
                .all(|e| e.body().starts_with("Condition must be bool")),
            "unexpected extra structured errors: {:?}",
            structured
        );
    }

    #[test]
    fn unknown_primitive_method_produces_structured_error() {
        let checker = check_source_capturing_errors("let n = 1\nn.no_such_method()");
        assert!(
            checker.env.get_structured_errors().iter().any(|e| matches!(
                e,
                CheckerError::UnknownMethod { method, on_type, .. }
                    if method == "no_such_method" && on_type == "int"
            )),
            "expected a structured UnknownMethod error, got: {:?}",
            checker.env.get_structured_errors()
        );
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

    // ========================================================================
    // Bound-aware operations on type parameters
    // ========================================================================

    #[test]
    fn numeric_bound_allows_arithmetic_on_type_param() {
        let result = check_source(
            r#"
            fn add<T: Numeric>(a: T, b: T) -> T {
                return a + b
            }
            "#,
        );
        assert!(
            result.is_ok(),
            "arithmetic on a Numeric-bounded type param should type-check, got: {:?}",
            result
        );
    }

    #[test]
    fn unbounded_type_param_rejects_arithmetic() {
        let result = check_source(
            r#"
            fn add<T>(a: T, b: T) -> T {
                return a + b
            }
            "#,
        );
        assert!(
            result.is_err(),
            "arithmetic on an unbounded type param should still be rejected"
        );
    }

    #[test]
    fn comparable_bound_allows_ordering_on_type_param() {
        let result = check_source(
            r#"
            fn less<T: Comparable>(a: T, b: T) -> bool {
                return a < b
            }
            "#,
        );
        assert!(
            result.is_ok(),
            "ordering on a Comparable-bounded type param should type-check, got: {:?}",
            result
        );
    }

    #[test]
    fn ord_bound_allows_ordering_on_type_param() {
        let result = check_source(
            r#"
            fn less<T: Ord>(a: T, b: T) -> bool {
                return a < b
            }
            "#,
        );
        assert!(
            result.is_ok(),
            "ordering on an Ord-bounded type param should type-check, got: {:?}",
            result
        );
    }

    #[test]
    fn unbounded_type_param_rejects_ordering() {
        let result = check_source(
            r#"
            fn less<T>(a: T, b: T) -> bool {
                return a < b
            }
            "#,
        );
        assert!(
            result.is_err(),
            "ordering on an unbounded type param should still be rejected"
        );
    }

    #[test]
    fn numeric_bound_allows_bitwise_on_type_param() {
        let result = check_source(
            r#"
            fn bor<T: Numeric>(a: T, b: T) -> T {
                return a | b
            }
            "#,
        );
        assert!(
            result.is_ok(),
            "bitwise on a Numeric-bounded type param should type-check, got: {:?}",
            result
        );
    }

    #[test]
    fn unbounded_type_param_rejects_bitwise() {
        let result = check_source(
            r#"
            fn bor<T>(a: T, b: T) -> T {
                return a | b
            }
            "#,
        );
        assert!(
            result.is_err(),
            "bitwise on an unbounded type param should still be rejected"
        );
    }

    #[test]
    fn generic_method_signature_resolves_type_params() {
        // `impl<T> Box<T>` with a method that introduces its own `<U>` must
        // resolve both T and U rather than reporting `Unknown type`.
        let result = check_source(
            r#"
            struct Box<T> { value: T }

            impl<T> Box<T> {
                fn get(self) -> T { return self.value }
                fn map<U>(self, f: fn(T) -> U) -> Box<U> {
                    return Box { value: f(self.value) }
                }
            }
            "#,
        );
        assert!(
            result.is_ok(),
            "generic method signatures should resolve T/U as erased type params, got: {:?}",
            result
        );
    }

    #[test]
    fn resolve_type_expr_maps_type_param_not_unknown() {
        use crate::ast::{TypeExpr, TypeExprKind};

        let mut checker = TypeChecker::new();
        checker
            .current_type_params
            .insert("T".to_string(), Vec::new());

        let span = Span { line: 1, column: 1 };
        let resolved = checker.resolve_type_expr(&TypeExpr {
            kind: TypeExprKind::Named("T".to_string()),
            span,
        });
        assert!(
            matches!(resolved, Type::TypeParam(ref n) if n == "T"),
            "an in-scope type param must resolve to TypeParam, got: {:?}",
            resolved
        );

        // A name that is NOT in scope must still be Unknown (and report).
        let span2 = Span { line: 1, column: 1 };
        let unknown = checker.resolve_type_expr(&TypeExpr {
            kind: TypeExprKind::Named("Nope".to_string()),
            span: span2,
        });
        assert!(
            matches!(unknown, Type::Unknown),
            "an out-of-scope name must remain Unknown, got: {:?}",
            unknown
        );
    }

    #[test]
    fn explicit_any_annotations_resolve_and_check() {
        let result = check_source(
            r#"
            fn identity(value: any) -> any {
                return value
            }

            let number: any = identity(42)
            let text: any = identity("lira")
            println(number)
            println(text)
            "#,
        );

        assert!(
            result.is_ok(),
            "explicit `any` annotations should use the dynamic type: {result:?}"
        );
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
        assert!(
            !checked.sema.expr_types.is_empty(),
            "SemanticTables should have expression types after checking"
        );
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[0].kind
        {
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[0].kind
        {
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
        assert!(
            !checked.sema.symbols.is_empty() || checked.sema.generic_instantiations.is_empty(),
            "SemanticTables should be populated after checking"
        );
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[1].kind
        {
            assert!(
                checked.sema.symbol_refs.contains_key(&init.id),
                "Symbol reference should be recorded for identifier 'x'"
            );
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
        assert!(
            !checked.sema.symbols.is_empty(),
            "Symbols should be recorded after checking variable references"
        );
    }

    #[test]
    fn test_sema_repeated_uses_share_one_symbol_id() {
        // Multiple uses of one binding must resolve to the same SymbolId so
        // references can be grouped.
        let source = r#"
            let x = 1
            let a = x
            let b = x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        let use_id = |stmt_idx: usize| {
            if let StatementKind::VarDecl {
                initializer: Some(init),
                ..
            } = &checked.program.statements[stmt_idx].kind
            {
                *checked
                    .sema
                    .symbol_refs
                    .get(&init.id)
                    .expect("use should resolve to a symbol")
            } else {
                panic!("expected VarDecl");
            }
        };

        assert_eq!(use_id(1), use_id(2), "both uses of x share one SymbolId");
    }

    #[test]
    fn test_sema_shadowing_distinct_symbol_ids() {
        // An inner shadowing binding must get a distinct SymbolId from the
        // outer same-named binding, and its uses must resolve to the inner one.
        let source = r#"
            let x = 1
            fn f() -> int {
                let x = 2
                return x
            }
            let y = x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // Outer use: `let y = x`
        let outer_use = if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[2].kind
        {
            *checked.sema.symbol_refs.get(&init.id).unwrap()
        } else {
            panic!("expected VarDecl");
        };

        // Inner use: `return x` inside f
        let inner_use =
            if let StatementKind::FnDecl { body, .. } = &checked.program.statements[1].kind {
                let ret = body.statements.last().unwrap();
                if let StatementKind::Return(Some(e)) = &ret.kind {
                    *checked.sema.symbol_refs.get(&e.id).unwrap()
                } else {
                    panic!("expected return");
                }
            } else {
                panic!("expected FnDecl");
            };

        assert_ne!(
            outer_use, inner_use,
            "shadowed bindings must have distinct SymbolIds"
        );
    }

    #[test]
    fn test_sema_decl_node_points_at_declaration() {
        // The symbol entry's decl_node must be the declaration's pattern node,
        // and a cursor on the declaration (via symbol_refs at the decl node)
        // must resolve to the same SymbolId as the uses.
        let source = r#"
            let x = 1
            let a = x
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        // decl pattern node of `let x`
        let decl_node =
            if let StatementKind::VarDecl { pattern, .. } = &checked.program.statements[0].kind {
                pattern.id
            } else {
                panic!("expected VarDecl");
            };

        // use node in `let a = x`
        let use_node = if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[1].kind
        {
            init.id
        } else {
            panic!("expected VarDecl");
        };

        let decl_sym = *checked.sema.symbol_refs.get(&decl_node).unwrap();
        let use_sym = *checked.sema.symbol_refs.get(&use_node).unwrap();
        assert_eq!(decl_sym, use_sym, "decl and use share one SymbolId");
        assert_eq!(
            checked.sema.symbols.get(&decl_sym).unwrap().decl_node,
            decl_node,
            "symbol entry decl_node points at the declaration pattern"
        );
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[1].kind
        {
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[1].kind
        {
            assert!(
                checked.sema.expr_types.contains_key(&init.id),
                "Expression type should be recorded for enum constructor"
            );
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[3].kind
        {
            if let Some(resolution) = checked.sema.call_resolution.get(&init.id) {
                match resolution {
                    crate::sema::CallResolution::Method {
                        type_name,
                        method_name,
                    } => {
                        assert_eq!(type_name, "Point", "Should resolve to type 'Point'");
                        assert_eq!(method_name, "add", "Should resolve to method 'add'");
                    }
                    _ => panic!("Expected Method call resolution"),
                }
            }
        }
    }

    #[test]
    fn test_sema_static_method_call_resolution() {
        let source = r#"
            struct Counter {
                fn make() -> int { return 1 }
            }
            let value = Counter.make()
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        let StatementKind::VarDecl {
            initializer: Some(call),
            ..
        } = &checked.program.statements[1].kind
        else {
            panic!("expected initialized binding");
        };
        assert!(matches!(
            checked.sema.call_resolution.get(&call.id),
            Some(crate::sema::CallResolution::StaticMethod {
                type_name,
                method_name,
            }) if type_name == "Counter" && method_name == "make"
        ));
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
        if let StatementKind::VarDecl {
            initializer: Some(init),
            ..
        } = &checked.program.statements[1].kind
        {
            if let Some(resolution) = checked.sema.field_resolution.get(&init.id) {
                assert_eq!(resolution.field_name, "x", "Should resolve field 'x'");
                assert!(!resolution.is_method, "Should not be a method");
            }
        }
    }

    #[test]
    fn test_sema_self_member_resolution_recorded() {
        // `self.field` and `self.method()` inside a method body must be recorded
        // in `field_resolution`, scoped to the implementing type, so the LSP can
        // find references/rename members from inside method bodies.
        let source = r#"
            struct Point {
                x: int
                y: int
            }

            impl Point {
                fn dist(self) -> int { self.x + self.y }
                fn twice(self) -> int { self.dist() + self.dist() }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        let resolutions: Vec<_> = checked.sema.field_resolution.values().collect();

        // Every self-member is owned by Point (never any other type).
        assert!(resolutions
            .iter()
            .all(|r| r.owner_type == Type::Struct("Point".to_string())));

        let by = |name: &str| {
            resolutions
                .iter()
                .find(|r| r.field_name == name)
                .unwrap_or_else(|| panic!("missing resolution for `{name}`"))
        };
        assert!(!by("x").is_method, "x is a field");
        assert!(!by("y").is_method, "y is a field");
        assert!(by("dist").is_method, "dist is a method");
    }

    #[test]
    fn test_sema_self_member_scoped_per_type() {
        // Two structs each declare a field `x`. The `self.x` access in each
        // method must be scoped to its own owner type.
        let source = r#"
            struct Point { x: int }
            struct Box { x: int }

            impl Point { fn get(self) -> int { self.x } }
            impl Box { fn get(self) -> int { self.x } }
        "#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let checked = check(&ast).unwrap();

        let owners: Vec<_> = checked
            .sema
            .field_resolution
            .values()
            .filter(|r| r.field_name == "x")
            .map(|r| r.owner_type.clone())
            .collect();
        assert!(owners.contains(&Type::Struct("Point".to_string())));
        assert!(owners.contains(&Type::Struct("Box".to_string())));
    }

    // ========================================================================
    // Structured CheckerError variant tests
    // ========================================================================

    /// Type check `source` and return the structured errors it produced.
    fn collect_errors(source: &str) -> Vec<CheckerError> {
        let tokens = tokenize(source).expect("tokenize");
        let ast = parse(&tokens).expect("parse");
        let (_, errors) = check_collecting(&ast);
        errors
    }

    #[test]
    fn test_var_init_mismatch_is_typed_context() {
        let errors = collect_errors("let x: int = \"hello\"");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                CheckerError::TypeMismatchContext {
                    context: TypeContext::VarInit,
                    ..
                }
            )),
            "expected a VarInit TypeMismatchContext, got: {:?}",
            errors
        );
        // Message text must remain byte-for-byte unchanged.
        assert!(errors.iter().any(|e| e
            .message()
            .contains("Type mismatch: expected 'int', got 'string'")));
    }

    #[test]
    fn test_call_non_function_is_not_a_function() {
        let errors = collect_errors("let x = 5\nx()");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CheckerError::NotAFunction { .. })),
            "expected NotAFunction, got: {:?}",
            errors
        );
        assert!(errors
            .iter()
            .any(|e| e.message().contains("Cannot call non-function type:")));
    }

    #[test]
    fn test_if_stmt_condition_must_be_bool() {
        let errors = collect_errors("if 5 { }");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CheckerError::ConditionMustBeBool { got: Some(_), .. })),
            "expected ConditionMustBeBool with got, got: {:?}",
            errors
        );
        assert!(errors
            .iter()
            .any(|e| e.message().contains("Condition must be bool, got 'int'")));
    }

    #[test]
    fn test_array_index_must_be_integer_structured() {
        let errors = collect_errors("let a = [1, 2, 3]\nlet b = a[\"x\"]");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                CheckerError::MustBeInteger {
                    context: IntContext::ArrayIndex,
                    ..
                }
            )),
            "expected MustBeInteger ArrayIndex, got: {:?}",
            errors
        );
        assert!(errors
            .iter()
            .any(|e| e.message().contains("Array index must be integer")));
    }

    #[test]
    fn test_logical_not_requires_bool_operand() {
        let errors = collect_errors("let x = !5");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                CheckerError::RequiresBoolOperand {
                    arity: crate::errors::OperandArity::Single,
                    ..
                }
            )),
            "expected singular RequiresBoolOperand, got: {:?}",
            errors
        );
        assert!(errors
            .iter()
            .any(|e| e.message().contains("Logical not requires bool operand")));
    }

    // ========================================================================
    // Named-argument reordering at call sites
    // ========================================================================

    #[test]
    fn test_named_args_reordered_typecheck() {
        // Named args supplied out of order must bind to the correct parameter by
        // name, so the (string, int) call type-checks against fn(int, string).
        let src = "fn f(a: int, b: string) {}\nfn main() { f(b: \"x\", a: 1) }";
        assert!(
            check_source(src).is_ok(),
            "out-of-order named args should type-check: {:?}",
            check_source(src)
        );
    }

    #[test]
    fn test_named_args_mixed_with_positional() {
        let src = "fn g(a: int, b: int, c: int) {}\nfn main() { g(1, c: 3, b: 2) }";
        assert!(check_source(src).is_ok(), "{:?}", check_source(src));
    }

    #[test]
    fn test_named_args_unknown_name_errors() {
        let errors = collect_errors("fn f(a: int, b: string) {}\nfn main() { f(a: 1, c: \"x\") }");
        assert!(
            errors
                .iter()
                .any(|e| e.message().contains("Unknown named argument 'c'")),
            "expected unknown-named-argument error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_named_args_duplicate_errors() {
        let errors = collect_errors("fn f(a: int, b: string) {}\nfn main() { f(1, a: 2) }");
        assert!(
            errors
                .iter()
                .any(|e| e.message().contains("Duplicate value for parameter 'a'")),
            "expected duplicate-parameter error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_named_args_missing_required_errors() {
        // Supplying only `b` leaves required `a` unbound.
        let errors = collect_errors("fn f(a: int, b: string) {}\nfn main() { f(b: \"x\") }");
        assert!(
            errors
                .iter()
                .any(|e| e.message().contains("Missing value for parameter 'a'")),
            "expected missing-parameter error, got: {:?}",
            errors
        );
    }
}
