//! Bytecode loading and parsing
//!
//! Loads .lic bytecode files into memory for execution.

use crate::value::Value;
use lira_core::bytecode::DebugInfo;
use lira_core::{BYTECODE_MAGIC, BYTECODE_VERSION};
use std::rc::Rc;

/// A loaded bytecode program
pub struct Program {
    /// Constant pool
    pub constants: Vec<Value>,
    /// Bytecode instructions
    pub code: Vec<u8>,
    /// Entry point offset
    pub entry_point: usize,
    /// Function table
    pub functions: Vec<FunctionInfo>,
    /// Debug information for error reporting
    pub debug_info: DebugInfo,
    /// Source file name
    pub source_file: Option<String>,
}

/// Function information from bytecode
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub code_offset: usize,
    pub param_count: u8,
    pub local_count: u16,
}

/// Bytecode reader helper
struct BytecodeReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BytecodeReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err("Unexpected end of bytecode".to_string());
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    #[allow(dead_code)]
    fn read_u16(&mut self) -> Result<u16, String> {
        let lo = self.read_u8()? as u16;
        let hi = self.read_u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let b0 = self.read_u8()? as u32;
        let b1 = self.read_u8()? as u32;
        let b2 = self.read_u8()? as u32;
        let b3 = self.read_u8()? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn read_i64(&mut self) -> Result<i64, String> {
        let lo = self.read_u32()? as u64;
        let hi = self.read_u32()? as u64;
        Ok((lo | (hi << 32)) as i64)
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        let lo = self.read_u32()? as u64;
        let hi = self.read_u32()? as u64;
        Ok(f64::from_bits(lo | (hi << 32)))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.pos + len > self.data.len() {
            return Err("Unexpected end of bytecode".to_string());
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    #[allow(dead_code)]
    fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
}

/// Load bytecode from bytes
pub fn load(bytes: &[u8]) -> Result<Program, String> {
    if bytes.len() < 24 {
        return Err("Bytecode too short".to_string());
    }

    let mut reader = BytecodeReader::new(bytes);

    // Parse header
    let magic = reader.read_u32()?;
    let version = reader.read_u32()?;
    let _flags = reader.read_u32()?;
    let entry_point = reader.read_u32()? as usize;
    let constant_count = reader.read_u32()? as usize;
    let _function_count = reader.read_u32()?;

    // Validate header
    if magic != BYTECODE_MAGIC {
        return Err(format!("Invalid magic number: 0x{:08X}", magic));
    }
    if version != BYTECODE_VERSION {
        return Err(format!("Unsupported version: {}", version));
    }

    // Parse constant pool
    let mut constants = Vec::with_capacity(constant_count);
    for _ in 0..constant_count {
        let tag = reader.read_u8()?;
        let value = match tag {
            0x00 => Value::Null,
            0x01 => {
                let b = reader.read_u8()?;
                Value::Bool(b != 0)
            }
            0x02 => {
                let n = reader.read_i64()?;
                Value::Int(n)
            }
            0x03 => {
                let f = reader.read_f64()?;
                Value::Float(f)
            }
            0x04 => {
                let len = reader.read_u32()? as usize;
                let bytes = reader.read_bytes(len)?;
                let s = String::from_utf8(bytes.to_vec())
                    .map_err(|_| "Invalid UTF-8 in string constant")?;
                Value::String(Rc::new(s))
            }
            0x05 => {
                let offset = reader.read_i64()? as usize;
                Value::Function(offset)
            }
            _ => return Err(format!("Unknown constant tag: 0x{:02X}", tag)),
        };
        constants.push(value);
    }

    // Code section (now has length prefix)
    let code_len = reader.read_u32()? as usize;
    let code = reader.read_bytes(code_len)?.to_vec();

    // Debug info section
    let mut debug_info = DebugInfo::new();
    let line_count = reader.read_u32()? as usize;
    for _ in 0..line_count {
        let start_offset = reader.read_u32()?;
        let end_offset = reader.read_u32()?;
        let line = reader.read_u32()?;
        let column = reader.read_u32()?;
        debug_info.add_line(start_offset, end_offset, line, column);
    }

    // Source file name
    let filename_len = reader.read_u32()? as usize;
    let source_file = if filename_len > 0 {
        let bytes = reader.read_bytes(filename_len)?;
        Some(String::from_utf8(bytes.to_vec()).map_err(|_| "Invalid UTF-8 in source filename")?)
    } else {
        None
    };

    debug_info.source_file = source_file.clone();

    Ok(Program {
        constants,
        code,
        entry_point,
        functions: Vec::new(),
        debug_info,
        source_file,
    })
}
