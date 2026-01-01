# Lira Lexical Structure Specification

## Document Information

| Property          | Value                |
| ----------------- | -------------------- |
| **Document ID**   | 01-lexical-structure |
| **Version**       | 1.0.0-draft          |
| **Status**        | Draft Specification  |
| **Prerequisites** | 00-lira-overview.md  |

---

## Table of Contents

1. [Source Text](#1-source-text)
2. [Lexical Elements](#2-lexical-elements)
3. [Whitespace and Line Terminators](#3-whitespace-and-line-terminators)
4. [Comments](#4-comments)
5. [Identifiers](#5-identifiers)
6. [Keywords](#6-keywords)
7. [Literals](#7-literals)
8. [Operators and Punctuation](#8-operators-and-punctuation)
9. [Lexer State Machine](#9-lexer-state-machine)

---

## 1. Source Text

### 1.1 Character Encoding

Lira source files MUST be encoded in UTF-8. The byte order mark (BOM) is permitted but not recommended.

```
SourceFile ::= BOM? SourceCharacter*
BOM        ::= U+FEFF
```

### 1.2 Source Characters

A source file consists of a sequence of Unicode code points:

```
SourceCharacter ::= <any Unicode code point>
```

The following control characters have special meaning:

| Character | Code Point | Name            | Usage           |
| --------- | ---------- | --------------- | --------------- |
| HT        | U+0009     | Horizontal Tab  | Whitespace      |
| LF        | U+000A     | Line Feed       | Line terminator |
| CR        | U+000D     | Carriage Return | Line terminator |
| SP        | U+0020     | Space           | Whitespace      |

### 1.3 Line Terminators

Line terminators separate logical lines:

```
LineTerminator     ::= LF | CR | CRLF
LineTerminatorSeq  ::= LF | CR !LF | CR LF
```

Line numbers are incremented after each line terminator. The first line is line 1.

### 1.4 File Extension

Lira source files use the `.li` extension. Lira UI files use the `.liui` extension.

---

## 2. Lexical Elements

### 2.1 Token Categories

Lira source text is divided into the following token categories:

| Category        | Examples                         |
| --------------- | -------------------------------- |
| **Whitespace**  | spaces, tabs, newlines           |
| **Comments**    | `// ...`, `/* ... */`, `/// ...` |
| **Identifiers** | `foo`, `myVariable`, `_private`  |
| **Keywords**    | `fn`, `let`, `if`, `class`       |
| **Literals**    | `42`, `3.14`, `"hello"`, `true`  |
| **Operators**   | `+`, `-`, `==`, `&&`             |
| **Punctuation** | `{`, `}`, `(`, `)`, `;`          |

### 2.2 Token Boundaries

Tokens are delimited by:

- Whitespace
- Comments
- Other tokens (where lexically unambiguous)
- Maximum munch rule (longest match wins)

### 2.3 Maximum Munch Rule

When multiple token interpretations are possible, the lexer selects the longest valid token:

```li
// "++a" is tokenized as: [INCREMENT, IDENTIFIER(a)]
// not as: [PLUS, PLUS, IDENTIFIER(a)]
++a

// "a++b" is tokenized as: [IDENTIFIER(a), INCREMENT, IDENTIFIER(b)]
a++b
```

---

## 3. Whitespace and Line Terminators

### 3.1 Whitespace Characters

Whitespace separates tokens but carries no semantic meaning:

```
Whitespace ::= WhitespaceChar+
WhitespaceChar ::= SP | HT | LineTerminator
```

| Character | Description          |
| --------- | -------------------- |
| U+0009    | Horizontal Tab (HT)  |
| U+000A    | Line Feed (LF)       |
| U+000D    | Carriage Return (CR) |
| U+0020    | Space (SP)           |

### 3.2 Significant Newlines

Lira does NOT use significant newlines (unlike Python or Go). Statements are terminated by explicit semicolons or inferred from block structure:

```li
// These are equivalent
let a = 1; let b = 2
let a = 1
let b = 2

// Multi-line expressions are allowed
let result = 1 +
             2 +
             3
```

### 3.3 Indentation

Indentation is not syntactically significant but is encouraged for readability:

- Use 4 spaces per indentation level (recommended)
- Tabs are permitted but spaces are preferred
- Do not mix tabs and spaces in the same file

---

## 4. Comments

### 4.1 Single-Line Comments

Single-line comments begin with `//` and extend to the end of the line:

```
SingleLineComment ::= '//' CommentChar* LineTerminator?
CommentChar       ::= <any Unicode except LineTerminator>
```

Example:

```li
// This is a single-line comment
let x = 42  // Inline comment
```

### 4.2 Multi-Line Comments

Multi-line comments are delimited by `/*` and `*/`:

```
MultiLineComment ::= '/*' CommentContent '*/'
CommentContent   ::= <any sequence not containing '*/' >
```

Multi-line comments do NOT nest:

```li
/* This is a
   multi-line comment */

/* Outer /* Inner */ comment */  // ERROR: "Inner */" ends the outer comment
```

Example:

```li
/*
 * Multi-line comment
 * with asterisk decoration
 */
fn example() { }
```

### 4.3 Documentation Comments

Documentation comments use `///` for single-line or `/** */` for multi-line:

```
DocComment      ::= DocLineComment | DocBlockComment
DocLineComment  ::= '///' DocChar* LineTerminator?
DocBlockComment ::= '/**' DocContent '*/'
```

Documentation comments are attached to the following declaration and support Markdown formatting:

````li
/// Calculates the factorial of a number.
///
/// # Arguments
/// * `n` - The input number (must be non-negative)
///
/// # Returns
/// The factorial of `n`
///
/// # Examples
/// ```
/// let result = factorial(5)
/// assert(result == 120)
/// ```
fn factorial(n: int) -> int {
    if n <= 1 { return 1 }
    return n * factorial(n - 1)
}

/**
 * Represents a 2D point.
 *
 * @field x The x-coordinate
 * @field y The y-coordinate
 */
struct Point {
    x: float
    y: float
}
````

### 4.4 Comment Directives

Special comment directives provide compiler hints:

```li
//! Module-level documentation (first comment in file)

// TODO: Implement this feature
// FIXME: This is a known bug
// HACK: Temporary workaround
// NOTE: Important information

// @deprecated Use newFunction() instead
fn oldFunction() { }
```

---

## 5. Identifiers

### 5.1 Identifier Syntax

Identifiers name variables, functions, types, and other entities:

```
Identifier      ::= IdentifierStart IdentifierContinue*
IdentifierStart ::= Letter | '_'
IdentifierContinue ::= Letter | Digit | '_'
Letter          ::= UnicodeLetterCategory
Digit           ::= '0'..'9'
```

### 5.2 Unicode Letters

Lira accepts Unicode letters in identifiers, following Unicode category `L`:

```li
let name = "Alice"
let nombre = "Carlos"
let japanese = "value"      // Using ASCII for example
```

### 5.3 Reserved Identifiers

The following identifier patterns are reserved:

| Pattern                          | Reservation                 |
| -------------------------------- | --------------------------- |
| `_` (single underscore)          | Discard binding             |
| `__*` (double underscore prefix) | Reserved for implementation |
| `*__` (double underscore suffix) | Reserved for implementation |

### 5.4 Naming Conventions

While not enforced by the compiler, the following conventions are strongly recommended:

| Entity                      | Convention           | Example           |
| --------------------------- | -------------------- | ----------------- |
| Variables                   | snake_case           | `my_variable`     |
| Functions                   | snake_case           | `calculate_total` |
| Types (class, struct, enum) | PascalCase           | `UserAccount`     |
| Interfaces                  | PascalCase           | `Drawable`        |
| Constants                   | SCREAMING_SNAKE_CASE | `MAX_BUFFER_SIZE` |
| Type parameters             | Single uppercase     | `T`, `K`, `V`     |
| Private members             | Leading underscore   | `_internal`       |
| Modules                     | snake_case           | `string_utils`    |

### 5.5 Identifier Examples

```li
// Valid identifiers
let foo = 1
let _private = 2
let camelCase = 3
let PascalCase = 4
let snake_case = 5
let with123numbers = 6
let _ = ignored  // Discard binding

// Invalid identifiers
// let 123abc = 1    // Cannot start with digit
// let my-var = 2    // Hyphens not allowed
// let my var = 3    // Spaces not allowed
```

---

## 6. Keywords

### 6.1 Reserved Keywords

The following identifiers are reserved as keywords and cannot be used as identifiers:

#### Declaration Keywords

```
class       const       enum        export      fn
impl        import      interface   let         mod
priv        pub         static      struct      type
var
```

#### Control Flow Keywords

```
break       case        continue    default     else
for         if          in          loop        match
return      when        while
```

#### Type Keywords

```
bool        char        float       int         string
uint        void
```

#### Literal Keywords

```
false       null        true
```

#### Expression Keywords

```
as          is          new         this        super
```

#### Concurrency Keywords

```
async       await       receive     select      send
spawn
```

#### Error Handling Keywords

```
catch       finally     throw       try
```

#### Modifier Keywords

```
abstract    extends     mut         override
```

### 6.2 Contextual Keywords

These identifiers have special meaning only in specific contexts:

```
get         set         // Property accessors
where                   // Generic constraints
yield                   // Generator (future)
```

### 6.3 Reserved for Future Use

The following keywords are reserved for potential future features:

```
async       await       yield       macro       unsafe
do          goto        with        from
```

### 6.4 Keyword Table

Complete alphabetical list:

| Keyword     | Category    | Description           |
| ----------- | ----------- | --------------------- |
| `abstract`  | Modifier    | Abstract class/method |
| `as`        | Expression  | Type cast             |
| `async`     | Concurrency | Async function marker |
| `await`     | Concurrency | Await async result    |
| `bool`      | Type        | Boolean type          |
| `break`     | Control     | Exit loop             |
| `case`      | Control     | Match case            |
| `catch`     | Error       | Exception handler     |
| `char`      | Type        | Character type        |
| `class`     | Declaration | Reference type        |
| `const`     | Declaration | Compile-time constant |
| `continue`  | Control     | Next loop iteration   |
| `default`   | Control     | Default case          |
| `else`      | Control     | Alternative branch    |
| `enum`      | Declaration | Enumeration type      |
| `export`    | Declaration | Public export         |
| `extends`   | Modifier    | Inheritance           |
| `false`     | Literal     | Boolean false         |
| `finally`   | Error       | Cleanup block         |
| `float`     | Type        | Floating-point type   |
| `fn`        | Declaration | Function              |
| `for`       | Control     | For loop              |
| `if`        | Control     | Conditional           |
| `impl`      | Declaration | Implementation block  |
| `import`    | Declaration | Import module         |
| `in`        | Control     | Iterator keyword      |
| `int`       | Type        | Integer type          |
| `interface` | Declaration | Interface/trait       |
| `is`        | Expression  | Type check            |
| `let`       | Declaration | Immutable binding     |
| `loop`      | Control     | Infinite loop         |
| `match`     | Control     | Pattern matching      |
| `mod`       | Declaration | Module                |
| `mut`       | Modifier    | Mutable reference     |
| `new`       | Expression  | Object creation       |
| `null`      | Literal     | Null value            |
| `override`  | Modifier    | Override method       |
| `priv`      | Modifier    | Private visibility    |
| `pub`       | Modifier    | Public visibility     |
| `receive`   | Concurrency | Channel receive       |
| `return`    | Control     | Function return       |
| `select`    | Concurrency | Channel select        |
| `send`      | Concurrency | Channel send          |
| `spawn`     | Concurrency | Spawn fiber           |
| `static`    | Modifier    | Static member         |
| `string`    | Type        | String type           |
| `struct`    | Declaration | Value type            |
| `super`     | Expression  | Parent class          |
| `this`      | Expression  | Current instance      |
| `throw`     | Error       | Throw exception       |
| `true`      | Literal     | Boolean true          |
| `try`       | Error       | Try block             |
| `type`      | Declaration | Type alias            |
| `uint`      | Type        | Unsigned integer      |
| `var`       | Declaration | Mutable binding       |
| `void`      | Type        | No value type         |
| `when`      | Control     | Pattern guard         |
| `while`     | Control     | While loop            |

---

## 7. Literals

### 7.1 Integer Literals

Integer literals represent whole numbers:

```
IntegerLiteral ::= DecimalLiteral | HexLiteral | OctalLiteral | BinaryLiteral
DecimalLiteral ::= DecimalDigits IntegerSuffix?
HexLiteral     ::= '0' ('x'|'X') HexDigits IntegerSuffix?
OctalLiteral   ::= '0' ('o'|'O') OctalDigits IntegerSuffix?
BinaryLiteral  ::= '0' ('b'|'B') BinaryDigits IntegerSuffix?

DecimalDigits  ::= DecimalDigit (DecimalDigit | '_')*
HexDigits      ::= HexDigit (HexDigit | '_')*
OctalDigits    ::= OctalDigit (OctalDigit | '_')*
BinaryDigits   ::= BinaryDigit (BinaryDigit | '_')*

DecimalDigit   ::= '0'..'9'
HexDigit       ::= '0'..'9' | 'a'..'f' | 'A'..'F'
OctalDigit     ::= '0'..'7'
BinaryDigit    ::= '0' | '1'

IntegerSuffix  ::= 'i8' | 'i16' | 'i32' | 'i64'
                 | 'u8' | 'u16' | 'u32' | 'u64'
```

Examples:

```li
// Decimal
let a = 42
let b = 1_000_000        // Underscores for readability
let c = 0                // Zero

// Hexadecimal
let d = 0xFF
let e = 0xDEAD_BEEF

// Octal
let f = 0o755
let g = 0o177

// Binary
let h = 0b1010_1100
let i = 0b11111111

// With type suffix
let j = 255u8            // uint8
let k = -128i8           // int8
let l = 1_000_000i64     // int64
```

### 7.2 Floating-Point Literals

Floating-point literals represent decimal numbers:

```
FloatLiteral   ::= DecimalDigits '.' DecimalDigits Exponent? FloatSuffix?
                 | DecimalDigits Exponent FloatSuffix?
                 | DecimalDigits FloatSuffix

Exponent       ::= ('e'|'E') ('+'|'-')? DecimalDigits
FloatSuffix    ::= 'f32' | 'f64'
```

Examples:

```li
// Basic decimals
let a = 3.14159
let b = 0.5
let c = 10.0

// Scientific notation
let d = 6.022e23         // 6.022 × 10^23
let e = 1.6e-19          // 1.6 × 10^-19
let f = 1E10             // 1 × 10^10

// With underscores
let g = 1_234.567_890

// With type suffix
let h = 3.14f32          // float32
let i = 2.718f64         // float64

// Integer with float suffix
let j = 42f64            // 42.0 as float64
```

### 7.3 Boolean Literals

Boolean literals represent truth values:

```
BooleanLiteral ::= 'true' | 'false'
```

Examples:

```li
let active = true
let disabled = false
```

### 7.4 Character Literals

Character literals represent single Unicode code points:

```
CharLiteral    ::= "'" CharContent "'"
CharContent    ::= SingleChar | EscapeSequence
SingleChar     ::= <any Unicode except '\'' or '\' or LineTerminator>
```

Escape sequences:

```
EscapeSequence ::= SimpleEscape | UnicodeEscape
SimpleEscape   ::= '\' EscapeChar
EscapeChar     ::= 'n' | 'r' | 't' | '\\' | '\'' | '"' | '0'
UnicodeEscape  ::= '\u{' HexDigit{1,6} '}'
```

| Escape     | Character | Name               |
| ---------- | --------- | ------------------ |
| `\n`       | U+000A    | Newline            |
| `\r`       | U+000D    | Carriage return    |
| `\t`       | U+0009    | Tab                |
| `\\`       | U+005C    | Backslash          |
| `\'`       | U+0027    | Single quote       |
| `\"`       | U+0022    | Double quote       |
| `\0`       | U+0000    | Null               |
| `\u{XXXX}` | U+XXXX    | Unicode code point |

Examples:

```li
let a = 'A'
let newline = '\n'
let tab = '\t'
let quote = '\''
let backslash = '\\'
let emoji = '\u{1F600}'  // Emoji
let null_char = '\0'
```

### 7.5 String Literals

String literals represent sequences of characters:

```
StringLiteral  ::= '"' StringContent* '"'
                 | 'r"' RawStringContent* '"'
                 | '"""' MultiLineContent '"""'

StringContent  ::= StringChar | EscapeSequence | Interpolation
StringChar     ::= <any Unicode except '"' or '\' or LineTerminator>
RawStringContent ::= <any Unicode except '"'>
MultiLineContent ::= <any Unicode sequence except '"""'>

Interpolation  ::= '${' Expression '}'
```

Examples:

```li
// Simple strings
let s1 = "Hello, World!"
let s2 = "Line 1\nLine 2"
let s3 = "Tab:\tvalue"

// String interpolation
let name = "Alice"
let greeting = "Hello, ${name}!"
let expr = "2 + 2 = ${2 + 2}"

// Raw strings (no escape processing)
let path = r"C:\Users\helge\file.txt"
let regex = r"\d+\.\d+"

// Multi-line strings
let poem = """
    Roses are red,
    Violets are blue,
    Lira is great,
    And so are you!
    """

// Multi-line preserves indentation relative to closing quotes
let code = """
    fn main() {
        print("Hello!")
    }
    """
```

### 7.6 Null Literal

The null literal represents the absence of a value:

```
NullLiteral ::= 'null'
```

Null can only be assigned to optional types:

```li
let maybe: int? = null   // OK
// let never: int = null // ERROR: int is not optional
```

---

## 8. Operators and Punctuation

### 8.1 Arithmetic Operators

| Operator | Name           | Arity  | Example  |
| -------- | -------------- | ------ | -------- |
| `+`      | Addition       | Binary | `a + b`  |
| `-`      | Subtraction    | Binary | `a - b`  |
| `*`      | Multiplication | Binary | `a * b`  |
| `/`      | Division       | Binary | `a / b`  |
| `%`      | Remainder      | Binary | `a % b`  |
| `**`     | Exponentiation | Binary | `a ** b` |
| `-`      | Negation       | Unary  | `-a`     |

### 8.2 Comparison Operators

| Operator | Name             | Example  |
| -------- | ---------------- | -------- |
| `==`     | Equal            | `a == b` |
| `!=`     | Not equal        | `a != b` |
| `<`      | Less than        | `a < b`  |
| `>`      | Greater than     | `a > b`  |
| `<=`     | Less or equal    | `a <= b` |
| `>=`     | Greater or equal | `a >= b` |

### 8.3 Logical Operators

| Operator | Name        | Example    |
| -------- | ----------- | ---------- |
| `&&`     | Logical AND | `a && b`   |
| `\|\|`   | Logical OR  | `a \|\| b` |
| `!`      | Logical NOT | `!a`       |

Short-circuit evaluation:

- `&&` does not evaluate right operand if left is false
- `||` does not evaluate right operand if left is true

### 8.4 Bitwise Operators

| Operator | Name                     | Example   |
| -------- | ------------------------ | --------- |
| `&`      | Bitwise AND              | `a & b`   |
| `\|`     | Bitwise OR               | `a \| b`  |
| `^`      | Bitwise XOR              | `a ^ b`   |
| `~`      | Bitwise NOT              | `~a`      |
| `<<`     | Left shift               | `a << n`  |
| `>>`     | Right shift (arithmetic) | `a >> n`  |
| `>>>`    | Right shift (logical)    | `a >>> n` |

### 8.5 Assignment Operators

| Operator | Equivalent   | Example   |
| -------- | ------------ | --------- |
| `=`      | Assignment   | `a = b`   |
| `+=`     | `a = a + b`  | `a += b`  |
| `-=`     | `a = a - b`  | `a -= b`  |
| `*=`     | `a = a * b`  | `a *= b`  |
| `/=`     | `a = a / b`  | `a /= b`  |
| `%=`     | `a = a % b`  | `a %= b`  |
| `&=`     | `a = a & b`  | `a &= b`  |
| `\|=`    | `a = a \| b` | `a \|= b` |
| `^=`     | `a = a ^ b`  | `a ^= b`  |
| `<<=`    | `a = a << b` | `a <<= b` |
| `>>=`    | `a = a >> b` | `a >>= b` |

### 8.6 Special Operators

| Operator | Name              | Description                 |
| -------- | ----------------- | --------------------------- |
| `?.`     | Optional chaining | Access if not null          |
| `??`     | Null coalescing   | Default if null             |
| `?:`     | Elvis             | Return left if non-null     |
| `!`      | Force unwrap      | Unwrap optional (may panic) |
| `?`      | Propagation       | Propagate error/null        |
| `..`     | Range (exclusive) | `0..10` (0 to 9)            |
| `..=`    | Range (inclusive) | `0..=10` (0 to 10)          |
| `=>`     | Arrow             | Lambda, match arm           |
| `->`     | Return type       | Function signature          |
| `<-`     | Receive           | Channel receive             |

### 8.7 Punctuation

| Symbol  | Name        | Usage                 |
| ------- | ----------- | --------------------- |
| `{` `}` | Braces      | Blocks, objects       |
| `(` `)` | Parentheses | Grouping, calls       |
| `[` `]` | Brackets    | Arrays, indexing      |
| `;`     | Semicolon   | Statement terminator  |
| `:`     | Colon       | Type annotations      |
| `,`     | Comma       | Separator             |
| `.`     | Dot         | Member access         |
| `@`     | At          | Decorators/attributes |
| `#`     | Hash        | Compiler directives   |
| `$`     | Dollar      | Interpolation prefix  |
| `_`     | Underscore  | Discard, wildcards    |

### 8.8 Operator Precedence

From highest to lowest precedence:

| Level | Operators                        | Associativity |
| ----- | -------------------------------- | ------------- |
| 1     | `()` `[]` `.` `?.` `!` (postfix) | Left          |
| 2     | `!` `-` `~` (prefix)             | Right         |
| 3     | `**`                             | Right         |
| 4     | `*` `/` `%`                      | Left          |
| 5     | `+` `-`                          | Left          |
| 6     | `<<` `>>` `>>>`                  | Left          |
| 7     | `<` `<=` `>` `>=`                | Left          |
| 8     | `==` `!=`                        | Left          |
| 9     | `&`                              | Left          |
| 10    | `^`                              | Left          |
| 11    | `\|`                             | Left          |
| 12    | `&&`                             | Left          |
| 13    | `\|\|`                           | Left          |
| 14    | `??` `?:`                        | Right         |
| 15    | `..` `..=`                       | None          |
| 16    | `=` `+=` `-=` etc.               | Right         |
| 17    | `=>`                             | Right         |

---

## 9. Lexer State Machine

### 9.1 Lexer States

The Lira lexer operates as a finite state machine with the following states:

```
State       ::= Start | InIdentifier | InNumber | InString | InComment
              | InOperator | InCharacter | InInterpolation
```

### 9.2 State Transitions

```
Start:
  - Letter, '_'     → InIdentifier
  - Digit           → InNumber
  - '"'             → InString
  - '\''            → InCharacter
  - '/'             → CheckComment
  - Operator char   → InOperator
  - Whitespace      → Start (skip)
  - EOF             → Done

InIdentifier:
  - Letter, Digit, '_' → InIdentifier
  - Other              → Emit(Identifier or Keyword), Back to Start

InNumber:
  - Digit, '_'      → InNumber
  - '.'             → InFloat
  - 'e', 'E'        → InExponent
  - 'x', 'o', 'b'   → ChangeBase (if after '0')
  - Suffix chars    → InNumberSuffix
  - Other           → Emit(Number), Back to Start

InString:
  - '"'             → Emit(String), Back to Start
  - '\\'            → InEscape
  - '${'            → Start interpolation
  - Other           → InString

InComment:
  - LineTerminator  → Emit(Comment), Back to Start (for //)
  - '*/'            → Emit(Comment), Back to Start (for /*)
  - Other           → InComment
```

### 9.3 Error Recovery

The lexer reports errors for:

- Unterminated strings
- Invalid escape sequences
- Invalid number formats
- Unexpected characters
- Unterminated comments

After an error, the lexer attempts to recover by:

1. Skipping to the next whitespace or newline
2. Emitting an error token
3. Continuing lexing from the recovery point

### 9.4 Example Tokenization

Input:

```li
let x = 42 + 3.14
```

Tokens:

```
[
  Token(LET, "let", line=1, col=1),
  Token(IDENTIFIER, "x", line=1, col=5),
  Token(EQUALS, "=", line=1, col=7),
  Token(INTEGER, "42", line=1, col=9),
  Token(PLUS, "+", line=1, col=12),
  Token(FLOAT, "3.14", line=1, col=14),
  Token(EOF, "", line=1, col=18)
]
```

---

## Appendix A: ASCII Character Classification

| Range     | Classification        |
| --------- | --------------------- |
| 0x00-0x08 | Invalid               |
| 0x09      | Whitespace (tab)      |
| 0x0A      | Newline               |
| 0x0B-0x0C | Invalid               |
| 0x0D      | Newline (CR)          |
| 0x0E-0x1F | Invalid               |
| 0x20      | Whitespace (space)    |
| 0x21-0x2F | Operators/Punctuation |
| 0x30-0x39 | Digits                |
| 0x3A-0x40 | Operators/Punctuation |
| 0x41-0x5A | Letters (uppercase)   |
| 0x5B-0x60 | Operators/Punctuation |
| 0x61-0x7A | Letters (lowercase)   |
| 0x7B-0x7E | Operators/Punctuation |
| 0x7F      | Invalid               |
| 0x80+     | Unicode (extended)    |

---

## Appendix B: Complete Operator Table

```
+    -    *    /    %    **
++   --
==   !=   <    >    <=   >=
&&   ||   !
&    |    ^    ~    <<   >>   >>>
=    +=   -=   *=   /=   %=
&=   |=   ^=   <<=  >>=
?.   ??   ?:   ?    !
..   ..=
->   =>   <-
.    ,    :    ;
(    )    [    ]    {    }
@    #    $    _
```

---

_This document is part of the Lira Language Specification._
