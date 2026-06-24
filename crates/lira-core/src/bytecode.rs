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

/// Local variable symbol information
/// Maps a local slot to its source name within a scope
#[derive(Debug, Clone)]
pub struct LocalSymbol {
    /// Local variable slot index
    pub slot: u16,
    /// Variable name in source code
    pub name: String,
    /// Scope depth (0 = function top-level)
    pub scope_depth: u16,
    /// Bytecode offset where this variable becomes valid
    pub start_offset: u32,
    /// Bytecode offset where this variable goes out of scope (0 = until function end)
    pub end_offset: u32,
}

/// Function symbol information
/// Maps a function's entry bytecode offset to its source name, so call frames
/// and stack traces can be labelled with the real function name.
#[derive(Debug, Clone)]
pub struct FunctionSymbol {
    /// Bytecode offset of the function's entry point
    pub code_offset: u32,
    /// Function name in source code
    pub name: String,
}

/// Debug information section
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// Source file name
    pub source_file: Option<String>,
    /// Line number table
    pub line_table: Vec<LineInfo>,
    /// Local variable symbols
    pub local_symbols: Vec<LocalSymbol>,
    /// Function symbols (entry offset -> name)
    pub function_symbols: Vec<FunctionSymbol>,
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

    /// Add a local symbol entry
    pub fn add_local_symbol(
        &mut self,
        slot: u16,
        name: String,
        scope_depth: u16,
        start_offset: u32,
    ) {
        self.local_symbols.push(LocalSymbol {
            slot,
            name,
            scope_depth,
            start_offset,
            end_offset: 0, // Will be patched when scope ends
        });
    }

    /// Look up the name for a local slot at a given bytecode offset
    pub fn lookup_local_name(&self, slot: u16, offset: u32) -> Option<&str> {
        // Find the most recent symbol for this slot that's valid at this offset
        self.local_symbols
            .iter()
            .rev()
            .find(|sym| {
                sym.slot == slot
                    && offset >= sym.start_offset
                    && (sym.end_offset == 0 || offset < sym.end_offset)
            })
            .map(|sym| sym.name.as_str())
    }

    /// Add a function symbol entry
    pub fn add_function_symbol(&mut self, code_offset: u32, name: String) {
        self.function_symbols
            .push(FunctionSymbol { code_offset, name });
    }

    /// Look up the name of the function whose entry point is `offset`
    pub fn function_name_at(&self, offset: u32) -> Option<&str> {
        self.function_symbols
            .iter()
            .find(|sym| sym.code_offset == offset)
            .map(|sym| sym.name.as_str())
    }

    /// Get all local names valid at a given offset, returning a map of slot -> name
    pub fn get_local_names_at(&self, offset: u32) -> std::collections::HashMap<u16, &str> {
        let mut names = std::collections::HashMap::new();
        for sym in &self.local_symbols {
            if offset >= sym.start_offset && (sym.end_offset == 0 || offset < sym.end_offset) {
                names.insert(sym.slot, sym.name.as_str());
            }
        }
        names
    }
}
