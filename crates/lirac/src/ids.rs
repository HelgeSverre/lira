//! Identifier types for the Lira compiler
//!
//! Provides stable identifiers for AST nodes, symbols, functions, and types.
//! Used as keys for semantic side tables (SemanticTables).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier for an AST node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u32);

impl NodeId {
    /// Create a new NodeId with the given value
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "N{}", self.0)
    }
}

/// Identifier for a symbol (variable, function, type, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SymbolId(pub u32);

impl SymbolId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "S{}", self.0)
    }
}

/// Identifier for a function or method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FunctionId(pub u32);

impl FunctionId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for FunctionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "F{}", self.0)
    }
}

/// Identifier for a type (struct, enum, class, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypeId(pub u32);

impl TypeId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// Generator for unique NodeIds
pub struct NodeIdGen(u32);

impl NodeIdGen {
    /// Create a new generator starting at 0
    pub fn new() -> Self {
        Self(0)
    }

    /// Generate the next unique NodeId
    pub fn next(&mut self) -> NodeId {
        let id = NodeId(self.0);
        self.0 += 1;
        id
    }

    /// Get the current count of generated IDs
    pub fn count(&self) -> u32 {
        self.0
    }
}

impl Default for NodeIdGen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generation() {
        let mut gen = NodeIdGen::new();
        let id1 = gen.next();
        let id2 = gen.next();
        let id3 = gen.next();

        assert_eq!(id1, NodeId(0));
        assert_eq!(id2, NodeId(1));
        assert_eq!(id3, NodeId(2));
        assert_eq!(gen.count(), 3);
    }

    #[test]
    fn test_node_id_uniqueness() {
        let mut gen = NodeIdGen::new();
        let mut ids = std::collections::HashSet::new();

        for _ in 0..100 {
            let id = gen.next();
            assert!(ids.insert(id), "Duplicate NodeId: {:?}", id);
        }
    }

    #[test]
    fn test_node_id_display() {
        assert_eq!(NodeId(0).to_string(), "N0");
        assert_eq!(NodeId(42).to_string(), "N42");
    }

    #[test]
    fn test_symbol_id_display() {
        assert_eq!(SymbolId(0).to_string(), "S0");
        assert_eq!(SymbolId(5).to_string(), "S5");
    }

    #[test]
    fn test_function_id_display() {
        assert_eq!(FunctionId(0).to_string(), "F0");
        assert_eq!(FunctionId(10).to_string(), "F10");
    }

    #[test]
    fn test_type_id_display() {
        assert_eq!(TypeId(0).to_string(), "T0");
        assert_eq!(TypeId(3).to_string(), "T3");
    }
}
