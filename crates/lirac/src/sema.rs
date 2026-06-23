//! Lira Semantic Tables
//!
//! Captures all type checker facts in side tables for later use by codegen.
//! The checker already computes these facts — we just store them here.

use std::collections::HashMap;

use crate::checker::Type;
use crate::ids::{NodeId, SymbolId};

/// All semantic information produced by the type checker
#[derive(Debug, Clone)]
pub struct SemanticTables {
    /// Expression result types
    pub expr_types: HashMap<NodeId, Type>,

    /// Statement types (for VarDecl, the declared/inferred type)
    pub stmt_types: HashMap<NodeId, Type>,

    /// Pattern types
    pub pattern_types: HashMap<NodeId, Type>,

    /// Resolved symbol references (which SymbolId does this identifier refer to?)
    pub symbol_refs: HashMap<NodeId, SymbolId>,

    /// Symbol table (all declarations)
    pub symbols: HashMap<SymbolId, SymbolEntry>,

    /// Call resolution (which function/method was called?)
    pub call_resolution: HashMap<NodeId, CallResolution>,

    /// Field resolution (which field was accessed?)
    pub field_resolution: HashMap<NodeId, FieldResolution>,

    /// Generic instantiations.
    ///
    /// UNUSED INFRA: never read by codegen. Generics are type-erased, so a single
    /// shared body is emitted per generic function. Retained as scaffolding for a
    /// future monomorphizing backend; see `checker::GenericInstantiation`.
    pub generic_instantiations: Vec<GenericInstantiation>,

    /// Per-type member snapshot (fields + methods) keyed by type name.
    ///
    /// Populated by the checker from its (otherwise dropped) `TypeEnv` so that
    /// consumers like the LSP can enumerate a type's members for member
    /// completion and signature-aware hover without reaching into checker
    /// internals.
    pub type_members: HashMap<String, TypeMembers>,
}

impl SemanticTables {
    /// Create empty semantic tables
    pub fn new() -> Self {
        Self {
            expr_types: HashMap::new(),
            stmt_types: HashMap::new(),
            pattern_types: HashMap::new(),
            symbol_refs: HashMap::new(),
            symbols: HashMap::new(),
            call_resolution: HashMap::new(),
            field_resolution: HashMap::new(),
            generic_instantiations: Vec::new(),
            type_members: HashMap::new(),
        }
    }
}

impl Default for SemanticTables {
    fn default() -> Self {
        Self::new()
    }
}

/// A symbol entry in the symbol table
#[derive(Debug, Clone)]
pub struct SymbolEntry {
    pub id: SymbolId,
    pub name: String,
    pub ty: Type,
    pub kind: SymbolKind,
    pub decl_node: NodeId, // AST node where this symbol is declared
}

/// Kind of symbol
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Variable,
    Function,
    Parameter,
    Type,
    Field,
    Method,
    Constant,
}

/// How a call was resolved
#[derive(Debug, Clone)]
pub enum CallResolution {
    /// Free function call: foo(args)
    Function { name: String },
    /// Method call: obj.method(args)
    Method {
        type_name: String,
        method_name: String,
    },
    /// Static method call: Type::method(args)
    StaticMethod {
        type_name: String,
        method_name: String,
    },
}

/// A member of a type (field or method) exposed for completion/hover.
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub name: String,
    /// The member's type. For methods this is a `Type::Function`.
    pub ty: Type,
}

/// A snapshot of a type's members, decoupled from the checker's `TypeEnv`.
#[derive(Debug, Clone, Default)]
pub struct TypeMembers {
    pub fields: Vec<MemberInfo>,
    pub methods: Vec<MemberInfo>,
}

/// How a field access was resolved
#[derive(Debug, Clone)]
pub struct FieldResolution {
    pub owner_type: Type,
    pub field_name: String,
    pub is_method: bool,
    pub resolved_type: Type,
}

/// A generic instantiation that was created during type checking.
///
/// UNUSED INFRA: see [`SemanticTables::generic_instantiations`]. Recorded but not
/// consumed under the type-erasure runtime model.
#[derive(Debug, Clone)]
pub struct GenericInstantiation {
    pub generic_name: String,
    pub concrete_type: Type,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_semantic_tables() {
        let sema = SemanticTables::new();
        assert!(sema.expr_types.is_empty());
        assert!(sema.stmt_types.is_empty());
        assert!(sema.pattern_types.is_empty());
        assert!(sema.symbol_refs.is_empty());
        assert!(sema.symbols.is_empty());
        assert!(sema.call_resolution.is_empty());
        assert!(sema.field_resolution.is_empty());
        assert!(sema.generic_instantiations.is_empty());
        assert!(sema.type_members.is_empty());
    }

    #[test]
    fn test_symbol_entry_creation() {
        let entry = SymbolEntry {
            id: SymbolId(0),
            name: "x".to_string(),
            ty: Type::Int,
            kind: SymbolKind::Variable,
            decl_node: NodeId(1),
        };

        assert_eq!(entry.id, SymbolId(0));
        assert_eq!(entry.name, "x");
        assert_eq!(entry.ty, Type::Int);
        assert_eq!(entry.kind, SymbolKind::Variable);
        assert_eq!(entry.decl_node, NodeId(1));
    }

    #[test]
    fn test_call_resolution_variants() {
        let func = CallResolution::Function {
            name: "foo".to_string(),
        };
        let method = CallResolution::Method {
            type_name: "Bar".to_string(),
            method_name: "baz".to_string(),
        };
        let static_method = CallResolution::StaticMethod {
            type_name: "Qux".to_string(),
            method_name: "new".to_string(),
        };

        assert!(matches!(func, CallResolution::Function { .. }));
        assert!(matches!(method, CallResolution::Method { .. }));
        assert!(matches!(static_method, CallResolution::StaticMethod { .. }));
    }

    #[test]
    fn test_field_resolution() {
        let res = FieldResolution {
            owner_type: Type::Struct("Point".to_string()),
            field_name: "x".to_string(),
            is_method: false,
            resolved_type: Type::Int,
        };

        assert_eq!(res.owner_type, Type::Struct("Point".to_string()));
        assert_eq!(res.field_name, "x");
        assert!(!res.is_method);
        assert_eq!(res.resolved_type, Type::Int);
    }
}
