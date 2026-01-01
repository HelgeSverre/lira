# Lira VM Instruction Set Specification

## Document Information

| Property          | Value               |
| ----------------- | ------------------- |
| **Document ID**   | 11-instruction-set  |
| **Version**       | 1.0.0-draft         |
| **Status**        | Draft Specification |
| **Prerequisites** | 10-bytecode-format  |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Instruction Encoding](#2-instruction-encoding)
3. [Stack Operations](#3-stack-operations)
4. [Local Variables](#4-local-variables)
5. [Constants](#5-constants)
6. [Arithmetic Operations](#6-arithmetic-operations)
7. [Bitwise Operations](#7-bitwise-operations)
8. [Comparison Operations](#8-comparison-operations)
9. [Type Conversion](#9-type-conversion)
10. [Control Flow](#10-control-flow)
11. [Function Operations](#11-function-operations)
12. [Object Operations](#12-object-operations)
13. [Array Operations](#13-array-operations)
14. [String Operations](#14-string-operations)
15. [Reference Counting](#15-reference-counting)
16. [Channel Operations](#16-channel-operations)
17. [Fiber Operations](#17-fiber-operations)
18. [Syscall Operations](#18-syscall-operations)
19. [Miscellaneous](#19-miscellaneous)
20. [Opcode Summary Table](#20-opcode-summary-table)

---

## 1. Overview

### 1.1 Architecture

The Lira VM is a **stack-based virtual machine** with the following characteristics:

- **Operand Stack**: Per-frame stack for intermediate values
- **Local Variables**: Indexed array per call frame
- **Constant Pool**: Shared pool of constants per module
- **Heap**: Garbage-collected memory for objects

### 1.2 Value Types

All values on the stack and in locals are 64-bit using NaN-boxing:

```
┌─────────────────────────────────────────────────────────────────┐
│                     VALUE REPRESENTATION                         │
├─────────────────────────────────────────────────────────────────┤
│  Float64:  Actual IEEE 754 double (not a NaN)                   │
│  Pointer:  0x0000_XXXX_XXXX_XXXX (48-bit pointer)              │
│  Integer:  0x7FF8_0001_XXXX_XXXX (tagged 32-bit int)           │
│  Boolean:  0x7FF8_0002_0000_000X (X = 0 or 1)                  │
│  Null:     0x7FF8_0003_0000_0000                                │
│  Char:     0x7FF8_0004_00XX_XXXX (Unicode scalar)              │
└─────────────────────────────────────────────────────────────────┘
```

### 1.3 Notation

Stack effects are shown as:

```
( before -- after )
```

Where `before` shows stack state before instruction, `after` shows after.
Stack top is rightmost.

---

## 2. Instruction Encoding

### 2.1 Instruction Format

```
┌─────────────────────────────────────────────────────────────────┐
│                    INSTRUCTION FORMAT                            │
├─────────────────────────────────────────────────────────────────┤
│  ┌────────┐ ┌──────────────────────────────────────────────────┐│
│  │ Opcode │ │              Operands (0-8 bytes)                ││
│  │ (1 B)  │ │  Format depends on opcode                        ││
│  └────────┘ └──────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Operand Types

| Type  | Size | Description                     |
| ----- | ---- | ------------------------------- |
| `u8`  | 1    | Unsigned 8-bit                  |
| `i8`  | 1    | Signed 8-bit                    |
| `u16` | 2    | Unsigned 16-bit (little-endian) |
| `i16` | 2    | Signed 16-bit (little-endian)   |
| `u32` | 4    | Unsigned 32-bit (little-endian) |
| `i32` | 4    | Signed 32-bit (little-endian)   |

### 2.3 Opcode Ranges

| Range     | Category            |
| --------- | ------------------- |
| 0x00-0x0F | Stack Operations    |
| 0x10-0x1F | Local Variables     |
| 0x20-0x2F | Constants           |
| 0x30-0x4F | Arithmetic          |
| 0x50-0x5F | Bitwise             |
| 0x60-0x6F | Comparison          |
| 0x70-0x7F | Type Conversion     |
| 0x80-0x8F | Control Flow        |
| 0x90-0x9F | Function Operations |
| 0xA0-0xAF | Object Operations   |
| 0xB0-0xBF | Array Operations    |
| 0xC0-0xCF | String Operations   |
| 0xD0-0xD7 | Reference Counting  |
| 0xD8-0xDF | Channel Operations  |
| 0xE0-0xE7 | Fiber Operations    |
| 0xE8-0xEF | Syscall Operations  |
| 0xF0-0xFF | Miscellaneous       |

---

## 3. Stack Operations

### 3.1 NOP (0x00)

No operation.

```
Opcode:  0x00
Operands: None
Stack:   ( -- )
```

### 3.2 POP (0x01)

Pop and discard top of stack.

```
Opcode:  0x01
Operands: None
Stack:   ( value -- )
```

### 3.3 POP2 (0x02)

Pop and discard top two values.

```
Opcode:  0x02
Operands: None
Stack:   ( value1 value2 -- )
```

### 3.4 DUP (0x03)

Duplicate top of stack.

```
Opcode:  0x03
Operands: None
Stack:   ( value -- value value )
```

### 3.5 DUP2 (0x04)

Duplicate top two values.

```
Opcode:  0x04
Operands: None
Stack:   ( v1 v2 -- v1 v2 v1 v2 )
```

### 3.6 DUP_X1 (0x05)

Duplicate top and insert below second.

```
Opcode:  0x05
Operands: None
Stack:   ( v1 v2 -- v2 v1 v2 )
```

### 3.7 DUP_X2 (0x06)

Duplicate top and insert below third.

```
Opcode:  0x06
Operands: None
Stack:   ( v1 v2 v3 -- v3 v1 v2 v3 )
```

### 3.8 SWAP (0x07)

Swap top two values.

```
Opcode:  0x07
Operands: None
Stack:   ( v1 v2 -- v2 v1 )
```

### 3.9 ROT (0x08)

Rotate top three values.

```
Opcode:  0x08
Operands: None
Stack:   ( v1 v2 v3 -- v2 v3 v1 )
```

### 3.10 OVER (0x09)

Copy second value to top.

```
Opcode:  0x09
Operands: None
Stack:   ( v1 v2 -- v1 v2 v1 )
```

---

## 4. Local Variables

### 4.1 LOAD (0x10)

Load local variable onto stack.

```
Opcode:  0x10
Operands: index: u8
Stack:   ( -- value )
```

### 4.2 LOAD_W (0x11)

Load local variable (wide index).

```
Opcode:  0x11
Operands: index: u16
Stack:   ( -- value )
```

### 4.3 LOAD_0 (0x12)

Load local variable 0.

```
Opcode:  0x12
Operands: None
Stack:   ( -- value )
```

### 4.4 LOAD_1 (0x13)

Load local variable 1.

```
Opcode:  0x13
Operands: None
Stack:   ( -- value )
```

### 4.5 LOAD_2 (0x14)

Load local variable 2.

```
Opcode:  0x14
Operands: None
Stack:   ( -- value )
```

### 4.6 LOAD_3 (0x15)

Load local variable 3.

```
Opcode:  0x15
Operands: None
Stack:   ( -- value )
```

### 4.7 STORE (0x16)

Store top of stack into local variable.

```
Opcode:  0x16
Operands: index: u8
Stack:   ( value -- )
```

### 4.8 STORE_W (0x17)

Store into local variable (wide index).

```
Opcode:  0x17
Operands: index: u16
Stack:   ( value -- )
```

### 4.9 STORE_0 (0x18)

Store into local variable 0.

```
Opcode:  0x18
Operands: None
Stack:   ( value -- )
```

### 4.10 STORE_1 (0x19)

Store into local variable 1.

```
Opcode:  0x19
Operands: None
Stack:   ( value -- )
```

### 4.11 STORE_2 (0x1A)

Store into local variable 2.

```
Opcode:  0x1A
Operands: None
Stack:   ( value -- )
```

### 4.12 STORE_3 (0x1B)

Store into local variable 3.

```
Opcode:  0x1B
Operands: None
Stack:   ( value -- )
```

### 4.13 IINC (0x1C)

Increment local integer variable.

```
Opcode:  0x1C
Operands: index: u8, delta: i8
Stack:   ( -- )
Effect:  locals[index] += delta
```

### 4.14 IINC_W (0x1D)

Increment local integer variable (wide).

```
Opcode:  0x1D
Operands: index: u16, delta: i16
Stack:   ( -- )
Effect:  locals[index] += delta
```

---

## 5. Constants

### 5.1 CONST_NULL (0x20)

Push null onto stack.

```
Opcode:  0x20
Operands: None
Stack:   ( -- null )
```

### 5.2 CONST_TRUE (0x21)

Push true onto stack.

```
Opcode:  0x21
Operands: None
Stack:   ( -- true )
```

### 5.3 CONST_FALSE (0x22)

Push false onto stack.

```
Opcode:  0x22
Operands: None
Stack:   ( -- false )
```

### 5.4 CONST_I0 (0x23)

Push integer 0.

```
Opcode:  0x23
Operands: None
Stack:   ( -- 0 )
```

### 5.5 CONST_I1 (0x24)

Push integer 1.

```
Opcode:  0x24
Operands: None
Stack:   ( -- 1 )
```

### 5.6 CONST_I2 (0x25)

Push integer 2.

```
Opcode:  0x25
Operands: None
Stack:   ( -- 2 )
```

### 5.7 CONST_IM1 (0x26)

Push integer -1.

```
Opcode:  0x26
Operands: None
Stack:   ( -- -1 )
```

### 5.8 CONST_F0 (0x27)

Push float 0.0.

```
Opcode:  0x27
Operands: None
Stack:   ( -- 0.0 )
```

### 5.9 CONST_F1 (0x28)

Push float 1.0.

```
Opcode:  0x28
Operands: None
Stack:   ( -- 1.0 )
```

### 5.10 BIPUSH (0x29)

Push byte as integer.

```
Opcode:  0x29
Operands: value: i8
Stack:   ( -- int )
```

### 5.11 SIPUSH (0x2A)

Push short as integer.

```
Opcode:  0x2A
Operands: value: i16
Stack:   ( -- int )
```

### 5.12 LDC (0x2B)

Load constant from pool.

```
Opcode:  0x2B
Operands: index: u8
Stack:   ( -- const )
```

### 5.13 LDC_W (0x2C)

Load constant from pool (wide index).

```
Opcode:  0x2C
Operands: index: u16
Stack:   ( -- const )
```

### 5.14 LDC2_W (0x2D)

Load 64-bit constant from pool.

```
Opcode:  0x2D
Operands: index: u16
Stack:   ( -- const64 )
Note:    For Float64 and Int64 constants
```

---

## 6. Arithmetic Operations

### 6.1 Integer Arithmetic

#### IADD (0x30)

Integer addition.

```
Opcode:  0x30
Operands: None
Stack:   ( a b -- a+b )
```

#### ISUB (0x31)

Integer subtraction.

```
Opcode:  0x31
Operands: None
Stack:   ( a b -- a-b )
```

#### IMUL (0x32)

Integer multiplication.

```
Opcode:  0x32
Operands: None
Stack:   ( a b -- a*b )
```

#### IDIV (0x33)

Integer division.

```
Opcode:  0x33
Operands: None
Stack:   ( a b -- a/b )
Throws:  DivisionByZero if b == 0
```

#### IREM (0x34)

Integer remainder (modulo).

```
Opcode:  0x34
Operands: None
Stack:   ( a b -- a%b )
Throws:  DivisionByZero if b == 0
```

#### INEG (0x35)

Integer negation.

```
Opcode:  0x35
Operands: None
Stack:   ( a -- -a )
```

#### IABS (0x36)

Integer absolute value.

```
Opcode:  0x36
Operands: None
Stack:   ( a -- |a| )
```

#### IMIN (0x37)

Integer minimum.

```
Opcode:  0x37
Operands: None
Stack:   ( a b -- min(a,b) )
```

#### IMAX (0x38)

Integer maximum.

```
Opcode:  0x38
Operands: None
Stack:   ( a b -- max(a,b) )
```

### 6.2 Long Integer Arithmetic

#### LADD (0x39)

64-bit integer addition.

```
Opcode:  0x39
Operands: None
Stack:   ( a:i64 b:i64 -- a+b:i64 )
```

#### LSUB (0x3A)

64-bit integer subtraction.

```
Opcode:  0x3A
Operands: None
Stack:   ( a:i64 b:i64 -- a-b:i64 )
```

#### LMUL (0x3B)

64-bit integer multiplication.

```
Opcode:  0x3B
Operands: None
Stack:   ( a:i64 b:i64 -- a*b:i64 )
```

#### LDIV (0x3C)

64-bit integer division.

```
Opcode:  0x3C
Operands: None
Stack:   ( a:i64 b:i64 -- a/b:i64 )
Throws:  DivisionByZero if b == 0
```

#### LREM (0x3D)

64-bit integer remainder.

```
Opcode:  0x3D
Operands: None
Stack:   ( a:i64 b:i64 -- a%b:i64 )
```

#### LNEG (0x3E)

64-bit integer negation.

```
Opcode:  0x3E
Operands: None
Stack:   ( a:i64 -- -a:i64 )
```

### 6.3 Floating Point Arithmetic

#### FADD (0x40)

Float32 addition.

```
Opcode:  0x40
Operands: None
Stack:   ( a:f32 b:f32 -- a+b:f32 )
```

#### FSUB (0x41)

Float32 subtraction.

```
Opcode:  0x41
Operands: None
Stack:   ( a:f32 b:f32 -- a-b:f32 )
```

#### FMUL (0x42)

Float32 multiplication.

```
Opcode:  0x42
Operands: None
Stack:   ( a:f32 b:f32 -- a*b:f32 )
```

#### FDIV (0x43)

Float32 division.

```
Opcode:  0x43
Operands: None
Stack:   ( a:f32 b:f32 -- a/b:f32 )
```

#### FREM (0x44)

Float32 remainder.

```
Opcode:  0x44
Operands: None
Stack:   ( a:f32 b:f32 -- a%b:f32 )
```

#### FNEG (0x45)

Float32 negation.

```
Opcode:  0x45
Operands: None
Stack:   ( a:f32 -- -a:f32 )
```

### 6.4 Double Precision Arithmetic

#### DADD (0x46)

Float64 addition.

```
Opcode:  0x46
Operands: None
Stack:   ( a:f64 b:f64 -- a+b:f64 )
```

#### DSUB (0x47)

Float64 subtraction.

```
Opcode:  0x47
Operands: None
Stack:   ( a:f64 b:f64 -- a-b:f64 )
```

#### DMUL (0x48)

Float64 multiplication.

```
Opcode:  0x48
Operands: None
Stack:   ( a:f64 b:f64 -- a*b:f64 )
```

#### DDIV (0x49)

Float64 division.

```
Opcode:  0x49
Operands: None
Stack:   ( a:f64 b:f64 -- a/b:f64 )
```

#### DREM (0x4A)

Float64 remainder.

```
Opcode:  0x4A
Operands: None
Stack:   ( a:f64 b:f64 -- a%b:f64 )
```

#### DNEG (0x4B)

Float64 negation.

```
Opcode:  0x4B
Operands: None
Stack:   ( a:f64 -- -a:f64 )
```

#### DABS (0x4C)

Float64 absolute value.

```
Opcode:  0x4C
Operands: None
Stack:   ( a:f64 -- |a|:f64 )
```

#### DSQRT (0x4D)

Float64 square root.

```
Opcode:  0x4D
Operands: None
Stack:   ( a:f64 -- sqrt(a):f64 )
```

#### DMIN (0x4E)

Float64 minimum.

```
Opcode:  0x4E
Operands: None
Stack:   ( a:f64 b:f64 -- min(a,b):f64 )
```

#### DMAX (0x4F)

Float64 maximum.

```
Opcode:  0x4F
Operands: None
Stack:   ( a:f64 b:f64 -- max(a,b):f64 )
```

---

## 7. Bitwise Operations

### 7.1 IAND (0x50)

Integer bitwise AND.

```
Opcode:  0x50
Operands: None
Stack:   ( a b -- a&b )
```

### 7.2 IOR (0x51)

Integer bitwise OR.

```
Opcode:  0x51
Operands: None
Stack:   ( a b -- a|b )
```

### 7.3 IXOR (0x52)

Integer bitwise XOR.

```
Opcode:  0x52
Operands: None
Stack:   ( a b -- a^b )
```

### 7.4 INOT (0x53)

Integer bitwise NOT.

```
Opcode:  0x53
Operands: None
Stack:   ( a -- ~a )
```

### 7.5 ISHL (0x54)

Integer shift left.

```
Opcode:  0x54
Operands: None
Stack:   ( a count -- a<<count )
```

### 7.6 ISHR (0x55)

Integer arithmetic shift right (sign-extending).

```
Opcode:  0x55
Operands: None
Stack:   ( a count -- a>>count )
```

### 7.7 IUSHR (0x56)

Integer logical shift right (zero-extending).

```
Opcode:  0x56
Operands: None
Stack:   ( a count -- a>>>count )
```

### 7.8 LAND (0x57)

Long bitwise AND.

```
Opcode:  0x57
Operands: None
Stack:   ( a:i64 b:i64 -- a&b:i64 )
```

### 7.9 LOR (0x58)

Long bitwise OR.

```
Opcode:  0x58
Operands: None
Stack:   ( a:i64 b:i64 -- a|b:i64 )
```

### 7.10 LXOR (0x59)

Long bitwise XOR.

```
Opcode:  0x59
Operands: None
Stack:   ( a:i64 b:i64 -- a^b:i64 )
```

### 7.11 LNOT (0x5A)

Long bitwise NOT.

```
Opcode:  0x5A
Operands: None
Stack:   ( a:i64 -- ~a:i64 )
```

### 7.12 LSHL (0x5B)

Long shift left.

```
Opcode:  0x5B
Operands: None
Stack:   ( a:i64 count -- a<<count:i64 )
```

### 7.13 LSHR (0x5C)

Long arithmetic shift right.

```
Opcode:  0x5C
Operands: None
Stack:   ( a:i64 count -- a>>count:i64 )
```

### 7.14 LUSHR (0x5D)

Long logical shift right.

```
Opcode:  0x5D
Operands: None
Stack:   ( a:i64 count -- a>>>count:i64 )
```

---

## 8. Comparison Operations

### 8.1 Integer Comparisons

#### ICMP_EQ (0x60)

Integer equal.

```
Opcode:  0x60
Operands: None
Stack:   ( a b -- a==b:bool )
```

#### ICMP_NE (0x61)

Integer not equal.

```
Opcode:  0x61
Operands: None
Stack:   ( a b -- a!=b:bool )
```

#### ICMP_LT (0x62)

Integer less than.

```
Opcode:  0x62
Operands: None
Stack:   ( a b -- a<b:bool )
```

#### ICMP_LE (0x63)

Integer less than or equal.

```
Opcode:  0x63
Operands: None
Stack:   ( a b -- a<=b:bool )
```

#### ICMP_GT (0x64)

Integer greater than.

```
Opcode:  0x64
Operands: None
Stack:   ( a b -- a>b:bool )
```

#### ICMP_GE (0x65)

Integer greater than or equal.

```
Opcode:  0x65
Operands: None
Stack:   ( a b -- a>=b:bool )
```

### 8.2 Float Comparisons

#### DCMP_EQ (0x66)

Float64 equal.

```
Opcode:  0x66
Operands: None
Stack:   ( a:f64 b:f64 -- a==b:bool )
Note:    NaN != NaN
```

#### DCMP_NE (0x67)

Float64 not equal.

```
Opcode:  0x67
Operands: None
Stack:   ( a:f64 b:f64 -- a!=b:bool )
```

#### DCMP_LT (0x68)

Float64 less than.

```
Opcode:  0x68
Operands: None
Stack:   ( a:f64 b:f64 -- a<b:bool )
```

#### DCMP_LE (0x69)

Float64 less than or equal.

```
Opcode:  0x69
Operands: None
Stack:   ( a:f64 b:f64 -- a<=b:bool )
```

#### DCMP_GT (0x6A)

Float64 greater than.

```
Opcode:  0x6A
Operands: None
Stack:   ( a:f64 b:f64 -- a>b:bool )
```

#### DCMP_GE (0x6B)

Float64 greater than or equal.

```
Opcode:  0x6B
Operands: None
Stack:   ( a:f64 b:f64 -- a>=b:bool )
```

### 8.3 Reference Comparisons

#### REF_EQ (0x6C)

Reference identity equal.

```
Opcode:  0x6C
Operands: None
Stack:   ( ref1 ref2 -- ref1===ref2:bool )
```

#### REF_NE (0x6D)

Reference identity not equal.

```
Opcode:  0x6D
Operands: None
Stack:   ( ref1 ref2 -- ref1!==ref2:bool )
```

#### IS_NULL (0x6E)

Check if null.

```
Opcode:  0x6E
Operands: None
Stack:   ( value -- value==null:bool )
```

#### IS_NOT_NULL (0x6F)

Check if not null.

```
Opcode:  0x6F
Operands: None
Stack:   ( value -- value!=null:bool )
```

---

## 9. Type Conversion

### 9.1 Integer Conversions

#### I2L (0x70)

Int32 to Int64.

```
Opcode:  0x70
Operands: None
Stack:   ( i:i32 -- i:i64 )
```

#### I2F (0x71)

Int32 to Float32.

```
Opcode:  0x71
Operands: None
Stack:   ( i:i32 -- f:f32 )
```

#### I2D (0x72)

Int32 to Float64.

```
Opcode:  0x72
Operands: None
Stack:   ( i:i32 -- d:f64 )
```

#### L2I (0x73)

Int64 to Int32 (truncate).

```
Opcode:  0x73
Operands: None
Stack:   ( l:i64 -- i:i32 )
```

#### L2F (0x74)

Int64 to Float32.

```
Opcode:  0x74
Operands: None
Stack:   ( l:i64 -- f:f32 )
```

#### L2D (0x75)

Int64 to Float64.

```
Opcode:  0x75
Operands: None
Stack:   ( l:i64 -- d:f64 )
```

### 9.2 Float Conversions

#### F2I (0x76)

Float32 to Int32.

```
Opcode:  0x76
Operands: None
Stack:   ( f:f32 -- i:i32 )
Note:    Truncates toward zero
```

#### F2L (0x77)

Float32 to Int64.

```
Opcode:  0x77
Operands: None
Stack:   ( f:f32 -- l:i64 )
```

#### F2D (0x78)

Float32 to Float64.

```
Opcode:  0x78
Operands: None
Stack:   ( f:f32 -- d:f64 )
```

#### D2I (0x79)

Float64 to Int32.

```
Opcode:  0x79
Operands: None
Stack:   ( d:f64 -- i:i32 )
```

#### D2L (0x7A)

Float64 to Int64.

```
Opcode:  0x7A
Operands: None
Stack:   ( d:f64 -- l:i64 )
```

#### D2F (0x7B)

Float64 to Float32.

```
Opcode:  0x7B
Operands: None
Stack:   ( d:f64 -- f:f32 )
```

### 9.3 Byte Conversions

#### I2B (0x7C)

Int32 to Int8 (truncate).

```
Opcode:  0x7C
Operands: None
Stack:   ( i:i32 -- b:i8 )
```

#### I2S (0x7D)

Int32 to Int16 (truncate).

```
Opcode:  0x7D
Operands: None
Stack:   ( i:i32 -- s:i16 )
```

#### I2C (0x7E)

Int32 to Char (Unicode scalar).

```
Opcode:  0x7E
Operands: None
Stack:   ( i:i32 -- c:char )
Throws:  InvalidChar if not valid Unicode scalar
```

---

## 10. Control Flow

### 10.1 Unconditional Jumps

#### GOTO (0x80)

Unconditional jump.

```
Opcode:  0x80
Operands: offset: i16
Stack:   ( -- )
Effect:  pc += offset
```

#### GOTO_W (0x81)

Unconditional jump (wide).

```
Opcode:  0x81
Operands: offset: i32
Stack:   ( -- )
Effect:  pc += offset
```

### 10.2 Conditional Jumps

#### IF_TRUE (0x82)

Jump if true.

```
Opcode:  0x82
Operands: offset: i16
Stack:   ( cond:bool -- )
Effect:  if cond: pc += offset
```

#### IF_FALSE (0x83)

Jump if false.

```
Opcode:  0x83
Operands: offset: i16
Stack:   ( cond:bool -- )
Effect:  if !cond: pc += offset
```

#### IF_NULL (0x84)

Jump if null.

```
Opcode:  0x84
Operands: offset: i16
Stack:   ( value -- )
Effect:  if value == null: pc += offset
```

#### IF_NOT_NULL (0x85)

Jump if not null.

```
Opcode:  0x85
Operands: offset: i16
Stack:   ( value -- )
Effect:  if value != null: pc += offset
```

### 10.3 Integer Conditional Jumps

#### IF_ICMP_EQ (0x86)

Jump if integers equal.

```
Opcode:  0x86
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a == b: pc += offset
```

#### IF_ICMP_NE (0x87)

Jump if integers not equal.

```
Opcode:  0x87
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a != b: pc += offset
```

#### IF_ICMP_LT (0x88)

Jump if a < b.

```
Opcode:  0x88
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a < b: pc += offset
```

#### IF_ICMP_LE (0x89)

Jump if a <= b.

```
Opcode:  0x89
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a <= b: pc += offset
```

#### IF_ICMP_GT (0x8A)

Jump if a > b.

```
Opcode:  0x8A
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a > b: pc += offset
```

#### IF_ICMP_GE (0x8B)

Jump if a >= b.

```
Opcode:  0x8B
Operands: offset: i16
Stack:   ( a b -- )
Effect:  if a >= b: pc += offset
```

### 10.4 Switch

#### TABLESWITCH (0x8C)

Table-based switch.

```
Opcode:  0x8C
Operands: padding (0-3 bytes for alignment)
          default_offset: i32
          low: i32
          high: i32
          offsets: i32[high - low + 1]
Stack:   ( index -- )
Effect:  Jump to offsets[index - low] or default_offset
```

#### LOOKUPSWITCH (0x8D)

Lookup-based switch.

```
Opcode:  0x8D
Operands: padding (0-3 bytes for alignment)
          default_offset: i32
          npairs: u32
          pairs: (match: i32, offset: i32)[npairs]
Stack:   ( key -- )
Effect:  Binary search for key, jump to offset or default
```

---

## 11. Function Operations

### 11.1 INVOKE (0x90)

Invoke function by index.

```
Opcode:  0x90
Operands: func_index: u16
Stack:   ( args... -- result )
Effect:  Call function with args, push return value
```

### 11.2 INVOKE_VIRTUAL (0x91)

Invoke virtual method.

```
Opcode:  0x91
Operands: method_index: u16
Stack:   ( receiver args... -- result )
Effect:  Look up method in receiver's vtable, call
```

### 11.3 INVOKE_INTERFACE (0x92)

Invoke interface method.

```
Opcode:  0x92
Operands: interface_index: u16, method_index: u8
Stack:   ( receiver args... -- result )
Effect:  Look up method in interface table, call
```

### 11.4 INVOKE_STATIC (0x93)

Invoke static method.

```
Opcode:  0x93
Operands: class_index: u16, method_index: u16
Stack:   ( args... -- result )
Effect:  Call static method
```

### 11.5 INVOKE_SPECIAL (0x94)

Invoke specific method (constructors, super calls).

```
Opcode:  0x94
Operands: method_ref: u16
Stack:   ( receiver args... -- result )
Effect:  Call exact method (no virtual dispatch)
```

### 11.6 INVOKE_DYNAMIC (0x95)

Invoke dynamically resolved method.

```
Opcode:  0x95
Operands: call_site_index: u16
Stack:   ( target args... -- result )
Effect:  Resolve and call at runtime
```

### 11.7 RETURN (0x96)

Return from function.

```
Opcode:  0x96
Operands: None
Stack:   ( value -- )
Effect:  Pop frame, push value to caller
```

### 11.8 RETURN_VOID (0x97)

Return void from function.

```
Opcode:  0x97
Operands: None
Stack:   ( -- )
Effect:  Pop frame
```

### 11.9 TAILCALL (0x98)

Tail call optimization.

```
Opcode:  0x98
Operands: func_index: u16
Stack:   ( args... -- )
Effect:  Replace current frame, jump to function
```

---

## 12. Object Operations

### 12.1 NEW (0xA0)

Allocate new object.

```
Opcode:  0xA0
Operands: type_index: u16
Stack:   ( -- obj )
Effect:  Allocate object of type, push reference
```

### 12.2 GET_FIELD (0xA1)

Get instance field.

```
Opcode:  0xA1
Operands: field_index: u16
Stack:   ( obj -- value )
Throws:  NullPointer if obj is null
```

### 12.3 PUT_FIELD (0xA2)

Set instance field.

```
Opcode:  0xA2
Operands: field_index: u16
Stack:   ( obj value -- )
Throws:  NullPointer if obj is null
```

### 12.4 GET_STATIC (0xA3)

Get static field.

```
Opcode:  0xA3
Operands: class_index: u16, field_index: u16
Stack:   ( -- value )
```

### 12.5 PUT_STATIC (0xA4)

Set static field.

```
Opcode:  0xA4
Operands: class_index: u16, field_index: u16
Stack:   ( value -- )
```

### 12.6 INSTANCEOF (0xA5)

Check instance type.

```
Opcode:  0xA5
Operands: type_index: u16
Stack:   ( obj -- result:bool )
```

### 12.7 CHECKCAST (0xA6)

Cast with type check.

```
Opcode:  0xA6
Operands: type_index: u16
Stack:   ( obj -- obj )
Throws:  ClassCast if incompatible type
```

### 12.8 GET_TYPE (0xA7)

Get runtime type of object.

```
Opcode:  0xA7
Operands: None
Stack:   ( obj -- type )
```

### 12.9 EQUALS (0xA8)

Call equals method.

```
Opcode:  0xA8
Operands: None
Stack:   ( obj1 obj2 -- result:bool )
Effect:  Call obj1.equals(obj2) or reference equality
```

### 12.10 HASHCODE (0xA9)

Get hash code.

```
Opcode:  0xA9
Operands: None
Stack:   ( obj -- hash:int )
```

### 12.11 TOSTRING (0xAA)

Convert to string.

```
Opcode:  0xAA
Operands: None
Stack:   ( obj -- str:string )
```

### 12.12 CLONE (0xAB)

Clone object.

```
Opcode:  0xAB
Operands: None
Stack:   ( obj -- copy )
Effect:  Shallow copy of object
```

---

## 13. Array Operations

### 13.1 NEWARRAY (0xB0)

Create primitive array.

```
Opcode:  0xB0
Operands: elem_type: u8
Stack:   ( length -- array )
Throws:  NegativeArraySize if length < 0
```

Element types:
| Type | Code |
|------|------|
| bool | 0x01 |
| int8 | 0x02 |
| int16 | 0x03 |
| int32 | 0x04 |
| int64 | 0x05 |
| float32 | 0x0A |
| float64 | 0x0B |
| char | 0x0C |

### 13.2 ANEWARRAY (0xB1)

Create reference array.

```
Opcode:  0xB1
Operands: type_index: u16
Stack:   ( length -- array )
```

### 13.3 ARRAYLENGTH (0xB2)

Get array length.

```
Opcode:  0xB2
Operands: None
Stack:   ( array -- length:int )
Throws:  NullPointer if array is null
```

### 13.4 ALOAD (0xB3)

Load from array.

```
Opcode:  0xB3
Operands: None
Stack:   ( array index -- value )
Throws:  IndexOutOfBounds if index >= length
```

### 13.5 ASTORE (0xB4)

Store into array.

```
Opcode:  0xB4
Operands: None
Stack:   ( array index value -- )
Throws:  IndexOutOfBounds if index >= length
```

### 13.6 IALOAD (0xB5)

Load int from array.

```
Opcode:  0xB5
Operands: None
Stack:   ( array index -- value:int )
```

### 13.7 IASTORE (0xB6)

Store int into array.

```
Opcode:  0xB6
Operands: None
Stack:   ( array index value:int -- )
```

### 13.8 DALOAD (0xB7)

Load float64 from array.

```
Opcode:  0xB7
Operands: None
Stack:   ( array index -- value:f64 )
```

### 13.9 DASTORE (0xB8)

Store float64 into array.

```
Opcode:  0xB8
Operands: None
Stack:   ( array index value:f64 -- )
```

### 13.10 AALOAD (0xB9)

Load reference from array.

```
Opcode:  0xB9
Operands: None
Stack:   ( array index -- obj )
```

### 13.11 AASTORE (0xBA)

Store reference into array.

```
Opcode:  0xBA
Operands: None
Stack:   ( array index obj -- )
Throws:  ArrayStore if type incompatible
```

### 13.12 ARRAY_COPY (0xBB)

Copy array region.

```
Opcode:  0xBB
Operands: None
Stack:   ( src src_pos dst dst_pos length -- )
Effect:  Copy length elements from src[src_pos] to dst[dst_pos]
```

### 13.13 ARRAY_FILL (0xBC)

Fill array with value.

```
Opcode:  0xBC
Operands: None
Stack:   ( array value -- )
Effect:  Fill all elements with value
```

---

## 14. String Operations

### 14.1 STR_CONCAT (0xC0)

Concatenate strings.

```
Opcode:  0xC0
Operands: None
Stack:   ( str1 str2 -- result )
```

### 14.2 STR_LENGTH (0xC1)

Get string length.

```
Opcode:  0xC1
Operands: None
Stack:   ( str -- length:int )
```

### 14.3 STR_CHAR_AT (0xC2)

Get character at index.

```
Opcode:  0xC2
Operands: None
Stack:   ( str index -- char )
Throws:  IndexOutOfBounds if invalid index
```

### 14.4 STR_SUBSTRING (0xC3)

Get substring.

```
Opcode:  0xC3
Operands: None
Stack:   ( str start end -- substr )
```

### 14.5 STR_EQUALS (0xC4)

String equality.

```
Opcode:  0xC4
Operands: None
Stack:   ( str1 str2 -- result:bool )
```

### 14.6 STR_COMPARE (0xC5)

String comparison.

```
Opcode:  0xC5
Operands: None
Stack:   ( str1 str2 -- result:int )
Result:  <0 if str1<str2, 0 if equal, >0 if str1>str2
```

### 14.7 STR_CONTAINS (0xC6)

Check if string contains substring.

```
Opcode:  0xC6
Operands: None
Stack:   ( str substr -- result:bool )
```

### 14.8 STR_INDEX_OF (0xC7)

Find substring index.

```
Opcode:  0xC7
Operands: None
Stack:   ( str substr -- index:int )
Result:  Index or -1 if not found
```

### 14.9 STR_STARTS_WITH (0xC8)

Check string prefix.

```
Opcode:  0xC8
Operands: None
Stack:   ( str prefix -- result:bool )
```

### 14.10 STR_ENDS_WITH (0xC9)

Check string suffix.

```
Opcode:  0xC9
Operands: None
Stack:   ( str suffix -- result:bool )
```

### 14.11 STR_TRIM (0xCA)

Trim whitespace.

```
Opcode:  0xCA
Operands: None
Stack:   ( str -- trimmed )
```

### 14.12 STR_TO_UPPER (0xCB)

Convert to uppercase.

```
Opcode:  0xCB
Operands: None
Stack:   ( str -- upper )
```

### 14.13 STR_TO_LOWER (0xCC)

Convert to lowercase.

```
Opcode:  0xCC
Operands: None
Stack:   ( str -- lower )
```

### 14.14 STR_SPLIT (0xCD)

Split string.

```
Opcode:  0xCD
Operands: None
Stack:   ( str delimiter -- array )
```

### 14.15 STR_JOIN (0xCE)

Join array with delimiter.

```
Opcode:  0xCE
Operands: None
Stack:   ( array delimiter -- str )
```

### 14.16 STR_FORMAT (0xCF)

Format string with arguments.

```
Opcode:  0xCF
Operands: arg_count: u8
Stack:   ( format args... -- result )
```

---

## 15. Reference Counting

### 15.1 INC_REF (0xD0)

Increment reference count.

```
Opcode:  0xD0
Operands: None
Stack:   ( obj -- obj )
Effect:  obj.refcount += 1
```

### 15.2 DEC_REF (0xD1)

Decrement reference count.

```
Opcode:  0xD1
Operands: None
Stack:   ( obj -- )
Effect:  obj.refcount -= 1; free if 0
```

### 15.3 MOVE (0xD2)

Move reference (transfer ownership).

```
Opcode:  0xD2
Operands: None
Stack:   ( obj -- obj )
Effect:  Transfer without count change
```

### 15.4 WEAK_REF (0xD3)

Create weak reference.

```
Opcode:  0xD3
Operands: None
Stack:   ( obj -- weak )
Effect:  Create non-owning reference
```

### 15.5 WEAK_DEREF (0xD4)

Dereference weak reference.

```
Opcode:  0xD4
Operands: None
Stack:   ( weak -- obj_or_null )
Effect:  Get object if still alive, else null
```

### 15.6 CYCLE_COLLECT (0xD5)

Trigger cycle collection.

```
Opcode:  0xD5
Operands: None
Stack:   ( -- )
Effect:  Run cycle detection algorithm
```

---

## 16. Channel Operations

### 16.1 CHAN_NEW (0xD8)

Create new channel.

```
Opcode:  0xD8
Operands: None
Stack:   ( capacity -- channel )
Effect:  Create channel with buffer capacity (0 = unbuffered)
```

### 16.2 CHAN_SEND (0xD9)

Send value on channel.

```
Opcode:  0xD9
Operands: None
Stack:   ( channel value -- )
Effect:  Send value, may block if full
Throws:  ClosedChannel if channel closed
```

### 16.3 CHAN_RECV (0xDA)

Receive value from channel.

```
Opcode:  0xDA
Operands: None
Stack:   ( channel -- value )
Effect:  Receive value, may block if empty
Throws:  ClosedChannel if channel closed and empty
```

### 16.4 CHAN_TRY_SEND (0xDB)

Non-blocking send.

```
Opcode:  0xDB
Operands: None
Stack:   ( channel value -- success:bool )
Effect:  Send if possible, return false if would block
```

### 16.5 CHAN_TRY_RECV (0xDC)

Non-blocking receive.

```
Opcode:  0xDC
Operands: None
Stack:   ( channel -- value_or_null ok:bool )
Effect:  Receive if possible, return (null, false) if would block
```

### 16.6 CHAN_CLOSE (0xDD)

Close channel.

```
Opcode:  0xDD
Operands: None
Stack:   ( channel -- )
Effect:  Mark channel as closed, wake waiters
```

### 16.7 SELECT (0xDE)

Select from multiple channels.

```
Opcode:  0xDE
Operands: case_count: u8
          cases: [
            op: u8 (0=recv, 1=send)
            target_offset: i16
          ][case_count]
          default_offset: i16 (0 if no default)
Stack:   ( channels... values... -- index result_or_null )
Effect:  Wait on multiple channels, execute ready case
```

### 16.8 CHAN_LEN (0xDF)

Get channel buffer length.

```
Opcode:  0xDF
Operands: None
Stack:   ( channel -- length:int )
```

---

## 17. Fiber Operations

### 17.1 FIBER_SPAWN (0xE0)

Spawn new fiber.

```
Opcode:  0xE0
Operands: func_index: u16
Stack:   ( args... -- fiber )
Effect:  Create fiber running function with args
```

### 17.2 FIBER_YIELD (0xE1)

Yield current fiber.

```
Opcode:  0xE1
Operands: None
Stack:   ( -- )
Effect:  Suspend current fiber, schedule another
```

### 17.3 FIBER_RESUME (0xE2)

Resume suspended fiber.

```
Opcode:  0xE2
Operands: None
Stack:   ( fiber -- )
Effect:  Make fiber runnable
```

### 17.4 FIBER_JOIN (0xE3)

Wait for fiber to complete.

```
Opcode:  0xE3
Operands: None
Stack:   ( fiber -- result )
Effect:  Block until fiber finishes, get return value
```

### 17.5 FIBER_CURRENT (0xE4)

Get current fiber.

```
Opcode:  0xE4
Operands: None
Stack:   ( -- fiber )
```

### 17.6 FIBER_STATUS (0xE5)

Get fiber status.

```
Opcode:  0xE5
Operands: None
Stack:   ( fiber -- status:int )
Status:  0=created, 1=running, 2=suspended, 3=completed, 4=failed
```

### 17.7 FIBER_CANCEL (0xE6)

Cancel fiber.

```
Opcode:  0xE6
Operands: None
Stack:   ( fiber -- )
Effect:  Request fiber cancellation
```

### 17.8 FIBER_SLEEP (0xE7)

Sleep current fiber.

```
Opcode:  0xE7
Operands: None
Stack:   ( millis:i64 -- )
Effect:  Suspend for duration
```

---

## 18. Syscall Operations

### 18.1 SYSCALL (0xE8)

Invoke system call.

```
Opcode:  0xE8
Operands: syscall_number: u16
Stack:   ( args... -- result )
Effect:  Invoke host syscall

Syscall numbers (host primitives):

Process (0-15):
0x0000 sys_exit
0x0001 sys_fork
0x0002 sys_exec
0x0003 sys_wait
0x0004 sys_getpid
0x0005 sys_getppid
0x0006 sys_yield
0x0007 sys_clone

Memory (16-31):
0x0010 sys_brk
0x0011 sys_mmap
0x0012 sys_munmap

File (32-63):
0x0020 sys_open
0x0021 sys_close
0x0022 sys_read
0x0023 sys_write
0x0024 sys_seek

Graphics (168-191):
0x00A8 sys_create_window (168)
0x00A9 sys_destroy_window (169)
0x00AD sys_get_event (173)
```

### 18.2 SYSCALL_FAST (0xE9)

Fast syscall (no stack frame).

```
Opcode:  0xE9
Operands: syscall_number: u8
Stack:   ( arg1 arg2 -- result )
Effect:  Optimized syscall for 2 args
```

---

## 19. Miscellaneous

### 19.1 THROW (0xF0)

Throw exception.

```
Opcode:  0xF0
Operands: None
Stack:   ( exception -- )
Effect:  Unwind to nearest handler
```

### 19.2 RETHROW (0xF1)

Rethrow current exception.

```
Opcode:  0xF1
Operands: None
Stack:   ( -- )
Effect:  Continue unwinding
```

### 19.3 MONITOR_ENTER (0xF2)

Enter synchronized block.

```
Opcode:  0xF2
Operands: None
Stack:   ( obj -- )
Effect:  Acquire object's monitor
```

### 19.4 MONITOR_EXIT (0xF3)

Exit synchronized block.

```
Opcode:  0xF3
Operands: None
Stack:   ( obj -- )
Effect:  Release object's monitor
```

### 19.5 BREAKPOINT (0xF4)

Debugger breakpoint.

```
Opcode:  0xF4
Operands: None
Stack:   ( -- )
Effect:  Pause execution for debugger
```

### 19.6 ASSERT (0xF5)

Assert condition.

```
Opcode:  0xF5
Operands: None
Stack:   ( cond:bool message:str -- )
Effect:  Throw AssertionError if cond is false
```

### 19.7 PRINT_DEBUG (0xF6)

Debug print (development only).

```
Opcode:  0xF6
Operands: None
Stack:   ( value -- )
Effect:  Print value to debug output
```

### 19.8 LINE_NUMBER (0xF7)

Source line marker.

```
Opcode:  0xF7
Operands: line: u16
Stack:   ( -- )
Effect:  Update current line for debugging
```

### 19.9 WIDE (0xFE)

Wide instruction prefix.

```
Opcode:  0xFE
Operands: opcode: u8, wide_operands...
Stack:   (depends on inner opcode)
Effect:  Execute opcode with widened operands
```

### 19.10 ILLEGAL (0xFF)

Illegal instruction.

```
Opcode:  0xFF
Operands: None
Stack:   ( -- )
Effect:  Trigger illegal instruction error
```

---

## 20. Opcode Summary Table

### 20.1 Complete Opcode Listing

| Opcode                              | Mnemonic         | Operands | Stack Effect                |
| ----------------------------------- | ---------------- | -------- | --------------------------- |
| **Stack Operations (0x00-0x0F)**    |                  |          |                             |
| 0x00                                | NOP              | -        | ( -- )                      |
| 0x01                                | POP              | -        | ( v -- )                    |
| 0x02                                | POP2             | -        | ( v1 v2 -- )                |
| 0x03                                | DUP              | -        | ( v -- v v )                |
| 0x04                                | DUP2             | -        | ( v1 v2 -- v1 v2 v1 v2 )    |
| 0x05                                | DUP_X1           | -        | ( v1 v2 -- v2 v1 v2 )       |
| 0x06                                | DUP_X2           | -        | ( v1 v2 v3 -- v3 v1 v2 v3 ) |
| 0x07                                | SWAP             | -        | ( v1 v2 -- v2 v1 )          |
| 0x08                                | ROT              | -        | ( v1 v2 v3 -- v2 v3 v1 )    |
| 0x09                                | OVER             | -        | ( v1 v2 -- v1 v2 v1 )       |
| **Local Variables (0x10-0x1F)**     |                  |          |                             |
| 0x10                                | LOAD             | u8       | ( -- v )                    |
| 0x11                                | LOAD_W           | u16      | ( -- v )                    |
| 0x12                                | LOAD_0           | -        | ( -- v )                    |
| 0x13                                | LOAD_1           | -        | ( -- v )                    |
| 0x14                                | LOAD_2           | -        | ( -- v )                    |
| 0x15                                | LOAD_3           | -        | ( -- v )                    |
| 0x16                                | STORE            | u8       | ( v -- )                    |
| 0x17                                | STORE_W          | u16      | ( v -- )                    |
| 0x18                                | STORE_0          | -        | ( v -- )                    |
| 0x19                                | STORE_1          | -        | ( v -- )                    |
| 0x1A                                | STORE_2          | -        | ( v -- )                    |
| 0x1B                                | STORE_3          | -        | ( v -- )                    |
| 0x1C                                | IINC             | u8, i8   | ( -- )                      |
| 0x1D                                | IINC_W           | u16, i16 | ( -- )                      |
| **Constants (0x20-0x2F)**           |                  |          |                             |
| 0x20                                | CONST_NULL       | -        | ( -- null )                 |
| 0x21                                | CONST_TRUE       | -        | ( -- true )                 |
| 0x22                                | CONST_FALSE      | -        | ( -- false )                |
| 0x23                                | CONST_I0         | -        | ( -- 0 )                    |
| 0x24                                | CONST_I1         | -        | ( -- 1 )                    |
| 0x25                                | CONST_I2         | -        | ( -- 2 )                    |
| 0x26                                | CONST_IM1        | -        | ( -- -1 )                   |
| 0x27                                | CONST_F0         | -        | ( -- 0.0 )                  |
| 0x28                                | CONST_F1         | -        | ( -- 1.0 )                  |
| 0x29                                | BIPUSH           | i8       | ( -- i )                    |
| 0x2A                                | SIPUSH           | i16      | ( -- i )                    |
| 0x2B                                | LDC              | u8       | ( -- c )                    |
| 0x2C                                | LDC_W            | u16      | ( -- c )                    |
| 0x2D                                | LDC2_W           | u16      | ( -- c64 )                  |
| **Integer Arithmetic (0x30-0x3F)**  |                  |          |                             |
| 0x30                                | IADD             | -        | ( a b -- a+b )              |
| 0x31                                | ISUB             | -        | ( a b -- a-b )              |
| 0x32                                | IMUL             | -        | ( a b -- a\*b )             |
| 0x33                                | IDIV             | -        | ( a b -- a/b )              |
| 0x34                                | IREM             | -        | ( a b -- a%b )              |
| 0x35                                | INEG             | -        | ( a -- -a )                 |
| 0x36                                | IABS             | -        | ( a -- \|a\| )              |
| 0x37                                | IMIN             | -        | ( a b -- min )              |
| 0x38                                | IMAX             | -        | ( a b -- max )              |
| 0x39                                | LADD             | -        | ( a b -- a+b ):i64          |
| 0x3A                                | LSUB             | -        | ( a b -- a-b ):i64          |
| 0x3B                                | LMUL             | -        | ( a b -- a\*b ):i64         |
| 0x3C                                | LDIV             | -        | ( a b -- a/b ):i64          |
| 0x3D                                | LREM             | -        | ( a b -- a%b ):i64          |
| 0x3E                                | LNEG             | -        | ( a -- -a ):i64             |
| **Float Arithmetic (0x40-0x4F)**    |                  |          |                             |
| 0x40                                | FADD             | -        | ( a b -- a+b ):f32          |
| 0x41                                | FSUB             | -        | ( a b -- a-b ):f32          |
| 0x42                                | FMUL             | -        | ( a b -- a\*b ):f32         |
| 0x43                                | FDIV             | -        | ( a b -- a/b ):f32          |
| 0x44                                | FREM             | -        | ( a b -- a%b ):f32          |
| 0x45                                | FNEG             | -        | ( a -- -a ):f32             |
| 0x46                                | DADD             | -        | ( a b -- a+b ):f64          |
| 0x47                                | DSUB             | -        | ( a b -- a-b ):f64          |
| 0x48                                | DMUL             | -        | ( a b -- a\*b ):f64         |
| 0x49                                | DDIV             | -        | ( a b -- a/b ):f64          |
| 0x4A                                | DREM             | -        | ( a b -- a%b ):f64          |
| 0x4B                                | DNEG             | -        | ( a -- -a ):f64             |
| 0x4C                                | DABS             | -        | ( a -- \|a\| ):f64          |
| 0x4D                                | DSQRT            | -        | ( a -- sqrt(a) ):f64        |
| 0x4E                                | DMIN             | -        | ( a b -- min ):f64          |
| 0x4F                                | DMAX             | -        | ( a b -- max ):f64          |
| **Bitwise Operations (0x50-0x5F)**  |                  |          |                             |
| 0x50                                | IAND             | -        | ( a b -- a&b )              |
| 0x51                                | IOR              | -        | ( a b -- a\|b )             |
| 0x52                                | IXOR             | -        | ( a b -- a^b )              |
| 0x53                                | INOT             | -        | ( a -- ~a )                 |
| 0x54                                | ISHL             | -        | ( a n -- a<<n )             |
| 0x55                                | ISHR             | -        | ( a n -- a>>n )             |
| 0x56                                | IUSHR            | -        | ( a n -- a>>>n )            |
| 0x57                                | LAND             | -        | ( a b -- a&b ):i64          |
| 0x58                                | LOR              | -        | ( a b -- a\|b ):i64         |
| 0x59                                | LXOR             | -        | ( a b -- a^b ):i64          |
| 0x5A                                | LNOT             | -        | ( a -- ~a ):i64             |
| 0x5B                                | LSHL             | -        | ( a n -- a<<n ):i64         |
| 0x5C                                | LSHR             | -        | ( a n -- a>>n ):i64         |
| 0x5D                                | LUSHR            | -        | ( a n -- a>>>n ):i64        |
| **Comparison (0x60-0x6F)**          |                  |          |                             |
| 0x60                                | ICMP_EQ          | -        | ( a b -- a==b )             |
| 0x61                                | ICMP_NE          | -        | ( a b -- a!=b )             |
| 0x62                                | ICMP_LT          | -        | ( a b -- a<b )              |
| 0x63                                | ICMP_LE          | -        | ( a b -- a<=b )             |
| 0x64                                | ICMP_GT          | -        | ( a b -- a>b )              |
| 0x65                                | ICMP_GE          | -        | ( a b -- a>=b )             |
| 0x66                                | DCMP_EQ          | -        | ( a b -- a==b ):f64         |
| 0x67                                | DCMP_NE          | -        | ( a b -- a!=b ):f64         |
| 0x68                                | DCMP_LT          | -        | ( a b -- a<b ):f64          |
| 0x69                                | DCMP_LE          | -        | ( a b -- a<=b ):f64         |
| 0x6A                                | DCMP_GT          | -        | ( a b -- a>b ):f64          |
| 0x6B                                | DCMP_GE          | -        | ( a b -- a>=b ):f64         |
| 0x6C                                | REF_EQ           | -        | ( r1 r2 -- r1===r2 )        |
| 0x6D                                | REF_NE           | -        | ( r1 r2 -- r1!==r2 )        |
| 0x6E                                | IS_NULL          | -        | ( v -- v==null )            |
| 0x6F                                | IS_NOT_NULL      | -        | ( v -- v!=null )            |
| **Type Conversion (0x70-0x7F)**     |                  |          |                             |
| 0x70                                | I2L              | -        | ( i -- l )                  |
| 0x71                                | I2F              | -        | ( i -- f )                  |
| 0x72                                | I2D              | -        | ( i -- d )                  |
| 0x73                                | L2I              | -        | ( l -- i )                  |
| 0x74                                | L2F              | -        | ( l -- f )                  |
| 0x75                                | L2D              | -        | ( l -- d )                  |
| 0x76                                | F2I              | -        | ( f -- i )                  |
| 0x77                                | F2L              | -        | ( f -- l )                  |
| 0x78                                | F2D              | -        | ( f -- d )                  |
| 0x79                                | D2I              | -        | ( d -- i )                  |
| 0x7A                                | D2L              | -        | ( d -- l )                  |
| 0x7B                                | D2F              | -        | ( d -- f )                  |
| 0x7C                                | I2B              | -        | ( i -- b )                  |
| 0x7D                                | I2S              | -        | ( i -- s )                  |
| 0x7E                                | I2C              | -        | ( i -- c )                  |
| **Control Flow (0x80-0x8F)**        |                  |          |                             |
| 0x80                                | GOTO             | i16      | ( -- )                      |
| 0x81                                | GOTO_W           | i32      | ( -- )                      |
| 0x82                                | IF_TRUE          | i16      | ( c -- )                    |
| 0x83                                | IF_FALSE         | i16      | ( c -- )                    |
| 0x84                                | IF_NULL          | i16      | ( v -- )                    |
| 0x85                                | IF_NOT_NULL      | i16      | ( v -- )                    |
| 0x86                                | IF_ICMP_EQ       | i16      | ( a b -- )                  |
| 0x87                                | IF_ICMP_NE       | i16      | ( a b -- )                  |
| 0x88                                | IF_ICMP_LT       | i16      | ( a b -- )                  |
| 0x89                                | IF_ICMP_LE       | i16      | ( a b -- )                  |
| 0x8A                                | IF_ICMP_GT       | i16      | ( a b -- )                  |
| 0x8B                                | IF_ICMP_GE       | i16      | ( a b -- )                  |
| 0x8C                                | TABLESWITCH      | ...      | ( i -- )                    |
| 0x8D                                | LOOKUPSWITCH     | ...      | ( k -- )                    |
| **Function Operations (0x90-0x9F)** |                  |          |                             |
| 0x90                                | INVOKE           | u16      | ( args -- r )               |
| 0x91                                | INVOKE_VIRTUAL   | u16      | ( rcv args -- r )           |
| 0x92                                | INVOKE_INTERFACE | u16, u8  | ( rcv args -- r )           |
| 0x93                                | INVOKE_STATIC    | u16, u16 | ( args -- r )               |
| 0x94                                | INVOKE_SPECIAL   | u16      | ( rcv args -- r )           |
| 0x95                                | INVOKE_DYNAMIC   | u16      | ( t args -- r )             |
| 0x96                                | RETURN           | -        | ( v -- )                    |
| 0x97                                | RETURN_VOID      | -        | ( -- )                      |
| 0x98                                | TAILCALL         | u16      | ( args -- )                 |
| **Object Operations (0xA0-0xAF)**   |                  |          |                             |
| 0xA0                                | NEW              | u16      | ( -- obj )                  |
| 0xA1                                | GET_FIELD        | u16      | ( obj -- v )                |
| 0xA2                                | PUT_FIELD        | u16      | ( obj v -- )                |
| 0xA3                                | GET_STATIC       | u16, u16 | ( -- v )                    |
| 0xA4                                | PUT_STATIC       | u16, u16 | ( v -- )                    |
| 0xA5                                | INSTANCEOF       | u16      | ( obj -- b )                |
| 0xA6                                | CHECKCAST        | u16      | ( obj -- obj )              |
| 0xA7                                | GET_TYPE         | -        | ( obj -- t )                |
| 0xA8                                | EQUALS           | -        | ( o1 o2 -- b )              |
| 0xA9                                | HASHCODE         | -        | ( obj -- h )                |
| 0xAA                                | TOSTRING         | -        | ( obj -- s )                |
| 0xAB                                | CLONE            | -        | ( obj -- cp )               |
| **Array Operations (0xB0-0xBF)**    |                  |          |                             |
| 0xB0                                | NEWARRAY         | u8       | ( len -- arr )              |
| 0xB1                                | ANEWARRAY        | u16      | ( len -- arr )              |
| 0xB2                                | ARRAYLENGTH      | -        | ( arr -- len )              |
| 0xB3                                | ALOAD            | -        | ( arr i -- v )              |
| 0xB4                                | ASTORE           | -        | ( arr i v -- )              |
| 0xB5                                | IALOAD           | -        | ( arr i -- v )              |
| 0xB6                                | IASTORE          | -        | ( arr i v -- )              |
| 0xB7                                | DALOAD           | -        | ( arr i -- v )              |
| 0xB8                                | DASTORE          | -        | ( arr i v -- )              |
| 0xB9                                | AALOAD           | -        | ( arr i -- o )              |
| 0xBA                                | AASTORE          | -        | ( arr i o -- )              |
| 0xBB                                | ARRAY_COPY       | -        | ( s sp d dp l -- )          |
| 0xBC                                | ARRAY_FILL       | -        | ( arr v -- )                |
| **String Operations (0xC0-0xCF)**   |                  |          |                             |
| 0xC0                                | STR_CONCAT       | -        | ( s1 s2 -- r )              |
| 0xC1                                | STR_LENGTH       | -        | ( s -- l )                  |
| 0xC2                                | STR_CHAR_AT      | -        | ( s i -- c )                |
| 0xC3                                | STR_SUBSTRING    | -        | ( s a b -- r )              |
| 0xC4                                | STR_EQUALS       | -        | ( s1 s2 -- b )              |
| 0xC5                                | STR_COMPARE      | -        | ( s1 s2 -- i )              |
| 0xC6                                | STR_CONTAINS     | -        | ( s p -- b )                |
| 0xC7                                | STR_INDEX_OF     | -        | ( s p -- i )                |
| 0xC8                                | STR_STARTS_WITH  | -        | ( s p -- b )                |
| 0xC9                                | STR_ENDS_WITH    | -        | ( s p -- b )                |
| 0xCA                                | STR_TRIM         | -        | ( s -- r )                  |
| 0xCB                                | STR_TO_UPPER     | -        | ( s -- r )                  |
| 0xCC                                | STR_TO_LOWER     | -        | ( s -- r )                  |
| 0xCD                                | STR_SPLIT        | -        | ( s d -- a )                |
| 0xCE                                | STR_JOIN         | -        | ( a d -- s )                |
| 0xCF                                | STR_FORMAT       | u8       | ( f args -- r )             |
| **Reference Counting (0xD0-0xD7)**  |                  |          |                             |
| 0xD0                                | INC_REF          | -        | ( o -- o )                  |
| 0xD1                                | DEC_REF          | -        | ( o -- )                    |
| 0xD2                                | MOVE             | -        | ( o -- o )                  |
| 0xD3                                | WEAK_REF         | -        | ( o -- w )                  |
| 0xD4                                | WEAK_DEREF       | -        | ( w -- o? )                 |
| 0xD5                                | CYCLE_COLLECT    | -        | ( -- )                      |
| **Channel Operations (0xD8-0xDF)**  |                  |          |                             |
| 0xD8                                | CHAN_NEW         | -        | ( cap -- ch )               |
| 0xD9                                | CHAN_SEND        | -        | ( ch v -- )                 |
| 0xDA                                | CHAN_RECV        | -        | ( ch -- v )                 |
| 0xDB                                | CHAN_TRY_SEND    | -        | ( ch v -- b )               |
| 0xDC                                | CHAN_TRY_RECV    | -        | ( ch -- v? b )              |
| 0xDD                                | CHAN_CLOSE       | -        | ( ch -- )                   |
| 0xDE                                | SELECT           | u8, ...  | ( chs... -- i r? )          |
| 0xDF                                | CHAN_LEN         | -        | ( ch -- l )                 |
| **Fiber Operations (0xE0-0xE7)**    |                  |          |                             |
| 0xE0                                | FIBER_SPAWN      | u16      | ( args -- f )               |
| 0xE1                                | FIBER_YIELD      | -        | ( -- )                      |
| 0xE2                                | FIBER_RESUME     | -        | ( f -- )                    |
| 0xE3                                | FIBER_JOIN       | -        | ( f -- r )                  |
| 0xE4                                | FIBER_CURRENT    | -        | ( -- f )                    |
| 0xE5                                | FIBER_STATUS     | -        | ( f -- s )                  |
| 0xE6                                | FIBER_CANCEL     | -        | ( f -- )                    |
| 0xE7                                | FIBER_SLEEP      | -        | ( ms -- )                   |
| **Syscall Operations (0xE8-0xEF)**  |                  |          |                             |
| 0xE8                                | SYSCALL          | u16      | ( args -- r )               |
| 0xE9                                | SYSCALL_FAST     | u8       | ( a1 a2 -- r )              |
| **Miscellaneous (0xF0-0xFF)**       |                  |          |                             |
| 0xF0                                | THROW            | -        | ( e -- )                    |
| 0xF1                                | RETHROW          | -        | ( -- )                      |
| 0xF2                                | MONITOR_ENTER    | -        | ( o -- )                    |
| 0xF3                                | MONITOR_EXIT     | -        | ( o -- )                    |
| 0xF4                                | BREAKPOINT       | -        | ( -- )                      |
| 0xF5                                | ASSERT           | -        | ( c m -- )                  |
| 0xF6                                | PRINT_DEBUG      | -        | ( v -- )                    |
| 0xF7                                | LINE_NUMBER      | u16      | ( -- )                      |
| 0xFE                                | WIDE             | u8, ...  | varies                      |
| 0xFF                                | ILLEGAL          | -        | error                       |

---

## Appendix A: Instruction Encoding Examples

### A.1 Simple Addition

```li
let a = 5
let b = 3
let c = a + b
```

Bytecode:

```
BIPUSH 5        ; 29 05
STORE_0         ; 18
BIPUSH 3        ; 29 03
STORE_1         ; 19
LOAD_0          ; 12
LOAD_1          ; 13
IADD            ; 30
STORE_2         ; 1A
```

### A.2 Function Call

```li
fn add(x: int, y: int) -> int {
    return x + y
}

let result = add(10, 20)
```

Bytecode (call site):

```
BIPUSH 10       ; 29 0A
BIPUSH 20       ; 29 14
INVOKE 0        ; 90 00 00    (function index 0)
STORE_0         ; 18
```

### A.3 Object Creation

```li
class Point {
    let x: int
    let y: int
}

let p = Point { x: 10, y: 20 }
```

Bytecode:

```
NEW 0           ; A0 00 00    (type index 0 = Point)
DUP             ; 03
BIPUSH 10       ; 29 0A
PUT_FIELD 0     ; A2 00 00    (field index 0 = x)
DUP             ; 03
BIPUSH 20       ; 29 14
PUT_FIELD 1     ; A2 00 01    (field index 1 = y)
STORE_0         ; 18
```

### A.4 Channel Communication

```li
let ch = Channel<int>.new()
spawn {
    ch.send(42)
}
let value = ch.receive()
```

Bytecode (main fiber):

```
CONST_I0        ; 23          (capacity 0 = unbuffered)
CHAN_NEW        ; D8
STORE_0         ; 18          (ch in local 0)
FIBER_SPAWN 1   ; E0 00 01    (spawn function at index 1)
POP             ; 01          (discard fiber handle)
LOAD_0          ; 12
CHAN_RECV       ; DA
STORE_1         ; 19          (value in local 1)
```

### A.5 Conditional Branch

```li
if x > 10 {
    y = 1
} else {
    y = 2
}
```

Bytecode:

```
LOAD_0          ; 12          (x)
BIPUSH 10       ; 29 0A
IF_ICMP_LE 6    ; 89 00 06    (skip to else if x <= 10)
CONST_I1        ; 24
STORE_1         ; 19
GOTO 3          ; 80 00 03    (skip else block)
CONST_I2        ; 25          (else block)
STORE_1         ; 19
```

---

## Appendix B: Exception Handling Example

```li
try {
    risky_operation()
} catch (e: Error) {
    handle_error(e)
}
```

Bytecode:

```
; Try block (offsets 0-4)
00: INVOKE 0        ; risky_operation
03: GOTO 8          ; skip catch

; Catch block (offset 5)
05: STORE_0         ; exception in local 0
06: LOAD_0
07: INVOKE 1        ; handle_error
0A: POP

; After try-catch
0B: ...
```

Exception handler table entry:

```
try_start:   0x00
try_end:     0x05
handler_pc:  0x05
catch_type:  0x01 (Error type index)
```

---

## Appendix C: Stack Frame Layout

```
┌─────────────────────────────────────────────────────────────────┐
│                      STACK FRAME                                 │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                 OPERAND STACK (grows up)                    ││
│  │  [slot N-1] [slot N-2] ... [slot 1] [slot 0]               ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                   LOCAL VARIABLES                            ││
│  │  [local 0: this/arg0] [local 1: arg1] ... [local N: var]   ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    FRAME METADATA                            ││
│  │  - Return address (PC)                                      ││
│  │  - Caller frame pointer                                     ││
│  │  - Method reference                                         ││
│  │  - Exception handler chain                                  ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

---

_This document is part of the Lira Language Specification._
