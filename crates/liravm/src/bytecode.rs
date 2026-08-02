//! Bytecode loading and parsing
//!
//! Loads .lic bytecode files into memory for execution.

use crate::value::Value;
use lira_core::bytecode::DebugInfo;
use lira_core::{BYTECODE_MAGIC, BYTECODE_VERSION};
use std::rc::Rc;

const MAX_CONSTANT_COUNT: u32 = 65536;
const MAX_CODE_LENGTH: u32 = 10_000_000;

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
    let constant_count = reader.read_u32()?;
    let _function_count = reader.read_u32()?;

    // Validate header
    if magic != BYTECODE_MAGIC {
        return Err(format!("Invalid magic number: 0x{:08X}", magic));
    }
    if version != BYTECODE_VERSION {
        return Err(format!("Unsupported version: {}", version));
    }

    if constant_count > MAX_CONSTANT_COUNT {
        return Err(format!(
            "Constant pool too large: {} (max: {})",
            constant_count, MAX_CONSTANT_COUNT
        ));
    }

    // Parse constant pool
    let mut constants = Vec::with_capacity(constant_count as usize);
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
    let code_len = reader.read_u32()?;
    if code_len > MAX_CODE_LENGTH {
        return Err(format!(
            "Code section too large: {} (max: {})",
            code_len, MAX_CODE_LENGTH
        ));
    }
    let code = reader.read_bytes(code_len as usize)?.to_vec();

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

    // Local symbols section (optional - may not be present in older bytecode)
    // Check if there's more data to read
    if reader.pos < reader.data.len() {
        let symbol_count = reader.read_u32()? as usize;
        for _ in 0..symbol_count {
            let slot = reader.read_u16()?;
            let scope_depth = reader.read_u16()?;
            let start_offset = reader.read_u32()?;
            let end_offset = reader.read_u32()?;
            let name_len = reader.read_u32()? as usize;
            let name_bytes = reader.read_bytes(name_len)?;
            let name = String::from_utf8(name_bytes.to_vec())
                .map_err(|_| "Invalid UTF-8 in local symbol name")?;
            debug_info.add_local_symbol(slot, name, scope_depth, start_offset);
            // Set end_offset if non-zero
            if end_offset > 0 {
                if let Some(sym) = debug_info.local_symbols.last_mut() {
                    sym.end_offset = end_offset;
                }
            }
        }
    }

    // Function symbols section (optional - may not be present in older bytecode)
    if reader.pos < reader.data.len() {
        let func_count = reader.read_u32()? as usize;
        for _ in 0..func_count {
            let code_offset = reader.read_u32()?;
            let name_len = reader.read_u32()? as usize;
            let name_bytes = reader.read_bytes(name_len)?;
            let name = String::from_utf8(name_bytes.to_vec())
                .map_err(|_| "Invalid UTF-8 in function symbol name")?;
            debug_info.add_function_symbol(code_offset, name);
        }
    }

    Ok(Program {
        constants,
        code,
        entry_point,
        functions: Vec::new(),
        debug_info,
        source_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_excessive_constant_count() {
        let mut buf = Vec::new();
        // Header: magic, version, flags, entry_point
        buf.extend_from_slice(&0x4C494243u32.to_le_bytes()); // BYTECODE_MAGIC
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&0u32.to_le_bytes()); // entry_point
                                                    // Excessive constant count
        buf.extend_from_slice(&(MAX_CONSTANT_COUNT + 1).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // function_count

        match load(&buf) {
            Ok(_) => panic!("Expected error for excessive constant count"),
            Err(err) => assert!(
                err.contains("Constant pool too large"),
                "Expected 'Constant pool too large' error, got: {}",
                err
            ),
        }
    }

    #[test]
    fn test_reject_excessive_code_length() {
        let mut buf = Vec::new();
        // Header: magic, version, flags, entry_point
        buf.extend_from_slice(&0x4C494243u32.to_le_bytes()); // BYTECODE_MAGIC
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&0u32.to_le_bytes()); // entry_point
        buf.extend_from_slice(&0u32.to_le_bytes()); // constant_count = 0
        buf.extend_from_slice(&0u32.to_le_bytes()); // function_count
                                                    // Excessive code length
        buf.extend_from_slice(&(MAX_CODE_LENGTH + 1).to_le_bytes());

        match load(&buf) {
            Ok(_) => panic!("Expected error for excessive code length"),
            Err(err) => assert!(
                err.contains("Code section too large"),
                "Expected 'Code section too large' error, got: {}",
                err
            ),
        }
    }

    #[test]
    fn test_accept_valid_bounds() {
        let mut buf = Vec::new();
        // Header: magic, version, flags, entry_point
        buf.extend_from_slice(&0x4C494243u32.to_le_bytes()); // BYTECODE_MAGIC
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&0u32.to_le_bytes()); // entry_point
        buf.extend_from_slice(&1u32.to_le_bytes()); // 1 constant
        buf.extend_from_slice(&0u32.to_le_bytes()); // function_count
                                                    // 1 constant: Null tag
        buf.push(0x00); // Null
                        // Small code section
        buf.extend_from_slice(&4u32.to_le_bytes()); // code_len = 4
        buf.extend_from_slice(&[0xFFu8, 0x00, 0x00, 0x00]); // Halt; Nop; Nop; Nop
                                                            // No debug info
        buf.extend_from_slice(&0u32.to_le_bytes()); // line_count = 0
        buf.extend_from_slice(&0u32.to_le_bytes()); // filename_len = 0

        match load(&buf) {
            Ok(prog) => {
                assert_eq!(prog.constants.len(), 1);
                assert_eq!(prog.code.len(), 4);
            }
            Err(e) => panic!("Valid bytecode should load, got: {}", e),
        }
    }
}
