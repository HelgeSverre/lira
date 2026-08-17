# Lira Bytecode Format

This document describes the `.lic` format emitted by `lirac` and consumed by
`liravm`. The executable definition is the serializer in
`crates/lirac/src/codegen.rs` and the reader in `crates/liravm/src/bytecode.rs`.
All integer fields are little-endian. There is no section table: the file is a
single sequential stream.

## File layout

```text
header (24 bytes)
constant pool (constant_count entries)
code length (u32), then code bytes
debug line table
source filename
[local symbols, when bytes remain]
[function symbols, when bytes remain]
```

The optional symbol records are length-delimited, but the current loader treats
any bytes remaining after the filename as the local-symbol section and any
bytes remaining after that as the function-symbol section. A producer should
therefore emit both records, including a zero count, when writing debug data.

## Header (24 bytes)

| Offset | Size | Field | Meaning |
| ---: | ---: | --- | --- |
| 0 | 4 | `magic: u32` | `BYTECODE_MAGIC`, the bytes `LiC\0` (`0x0043694c` as little-endian `u32`) |
| 4 | 4 | `version: u32` | Exact format generation; current value is `2` |
| 8 | 4 | `flags: u32` | Currently written as zero and ignored by the loader |
| 12 | 4 | `entry_point: u32` | Entry bytecode offset; current compiler writes zero |
| 16 | 4 | `constant_count: u32` | Number of constant records immediately following |
| 20 | 4 | `function_count: u32` | Compiler function count; the current loader validates no function table and leaves `Program.functions` empty |

The reader rejects a short header, a wrong magic, or any version other than
the exact current `BYTECODE_VERSION` (2). It caps the constant count at 65,536
and the code length at 10,000,000 bytes before allocating those structures.

## Constant pool

Each entry starts with one tag byte. The payload is:

| Tag | Constant | Payload |
| ---: | --- | --- |
| `0x00` | `Null` | none |
| `0x01` | `Bool` | `u8` (`0` is false; any nonzero value is true) |
| `0x02` | `Int` | signed `i64` |
| `0x03` | `Float` | IEEE-754 `f64` bit pattern |
| `0x04` | `String` | `u32` byte length followed by UTF-8 bytes (not NUL-terminated) |
| `0x05` | `Function` | `i64`-width code offset, read as `usize` |

There are no separate integer widths, type tables, import/export sections, or
relocation records in the current `.lic` format.

## Code stream

The pool is followed by `code_len: u32` and exactly `code_len` instruction
bytes. Each instruction begins with one opcode byte and has the inline operands
specified by [the instruction set](11-instruction-set.md). Operands are
little-endian. The compiler emits a terminal `Halt` byte (`0xff`) after the
top-level code. Unknown opcode bytes are rejected by opcode decoding rather
than interpreted as another instruction.

## Debug information

After code, the writer emits a line table:

```text
line_count: u32
repeat line_count times:
    start_offset: u32
    end_offset: u32       // exclusive
    line: u32             // 1-based
    column: u32            // 1-based
```

Then it emits the source filename:

```text
filename_len: u32
filename: [u8; filename_len]   // UTF-8; omitted when length is zero
```

If present, local symbols follow:

```text
symbol_count: u32
repeat symbol_count times:
    slot: u16
    scope_depth: u16
    start_offset: u32
    end_offset: u32             // zero means function end
    name_len: u32
    name: [u8; name_len]        // UTF-8
```

Function symbols are the final optional record:

```text
function_symbol_count: u32
repeat function_symbol_count times:
    code_offset: u32
    name_len: u32
    name: [u8; name_len]        // UTF-8
```

The VM uses these tables for source locations, local-variable names, and
human-readable call frames. Debug information does not change execution.

## Validation and compatibility

The reader bounds every length before slicing the input and rejects truncated
UTF-8 strings, unknown constant tags, excessive constant/code sizes, bad magic,
and unsupported versions. It does not verify that every code byte is reachable
or that `function_count` agrees with a serialized function table: there is no
such table in this format. A `.lic` file is therefore portable only between
readers implementing this exact sequential version-2 format.
