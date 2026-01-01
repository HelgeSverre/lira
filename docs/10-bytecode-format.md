# Lira Bytecode Format Specification

## Document Information

| Property           | Value                  |
| ------------------ | ---------------------- |
| **Document ID**    | 10-bytecode-format     |
| **Version**        | 1.0.0-draft            |
| **Status**         | Draft Specification    |
| **File Extension** | `.lic` (Lira Compiled) |

---

## Table of Contents

1. [Overview](#1-overview)
2. [File Structure](#2-file-structure)
3. [File Header](#3-file-header)
4. [Section Table](#4-section-table)
5. [Constant Pool](#5-constant-pool)
6. [Code Section](#6-code-section)
7. [Type Section](#7-type-section)
8. [Debug Section](#8-debug-section)
9. [Validation](#9-validation)

---

## 1. Overview

### 1.1 Purpose

The `.lic` (Lira Compiled) format is the bytecode format for Lira programs. It contains compiled bytecode instructions, constant data, type information, and optional debug information.

### 1.2 Design Goals

1. **Compact**: Minimize file size for fast loading
2. **Streamable**: Can be loaded incrementally
3. **Verifiable**: Structure allows validation before execution
4. **Debuggable**: Optional debug info for development

### 1.3 Byte Order

All multi-byte values are stored in **little-endian** format.

---

## 2. File Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                        LIC FILE LAYOUT                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    FILE HEADER (32 bytes)                  │  │
│  │  Magic (4) │ Version (4) │ Flags (4) │ Sections (4) │ ... │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              SECTION TABLE (N × 24 bytes)                  │  │
│  │  [SectionHeader₀, SectionHeader₁, ..., SectionHeaderₙ]    │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   SECTION DATA                             │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │              CONSTANT POOL (required)                │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │                 CODE (required)                      │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │                TYPES (required)                      │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │             IMPORTS (optional)                       │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │             EXPORTS (optional)                       │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │          DEBUG INFO (optional)                       │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. File Header

### 3.1 Header Structure (32 bytes)

```rust
#[repr(C, packed)]
struct LicHeader {
    /// Magic number: "LiC\0" = 0x0043694C (little-endian)
    magic: u32,

    /// Version: major (high 16 bits), minor (low 16 bits)
    version: u32,

    /// Compilation flags
    flags: u32,

    /// Number of sections
    section_count: u32,

    /// Entry point function index (0xFFFFFFFF if library)
    entry_point: u32,

    /// Total file size in bytes
    file_size: u32,

    /// CRC32 checksum of file (excluding this field)
    checksum: u32,

    /// Reserved for future use
    reserved: u32,
}
```

### 3.2 Magic Number

```
Byte 0: 'L' (0x4C)
Byte 1: 'i' (0x69)
Byte 2: 'C' (0x43)
Byte 3: '\0' (0x00)

As u32 (little-endian): 0x0043694C
```

### 3.3 Version Encoding

```
Bits 31-16: Major version (0-65535)
Bits 15-0:  Minor version (0-65535)

Current version: 1.0 = 0x00010000
```

### 3.4 Flags

```rust
struct LicFlags(u32);

impl LicFlags {
    /// Debug information present
    const DEBUG_INFO: u32       = 1 << 0;
    /// Optimized build
    const OPTIMIZED: u32        = 1 << 1;
    /// This is a library (no entry point)
    const LIBRARY: u32          = 1 << 2;
    /// Position-independent code
    const PIC: u32              = 1 << 3;
    /// Contains unsafe code
    const UNSAFE: u32           = 1 << 4;
    /// Uses native extensions
    const NATIVE_EXT: u32       = 1 << 5;
}
```

---

## 4. Section Table

### 4.1 Section Header (24 bytes)

```rust
#[repr(C, packed)]
struct SectionHeader {
    /// Section type identifier
    section_type: u32,

    /// Offset from file start to section data
    offset: u32,

    /// Size of section data in bytes
    size: u32,

    /// Number of entries (interpretation depends on type)
    entry_count: u32,

    /// Section-specific flags
    flags: u32,

    /// Required alignment (power of 2)
    alignment: u32,
}
```

### 4.2 Section Types

```rust
#[repr(u32)]
enum SectionType {
    /// Constant pool
    ConstPool   = 0x01,
    /// Bytecode instructions
    Code        = 0x02,
    /// Type definitions
    Types       = 0x03,
    /// Import declarations
    Imports     = 0x04,
    /// Export declarations
    Exports     = 0x05,
    /// String table (for names)
    Strings     = 0x06,
    /// Debug line numbers
    DebugLines  = 0x10,
    /// Debug local variables
    DebugLocals = 0x11,
    /// Source file references
    DebugSource = 0x12,
}
```

### 4.3 Required Sections

Every `.lic` file must contain:

- `ConstPool` (0x01)
- `Code` (0x02)
- `Types` (0x03)
- `Strings` (0x06)

---

## 5. Constant Pool

### 5.1 Constant Pool Header

```rust
#[repr(C)]
struct ConstPoolHeader {
    /// Total number of entries
    count: u32,
    /// Total size of constant data in bytes
    data_size: u32,
}
```

### 5.2 Constant Entry Format

Each constant entry starts with a 1-byte tag:

```rust
#[repr(u8)]
enum ConstTag {
    Null        = 0x00,
    Bool        = 0x01,
    Int8        = 0x02,
    Int16       = 0x03,
    Int32       = 0x04,
    Int64       = 0x05,
    UInt8       = 0x06,
    UInt16      = 0x07,
    UInt32      = 0x08,
    UInt64      = 0x09,
    Float32     = 0x10,
    Float64     = 0x11,
    Char        = 0x12,
    String      = 0x20,
    Bytes       = 0x21,
    TypeRef     = 0x30,
    FuncRef     = 0x31,
    FieldRef    = 0x32,
    MethodRef   = 0x33,
    ModuleRef   = 0x34,
}
```

### 5.3 Constant Formats

#### Null

```
[0x00]
Size: 1 byte
```

#### Boolean

```
[0x01][value: u8]
Size: 2 bytes
Value: 0x00 = false, 0x01 = true
```

#### Integers

```
Int8:   [0x02][value: i8]         // 2 bytes
Int16:  [0x03][value: i16]        // 3 bytes
Int32:  [0x04][value: i32]        // 5 bytes
Int64:  [0x05][value: i64]        // 9 bytes
UInt8:  [0x06][value: u8]         // 2 bytes
UInt16: [0x07][value: u16]        // 3 bytes
UInt32: [0x08][value: u32]        // 5 bytes
UInt64: [0x09][value: u64]        // 9 bytes
```

#### Floats

```
Float32: [0x10][value: f32]       // 5 bytes (IEEE 754)
Float64: [0x11][value: f64]       // 9 bytes (IEEE 754)
```

#### Character

```
Char: [0x12][codepoint: u32]      // 5 bytes (Unicode scalar)
```

#### String

```
String: [0x20][length: u32][data: u8...]
Size: 5 + length bytes
Data: UTF-8 encoded, NOT null-terminated
```

#### Bytes

```
Bytes: [0x21][length: u32][data: u8...]
Size: 5 + length bytes
```

#### Type Reference

```
TypeRef: [0x30][type_index: u32]
Size: 5 bytes
References entry in Types section
```

#### Function Reference

```
FuncRef: [0x31][func_index: u32]
Size: 5 bytes
References entry in Code section
```

#### Field Reference

```
FieldRef: [0x32][type_index: u32][field_index: u16]
Size: 7 bytes
```

#### Method Reference

```
MethodRef: [0x33][type_index: u32][method_index: u16]
Size: 7 bytes
```

---

## 6. Code Section

### 6.1 Code Section Header

```rust
#[repr(C)]
struct CodeSectionHeader {
    /// Number of functions
    function_count: u32,
    /// Total bytecode size
    bytecode_size: u32,
}
```

### 6.2 Function Definition

```rust
#[repr(C)]
struct FunctionDef {
    /// Index of function name in string table
    name_index: u32,

    /// Index of function signature in type section
    signature_index: u32,

    /// Offset to bytecode (from code section start)
    code_offset: u32,

    /// Length of bytecode in bytes
    code_length: u32,

    /// Number of local variable slots
    local_count: u16,

    /// Maximum operand stack depth
    max_stack: u16,

    /// Function flags
    flags: u16,

    /// Number of exception handlers
    handler_count: u16,
}

#[repr(u16)]
struct FunctionFlags {
    PUBLIC: u16     = 0x0001,
    STATIC: u16     = 0x0002,
    NATIVE: u16     = 0x0004,
    ASYNC: u16      = 0x0008,
    VARARGS: u16    = 0x0010,
    ABSTRACT: u16   = 0x0020,
    FINAL: u16      = 0x0040,
}
```

### 6.3 Exception Handler

```rust
#[repr(C)]
struct ExceptionHandler {
    /// Start of try block (bytecode offset)
    try_start: u32,
    /// End of try block (exclusive)
    try_end: u32,
    /// Handler entry point (bytecode offset)
    handler_pc: u32,
    /// Caught exception type (0 = catch all)
    catch_type: u32,
}
```

### 6.4 Bytecode Format

Bytecode is a sequence of variable-length instructions:

```
┌─────────────────────────────────────────────────────────────────┐
│                    BYTECODE INSTRUCTION                          │
├─────────────────────────────────────────────────────────────────┤
│  ┌────────┐                                                      │
│  │ Opcode │  1-byte instruction opcode                          │
│  │ (1 B)  │                                                      │
│  └────────┘                                                      │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │              OPERANDS (0-N bytes)                            ││
│  │  Format depends on opcode:                                   ││
│  │  - No operand: 0 bytes                                       ││
│  │  - Byte operand: 1 byte (u8 or i8)                          ││
│  │  - Short operand: 2 bytes (u16 or i16)                      ││
│  │  - Int operand: 4 bytes (u32 or i32)                        ││
│  │  - Multiple operands: varies                                 ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

See **11-instruction-set.md** for complete opcode reference.

---

## 7. Type Section

### 7.1 Type Section Header

```rust
#[repr(C)]
struct TypeSectionHeader {
    /// Number of type definitions
    type_count: u32,
}
```

### 7.2 Type Definition

```rust
#[repr(C)]
struct TypeDef {
    /// Type kind
    kind: TypeKind,
    /// Name in string table
    name_index: u32,
    /// Flags
    flags: u16,
    /// Parent type index (for classes, 0xFFFF = none)
    parent_index: u16,
    /// Number of fields
    field_count: u16,
    /// Number of methods
    method_count: u16,
    /// Number of implemented interfaces
    interface_count: u16,
    /// Reserved
    reserved: u16,
}

#[repr(u8)]
enum TypeKind {
    Void        = 0x00,
    Bool        = 0x01,
    Int8        = 0x02,
    Int16       = 0x03,
    Int32       = 0x04,
    Int64       = 0x05,
    UInt8       = 0x06,
    UInt16      = 0x07,
    UInt32      = 0x08,
    UInt64      = 0x09,
    Float32     = 0x0A,
    Float64     = 0x0B,
    Char        = 0x0C,
    String      = 0x10,
    List        = 0x11,
    Map         = 0x12,
    Set         = 0x13,
    Optional    = 0x14,
    Function    = 0x15,
    Tuple       = 0x16,
    Class       = 0x20,
    Struct      = 0x21,
    Enum        = 0x22,
    Interface   = 0x23,
    TypeParam   = 0x30,
}
```

### 7.3 Field Definition

```rust
#[repr(C)]
struct FieldDef {
    /// Field name in string table
    name_index: u32,
    /// Field type index
    type_index: u32,
    /// Flags
    flags: u16,
    /// Offset in object (for structs/classes)
    offset: u16,
}

struct FieldFlags(u16);
impl FieldFlags {
    const PUBLIC: u16   = 0x0001;
    const PRIVATE: u16  = 0x0002;
    const MUTABLE: u16  = 0x0004;
    const STATIC: u16   = 0x0008;
}
```

### 7.4 Method Definition

```rust
#[repr(C)]
struct MethodDef {
    /// Method name in string table
    name_index: u32,
    /// Function index in code section
    function_index: u32,
    /// Flags (same as function flags)
    flags: u16,
    /// Virtual table slot (-1 if not virtual)
    vtable_slot: i16,
}
```

### 7.5 Generic Type Parameters

```rust
#[repr(C)]
struct GenericTypeDef {
    /// Base type definition
    base: TypeDef,
    /// Number of type parameters
    param_count: u8,
    /// Type parameter definitions follow
}

#[repr(C)]
struct TypeParam {
    /// Name in string table
    name_index: u32,
    /// Constraint type index (0 = unconstrained)
    constraint_index: u32,
}
```

---

## 8. Debug Section

### 8.1 Debug Line Numbers

Maps bytecode offsets to source locations:

```rust
#[repr(C)]
struct LineNumberEntry {
    /// Bytecode offset
    pc: u32,
    /// Source file index
    file_index: u16,
    /// Line number (1-based)
    line: u16,
    /// Column number (1-based)
    column: u16,
    /// Length of span
    length: u16,
}
```

### 8.2 Debug Local Variables

```rust
#[repr(C)]
struct LocalVarEntry {
    /// Variable name in string table
    name_index: u32,
    /// Variable type index
    type_index: u32,
    /// Local slot index
    slot: u16,
    /// Start PC (where variable is in scope)
    start_pc: u32,
    /// End PC (exclusive)
    end_pc: u32,
}
```

### 8.3 Source File References

```rust
#[repr(C)]
struct SourceFileEntry {
    /// File path in string table
    path_index: u32,
    /// Source hash (for verification)
    hash: u32,
    /// Modification timestamp
    timestamp: u64,
}
```

---

## 9. Validation

### 9.1 Header Validation

1. Magic number must be 0x0043694C
2. Version must be supported
3. File size must match header
4. Checksum must verify
5. Section count must be > 0

### 9.2 Section Validation

1. All required sections present
2. Section offsets within file bounds
3. Section sizes don't overlap
4. Alignments are powers of 2

### 9.3 Constant Pool Validation

1. All indices within bounds
2. String data is valid UTF-8
3. References point to valid entries

### 9.4 Code Validation

1. All opcodes are valid
2. Operands are within bounds
3. Jump targets are valid
4. Stack depth is balanced
5. Type safety (if enabled)

### 9.5 Type Validation

1. All type references valid
2. Inheritance chains acyclic
3. Interface implementations complete
4. Generic constraints satisfied

---

## Appendix A: File Example

```
Offset  | Bytes                          | Description
--------|--------------------------------|------------------
0x0000  | 4C 69 43 00                    | Magic: "LiC\0"
0x0004  | 00 00 01 00                    | Version: 1.0
0x0008  | 01 00 00 00                    | Flags: DEBUG_INFO
0x000C  | 04 00 00 00                    | Section count: 4
0x0010  | 00 00 00 00                    | Entry point: 0
0x0014  | XX XX XX XX                    | File size
0x0018  | XX XX XX XX                    | Checksum
0x001C  | 00 00 00 00                    | Reserved

0x0020  | Section table (4 × 24 = 96 bytes)
        | ... (ConstPool, Code, Types, Strings)

0x0080  | Constant pool data
0x0XXX  | Code section data
0x0XXX  | Types section data
0x0XXX  | Strings section data
```

---

## Appendix B: String Table Format

```rust
#[repr(C)]
struct StringTableHeader {
    /// Number of strings
    count: u32,
    /// Total size of string data
    data_size: u32,
}

// Followed by count × StringEntry
#[repr(C)]
struct StringEntry {
    /// Offset into data area
    offset: u32,
    /// String length in bytes
    length: u32,
}

// Followed by packed string data (UTF-8, no null terminators)
```

---

_This document is part of the Lira Language Specification._
