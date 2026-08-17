# Lira VM Instruction Set Specification

## Document Information

| Property | Value |
| --- | --- |
| Document ID | 11-instruction-set |
| Status | Executable bytecode reference |
| Authority | `crates/lira-core/src/opcode.rs` |
| Prerequisites | `docs/10-bytecode-format.md` |

## Overview

Lira bytecode is a stack-machine instruction stream. Every instruction begins
with one opcode byte; operands immediately follow in little-endian form. The
defined bytes are exactly the `Opcode` values in `lira-core`; all unlisted bytes
are reserved and a bytecode loader must reject them.

Stack effects use `( before -- after )`, with the stack top on the right.
`value` means a Lira runtime value, and `count`, `index`, and `offset` mean
integer values unless an inline operand type is shown.

### Opcode ranges

| Range | Category |
| --- | --- |
| `0x00`–`0x04` | Stack and constants |
| `0x10`–`0x11` | Locals |
| `0x20`–`0x26` | Dynamic arithmetic |
| `0x30`–`0x35` | Dynamic comparisons |
| `0x40`–`0x49` | Logical and bitwise operations |
| `0x50`–`0x52` | Control flow |
| `0x60`–`0x62` | Calls and return |
| `0x70`–`0x75` | Objects, structs, and interfaces |
| `0x80`–`0x87` | Arrays and tuples |
| `0x90`–`0x92` | Fibers |
| `0xA0`–`0xA5` | Channels and select |
| `0xB0`–`0xB1` | Closures |
| `0xC0`–`0xC2` | Runtime type operations |
| `0xD0`–`0xDB` | Typed arithmetic |
| `0xE0`–`0xEB` | Typed comparisons |
| `0xF0`–`0xFF` | System operations |

## Instruction summary

### Stack and locals

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0x00` | `Nop` | — | `( -- )` |
| `0x01` | `LoadConst` | `constant_index: u16` | `( -- value )` |
| `0x02` | `Pop` | — | `( value -- )` |
| `0x03` | `Dup` | — | `( value -- value value )` |
| `0x04` | `CopyValue` | — | `( value -- copied-value )` |
| `0x10` | `LoadLocal` | `slot: u16` | `( -- value )` |
| `0x11` | `StoreLocal` | `slot: u16` | `( value -- )` |

`CopyValue` follows Lira's value/reference semantics: structs and tuples are
recursively copied, while reference-semantic values retain their handles.

### Dynamic arithmetic, comparison, logical, and bitwise operations

| Bytes | Opcodes | Operands | Stack effect |
| --- | --- | --- | --- |
| `0x20`–`0x24` | `Add`, `Sub`, `Mul`, `Div`, `Mod` | — | `( left right -- result )` |
| `0x25`–`0x26` | `Neg`, `Pow` | — | `( value -- result )` for `Neg`; `( left right -- result )` for `Pow` |
| `0x30`–`0x35` | `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge` | — | `( left right -- bool )` |
| `0x40`–`0x41` | `And`, `Or` | — | `( left right -- bool )` |
| `0x42` | `Not` | — | `( value -- bool )` |
| `0x43`–`0x45` | `BitAnd`, `BitOr`, `BitXor` | — | `( left right -- int )` |
| `0x46` | `BitNot` | — | `( int -- int )` |
| `0x47`–`0x49` | `Shl`, `Shr`, `UShr` | — | `( int count -- int )` |

### Control flow and calls

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0x50` | `Jump` | `relative_offset: i16` | `( -- )` |
| `0x51` | `JumpIfTrue` | `relative_offset: i16` | `( condition -- )` |
| `0x52` | `JumpIfFalse` | `relative_offset: i16` | `( condition -- )` |
| `0x60` | `Call` | `arg_count: u8` | `( args... callee -- result )` |
| `0x61` | `Return` | — | `( result -- )` |
| `0x62` | `TailCall` | `arg_count: u8` | `( args... callee -- result )` |

Jump offsets are relative to the instruction pointer after the `i16` operand.
`TailCall` replaces the current call frame; its eventual `Return` resumes the
current frame's caller.

### Objects, arrays, and tuples

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0x70` | `GetField` | `field_name_constant: u16` | `( object -- value )` |
| `0x71` | `SetField` | `field_name_constant: u16` | `( object value -- )` |
| `0x72` | `NewObject` | — | `( -- object )` |
| `0x73` | `NewStruct` | — | `( -- struct )` |
| `0x74` | `InterfaceBox` | see below | `( receiver -- interface )` |
| `0x75` | `InterfaceCall` | `method_name_constant: u16`, `arg_count: u8` | `( interface args... -- result )` |
| `0x80` | `NewArray` | — | `( length -- array )` |
| `0x81` | `ArrayGet` | — | `( collection index -- value )` |
| `0x82` | `ArraySet` | — | `( array index value -- )` |
| `0x83` | `ArrayLen` | — | `( collection -- length )` |
| `0x84` | `ArrayPush` | — | `( array value -- )` |
| `0x85` | `ArrayPop` | — | `( array -- value-or-null )` |
| `0x86` | `NewTuple` | — | `( length -- tuple )` |
| `0x87` | `TupleSet` | — | `( tuple index value -- )` |

`NewArray` creates an array initialized with `null`. `ArrayGet` reads arrays,
tuples, strings, and object keys as supported by the VM; `ArraySet`,
`ArrayPush`, and `ArrayPop` require an array. `NewTuple` creates a fixed-size
tuple under construction. `TupleSet` initializes its elements in ascending
index order and is invalid after construction completes.

`InterfaceBox` carries its witness inline, so it does not add a descriptor table
to the `.lic` file. Its operands are:

```text
flags: u8                 // bit 0: recursively copy a struct receiver
method_count: u8
repeat method_count times:
    method_name_constant: u16
    witness_kind: u8
    [function_code_offset: u16]  // only for kind 1
    [intrinsic_id: u8]           // only for kind 2
