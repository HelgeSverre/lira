//! Bytecode format definitions
//!
//! Defines the structure of .lic bytecode files.
//! See docs/lira/10-bytecode-format.md for the full specification.

use crate::{BYTECODE_MAGIC, BYTECODE_VERSION};

/// Bytecode file header
#[derive(Debug, Clone)]
pub struct BytecodeHeader {
    /// Magic number (BYTECODE_MAGIC)
    pub magic: u32,
    /// Format version
    pub version: u32,
    /// Flags (reserved)
    pub flags: u32,
    /// Entry point function index
    pub entry_point: u32,
    /// Number of constants in constant pool
    pub constant_count: u32,
    /// Number of functions
    pub function_count: u32,
}

impl BytecodeHeader {
    pub fn new(entry_point: u32, constant_count: u32, function_count: u32) -> Self {
        Self {
            magic: BYTECODE_MAGIC,
            version: BYTECODE_VERSION,
            flags: 0,
            entry_point,
            constant_count,
            function_count,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.magic != BYTECODE_MAGIC {
            return Err("Invalid bytecode magic number");
        }
        if self.version != BYTECODE_VERSION {
            return Err("Unsupported bytecode version");
        }
        Ok(())
    }
}

/// Constant pool entry types
#[derive(Debug, Clone)]
pub enum Constant {
    /// Integer constant
    Int(i64),
    /// Floating point constant
    Float(f64),
    /// String constant
    String(String),
    /// Boolean constant
    Bool(bool),
    /// Null constant
    Null,
    /// Function reference (code offset)
    Function(usize),
}

/// Debug line information entry
/// Maps a range of bytecode offsets to a source line
#[derive(Debug, Clone)]
pub struct LineInfo {
    /// Start offset in bytecode
    pub start_offset: u32,
    /// End offset in bytecode (exclusive)
    pub end_offset: u32,
    /// Source line number (1-based)
    pub line: u32,
    /// Source column number (1-based)
    pub column: u32,
}

/// Debug information section
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// Source file name
    pub source_file: Option<String>,
    /// Line number table
    pub line_table: Vec<LineInfo>,
}

impl DebugInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a line info entry
    pub fn add_line(&mut self, start_offset: u32, end_offset: u32, line: u32, column: u32) {
        self.line_table.push(LineInfo {
            start_offset,
            end_offset,
            line,
            column,
        });
    }

    /// Look up the source location for a bytecode offset
    pub fn lookup(&self, offset: u32) -> Option<(u32, u32)> {
        for info in &self.line_table {
            if offset >= info.start_offset && offset < info.end_offset {
                return Some((info.line, info.column));
            }
        }
        None
    }
}
