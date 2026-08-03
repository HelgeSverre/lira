//! Identifier types for the Lira compiler
//!
//! Provides stable identifiers for AST nodes, symbols, functions, and types.
//! Used as keys for semantic side tables (SemanticTables).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier for an AST node.
///
/// High 32 bits encode the source-file index (0 = main file, 1+ = imported
/// modules). Low 32 bits are a per-file monotonic counter. Together they
/// guarantee global uniqueness across a multi-file compilation without
/// remapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u64);

impl NodeId {
    /// Create a new NodeId with the given raw value (for synthetic/dummy nodes).
    pub fn new(value: u32) -> Self {
        Self(value as u64)
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

/// Generator for globally-unique `NodeId`s.
///
/// Seeds from `(file_index << 32)` so that every compilation unit produces
/// non-overlapping ID ranges.  File 0 (main source) gets the low 32-bit
/// range; file 1 (first import) gets the next range, etc.
pub struct NodeIdGen(u64);

impl NodeIdGen {
    /// Create a new generator for the given compilation-unit file index.
    /// `file_index = 0` for the main file, 1+ for imported modules.
    pub fn new(file_index: u32) -> Self {
        Self((file_index as u64) << 32)
    }

    /// Generate the next unique NodeId (monotonic within this file).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> NodeId {
        let id = NodeId(self.0);
        self.0 += 1;
        id
    }

    /// Number of IDs generated so far (local counter, low 32 bits only).
    pub fn count(&self) -> u32 {
        (self.0 & 0xFFFF_FFFF) as u32
    }
}

impl Default for NodeIdGen {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generation() {
        let mut gen = NodeIdGen::new(0);
        let id1 = gen.next();
        let id2 = gen.next();
        let id3 = gen.next();

        assert_eq!(id1, NodeId(0));
        assert_eq!(id2, NodeId(1));
        assert_eq!(id3, NodeId(2));
        assert_eq!(gen.count(), 3);
    }

    #[test]
    fn test_node_id_file_prefix() {
        let mut gen0 = NodeIdGen::new(0);
        let mut gen1 = NodeIdGen::new(1);
        let id0 = gen0.next();
        let id1 = gen1.next();
        // IDs from different files must not collide
        assert_ne!(id0, id1);
        assert_eq!(id0, NodeId(0));
        assert_eq!(id1, NodeId(1 << 32));
    }

    #[test]
    fn test_node_id_uniqueness() {
        let mut gen = NodeIdGen::new(0);
        let mut ids = std::collections::HashSet::new();

        for _ in 0..100 {
            let id = gen.next();
            assert!(ids.insert(id), "Duplicate NodeId: {:?}", id);
        }
    }

    #[test]
    fn test_node_id_display() {
        assert_eq!(NodeId::new(0).to_string(), "N0");
        assert_eq!(NodeId::new(42).to_string(), "N42");
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