```

Witness kinds are `0` (look up a callable method on an object/struct, or reuse
the method from an existing interface), `1` (a direct bytecode function at the
following `u16` code offset), `2` (the following intrinsic: `0` string `len`,
`1` array `len`, `2` array `push`, `3` array `pop`), and `3` (resolve a method
from an erased receiver at runtime). The VM stores the receiver and resolved
methods in `Value::Interface`; missing or non-callable methods are runtime
errors.

`InterfaceCall` consumes the interface followed by its explicit arguments. It
looks up the named method in the witness, prepends the stored receiver, and
dispatches either a bytecode function/closure or one of the four intrinsics.
Its `arg_count` excludes the implicit receiver.

### Fibers, channels, and closures

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0x90` | `Spawn` | `code_offset: u16`, `arg_count: u8` | `( args... -- fiber )` |
| `0x91` | `Yield` | — | `( -- )` |
| `0x92` | `FiberId` | — | `( -- int )` |
| `0xA0` | `ChanNew` | — | `( capacity -- channel )` |
| `0xA1` | `ChanSend` | — | `( channel value -- )` |
| `0xA2` | `ChanRecv` | — | `( channel -- value open )` |
| `0xA3` | `ChanClose` | — | `( channel -- )` |
| `0xA5` | `Select` | see below | `( arm-operands... -- selected-arm-result? )` |
| `0xB0` | `MakeClosure` | `code_offset: u16`, `capture_count: u8` | `( captures... -- closure )` |
| `0xB1` | `LoadCapture` | `capture_index: u8` | `( -- value )` |

`Select` is encoded as `u8 arm_count`, followed by one `u8` tag per arm and one
`i16` body-relative offset per arm. Tags are `0` (receive), `1` (send), and
`2` (default). Code generation places arm operands on the stack in arm order:
a receive arm contributes its channel, a send arm contributes channel then
value, and a default arm contributes nothing. The VM deterministically chooses
among ready arms, runs a default when none is ready, or parks the fiber until an
arm becomes ready.

### Type operations

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0xC0` | `TypeIs` | `type_id: u8` | `( value -- bool )` |
| `0xC1` | `Cast` | `type_id: u8` | `( value -- result )` |
| `0xC2` | `InterfaceIs` | see below | `( value -- bool )` |

`TypeIs` type IDs are: `0` null, `1` bool, `2` int, `3` float, `4` string,
`5` array, `6` object or struct, `7` function or closure, `8` tuple, and `9`
channel. `10` is an interface value (`Value::Interface`). `Cast` converts primitive targets (`bool`, `int`, `float`, and
`string`) and leaves other runtime values unchanged.

`InterfaceIs` carries a bounded inline structural witness query:

```text
source_type_id: u8          // 0xff means erased/unknown
method_count: u8
repeat method_count times:
    method_name_constant: u16
    witness_kind: u8
    [intrinsic_id: u8]       // only for kind 2
```

For this query, kind `0` checks a callable method on an interface/object/struct,
kind `1` checks a statically known implementation, kind `2` checks intrinsic
`0` string `len` or `1..3` array `len`/`push`/`pop`, and kind `3` performs the
erased runtime lookup. All listed methods must match. `InterfaceIs` is a
structural membership test; it does not compare a nominal interface name.

### Typed arithmetic and comparison

| Bytes | Opcodes | Operands | Stack effect |
| --- | --- | --- | --- |
| `0xD0`–`0xD4` | `IAdd`, `ISub`, `IMul`, `IDiv`, `IMod` | — | `( int int -- int )` |
| `0xD5` | `INeg` | — | `( int -- int )` |
| `0xD6`–`0xDA` | `FAdd`, `FSub`, `FMul`, `FDiv`, `FMod` | — | `( float float -- float )` |
| `0xDB` | `FNeg` | — | `( float -- float )` |
| `0xE0`–`0xE5` | `IEq`, `INe`, `ILt`, `ILe`, `IGt`, `IGe` | — | `( int int -- bool )` |
| `0xE6`–`0xEB` | `FEq`, `FNe`, `FLt`, `FLe`, `FGt`, `FGe` | — | `( float float -- bool )` |

### System operations

| Byte | Opcode | Operands | Stack effect |
| --- | --- | --- | --- |
| `0xF0` | `Print` | — | `( value -- )` |
| `0xF1` | `Println` | — | `( value -- )` |
| `0xF2` | `Assert` | — | `( bool -- )` |
| `0xFD` | `Collect` | — | `( -- null )` |
| `0xFE` | `Syscall` | `syscall_number: u8` | implementation-defined |
| `0xFF` | `Halt` | — | terminates execution with success |

`Println` appends one newline. `Assert` rejects `false` and non-boolean values.
`Collect` forces a garbage-collection cycle and pushes `null`. The runtime
dispatches `Syscall` by its one-byte encoded number; syscall numbers are not
opcode numbers and are defined by the runtime implementation. `Halt` is the
defined `0xff` success terminator, not an illegal-opcode sentinel.

## Reserved bytes

All bytes not listed above are reserved. In particular, `0xA4` is reserved for
the planned non-blocking receive operation and is currently undefined. `0xFF`
is `Halt`, not an illegal-opcode sentinel.
