//! Lira Core Library
//!
//! Shared types between the compiler (lirac) and virtual machine (liravm).
//! This includes bytecode format definitions, opcodes, and other shared structures.

pub mod bytecode;
pub mod opcode;

/// Lira bytecode file magic number: "LIBC" (0x4C494243)
pub const BYTECODE_MAGIC: u32 = 0x4C494243;

/// Current bytecode format version
pub const BYTECODE_VERSION: u32 = 2;
