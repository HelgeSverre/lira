# Lira Language Specification

**Version**: 1.0.0
**Status**: Draft
**Last Updated**: 2025-12-30
**Notation**: ISO/IEC 14977 EBNF with extensions

---

## Table of Contents

1. [Notation](#1-notation)
2. [Lexical Structure](#2-lexical-structure)
3. [Syntax](#3-syntax)
4. [Type System](#4-type-system)
5. [Semantics](#5-semantics)
6. [Concurrency Model](#6-concurrency-model)
7. [Memory Model](#7-memory-model)
8. [Standard Library Contracts](#8-standard-library-contracts)
9. [Validation](#9-validation)

---

## 1. Notation

This specification uses Extended Backus-Naur Form (EBNF) as defined in ISO/IEC 14977, with the following conventions:

```ebnf
(* EBNF Notation *)
definition     = symbol , "=" , expression , ";" ;
expression     = term , { "|" , term } ;
term           = factor , { factor } ;
factor         = symbol | terminal | group | option | repetition ;
group          = "(" , expression , ")" ;
option         = "[" , expression , "]" ;
repetition     = "{" , expression , "}" ;
terminal       = "'" , character , { character } , "'"
               | '"' , character , { character } , '"' ;
symbol         = letter , { letter | digit | "_" } ;

(* Extensions *)
(* ... *)       = comment
(* - *)         = exception (set difference)
(* + *)         = one or more repetition
(* ? *)         = zero or one (same as option)
```

**Type Inference Rules** use standard notation:
```
Γ ⊢ e : τ        (* In environment Γ, expression e has type τ *)
Γ, x:τ           (* Environment Γ extended with binding x:τ *)
τ₁ <: τ₂         (* τ₁ is a subtype of τ₂ *)
τ₁ ~ τ₂          (* τ₁ is compatible with τ₂ *)
```

---

## 2. Lexical Structure

### 2.1 Source Encoding

```ebnf
source_file = { source_element } , EOF ;
source_element = whitespace | comment | token ;
```

**Encoding**: UTF-8 (required)
**Line endings**: LF (U+000A) or CRLF (U+000D U+000A)

### 2.2 Whitespace

```ebnf
whitespace = whitespace_char , { whitespace_char } ;
whitespace_char = " " | "\t" | "\r" | "\n" ;
```

Whitespace is significant only for token separation.

### 2.3 Comments

```ebnf
comment = line_comment | block_comment | doc_comment ;

line_comment = "//" , { any_char - newline } , newline ;
block_comment = "/*" , { any_char | block_comment } , "*/" ;
doc_comment = "///" , { any_char - newline } , newline
            | "/**" , { any_char } , "*/" ;
```

**Note**: Block comments do NOT nest in the current implementation.

### 2.4 Identifiers

```ebnf
identifier = identifier_start , { identifier_continue } - keyword ;
identifier_start = letter | "_" ;
identifier_continue = letter | digit | "_" ;

letter = "A" | "B" | ... | "Z" | "a" | "b" | ... | "z" | unicode_letter ;
digit = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
unicode_letter = (* Any Unicode character with category L *) ;
```

**Reserved identifiers**:
- `_` (discard/wildcard pattern)
- Identifiers starting with `__` (reserved for implementation)

### 2.5 Keywords

```ebnf
keyword = declaration_keyword | control_keyword | type_keyword
        | expr_keyword | concurrency_keyword | modifier_keyword
        | literal_keyword ;

declaration_keyword = "let" | "var" | "const" | "fn" | "class" | "struct"
                    | "enum" | "interface" | "impl" | "trait" | "import"
                    | "export" | "mod" | "type" | "pub" | "priv" | "static" | "use" ;

control_keyword = "if" | "else" | "match" | "while" | "for" | "in"
                | "loop" | "break" | "continue" | "return"
                | "when" | "case" | "default" ;

type_keyword = "int" | "float" | "bool" | "string" | "char" | "void" ;

expr_keyword = "as" | "is" | "this" | "super" | "self" | "Self" ;

concurrency_keyword = "spawn" | "select" | "send" | "receive" | "async" | "await" ;

modifier_keyword = "abstract" | "extends" | "mut" | "override" | "where" ;

literal_keyword = "true" | "false" | "null" ;

error_keyword = "try" | "catch" | "finally" | "throw" ;
```

**Total keywords**: 52

### 2.6 Literals

#### 2.6.1 Integer Literals

```ebnf
integer_literal = decimal_literal | hex_literal | octal_literal | binary_literal ;

decimal_literal = "0" | ( nonzero_digit , { digit | "_" } ) ;
hex_literal = "0" , ( "x" | "X" ) , hex_digit , { hex_digit | "_" } ;
octal_literal = "0" , ( "o" | "O" ) , octal_digit , { octal_digit | "_" } ;
binary_literal = "0" , ( "b" | "B" ) , binary_digit , { binary_digit | "_" } ;

nonzero_digit = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
hex_digit = digit | "a" | "b" | "c" | "d" | "e" | "f"
                  | "A" | "B" | "C" | "D" | "E" | "F" ;
octal_digit = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" ;
binary_digit = "0" | "1" ;
```

**Semantics**: Parsed as `int64` (64-bit signed integer).
**Range**: -2⁶³ to 2⁶³ - 1

#### 2.6.2 Float Literals

```ebnf
float_literal = decimal_float | exponent_float ;

decimal_float = digit_sequence , "." , digit_sequence , [ exponent_part ] ;
exponent_float = digit_sequence , exponent_part ;

digit_sequence = digit , { digit | "_" } ;
exponent_part = ( "e" | "E" ) , [ "+" | "-" ] , digit_sequence ;
```

**Semantics**: Parsed as `float64` (IEEE 754 double precision).

#### 2.6.3 String Literals

```ebnf
string_literal = '"' , { string_char | escape_sequence | interpolation } , '"' ;

string_char = any_char - ( '"' | "\\" | "$" | newline ) ;
escape_sequence = "\\" , ( "n" | "r" | "t" | "\\" | '"' | "'" | "0" | unicode_escape ) ;
unicode_escape = "u" , "{" , hex_digit , { hex_digit } , "}" ;
interpolation = "${" , expression , "}" ;
```

**Encoding**: UTF-8
**Escape sequences**:
| Escape | Meaning |
|--------|---------|
| `\n` | Newline (U+000A) |
| `\r` | Carriage return (U+000D) |
| `\t` | Tab (U+0009) |
| `\\` | Backslash |
| `\"` | Double quote |
| `\'` | Single quote |
| `\0` | Null (U+0000) |
| `\u{XXXX}` | Unicode code point (1-6 hex digits) |

#### 2.6.4 Character Literals

```ebnf
char_literal = "'" , ( char_char | escape_sequence ) , "'" ;
char_char = any_char - ( "'" | "\\" | newline ) ;
```

**Semantics**: Single Unicode scalar value (4 bytes).

#### 2.6.5 Boolean Literals

```ebnf
bool_literal = "true" | "false" ;
```

#### 2.6.6 Null Literal

```ebnf
null_literal = "null" ;
```

### 2.7 Operators

```ebnf
operator = arithmetic_op | comparison_op | logical_op | bitwise_op
         | assignment_op | special_op ;

arithmetic_op = "+" | "-" | "*" | "/" | "%" | "**" | "++" | "--" ;
comparison_op = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
logical_op = "&&" | "||" | "!" ;
bitwise_op = "&" | "|" | "^" | "~" | "<<" | ">>" | ">>>" ;
assignment_op = "=" | "+=" | "-=" | "*=" | "/=" | "%="
              | "&=" | "|=" | "^=" | "<<=" | ">>=" ;
special_op = "?." | "??" | "?:" | "?" | ".." | "..=" | "->" | "=>" | "<-" ;
```

### 2.8 Delimiters

```ebnf
delimiter = "(" | ")" | "{" | "}" | "[" | "]"
          | "," | ":" | "::" | ";" | "." | "@" | "#" | "$" | "_" ;
```

### 2.9 Operator Precedence

| Precedence | Operators | Associativity | Description |
|------------|-----------|---------------|-------------|
| 16 | `()` `[]` `.` `?.` `!` (postfix) `++` `--` (postfix) | Left | Call, index, access |
| 15 | `as` `is` | Left | Type operations |
| 14 | `-` `!` `~` `++` `--` (prefix) | Right | Unary |
| 13 | `**` | Right | Exponentiation |
| 12 | `*` `/` `%` | Left | Multiplicative |
| 11 | `+` `-` | Left | Additive |
| 10 | `<<` `>>` `>>>` | Left | Shift |
| 9 | `<` `<=` `>` `>=` | Left | Relational |
| 8 | `==` `!=` | Left | Equality |
| 7 | `&` | Left | Bitwise AND |
| 6 | `^` | Left | Bitwise XOR |
| 5 | `\|` | Left | Bitwise OR |
| 4 | `&&` | Left | Logical AND |
| 3 | `\|\|` | Left | Logical OR |
| 2 | `..` `..=` | Non-assoc | Range |
| 1 | `??` | Right | Null coalescing |
| 0 | `=` `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` | Right | Assignment |

---

## 3. Syntax

### 3.1 Program Structure

```ebnf
program = { statement } ;

statement = declaration | control_statement | expression_statement ;
expression_statement = expression , [ ";" ] ;
```

### 3.2 Declarations

#### 3.2.1 Variable Declarations

```ebnf
variable_decl = ( "let" | "var" ) , binding_pattern , [ ":" , type_expr ] , [ "=" , expression ] ;

binding_pattern = identifier
                | "(" , binding_pattern , { "," , binding_pattern } , ")"
                | "{" , field_pattern , { "," , field_pattern } , "}" ;

field_pattern = identifier , [ ":" , binding_pattern ] ;
```

**Semantics**:
- `let` declares an immutable binding
- `var` declares a mutable binding
- Type annotation is optional (inferred if omitted)
- Initializer is required for `let`, optional for `var`

#### 3.2.2 Constant Declarations

```ebnf
const_decl = "const" , identifier , [ ":" , type_expr ] , "=" , expression ;
```

**Semantics**: Compile-time constant. Expression must be evaluable at compile time.

#### 3.2.3 Function Declarations

```ebnf
function_decl = [ "pub" ] , [ "override" ] , "fn" , identifier , [ type_params ] ,
                "(" , [ parameters ] , ")" , [ "->" , type_expr ] ,
                ( block | "=>" , expression ) ;

type_params = "<" , type_param , { "," , type_param } , ">" ;
type_param = identifier , [ ":" , trait_bounds ] ;
trait_bounds = identifier , { "+" , identifier } ;

parameters = parameter , { "," , parameter } ;
parameter = ( "self" , [ "mut" ] )
          | ( identifier , ":" , type_expr , [ "=" , expression ] ) ;
```

**Semantics**:
- `pub` makes function publicly visible
- `override` indicates method override in subclass
- `self` or `self mut` for instance methods
- Default parameter values supported
- Return type inferred if omitted (except for recursive functions)

#### 3.2.4 Struct Declarations

```ebnf
struct_decl = "struct" , identifier , [ type_params ] , "{" , { struct_member } , "}" ;

struct_member = field_decl | method_decl ;
field_decl = [ "pub" ] , [ "let" | "var" ] , identifier , ":" , type_expr , [ "," ] ;
method_decl = [ "pub" ] , "fn" , identifier , "(" , [ parameters ] , ")" ,
              [ "->" , type_expr ] , block ;
```

**Semantics**: Value type with copy semantics.

#### 3.2.5 Class Declarations

```ebnf
class_decl = "class" , identifier , [ "extends" , identifier ] ,
             [ ":" , identifier , { "," , identifier } ] ,
             "{" , { class_member } , "}" ;

class_member = field_decl | method_decl ;
```

**Semantics**: Reference type with identity and inheritance.

#### 3.2.6 Enum Declarations

```ebnf
enum_decl = "enum" , identifier , "{" , enum_variant , { "," , enum_variant } , [ "," ] , "}" ;

enum_variant = identifier , [ "(" , type_expr , { "," , type_expr } , ")" ]
             | identifier , "=" , integer_literal ;
```

**Semantics**: Sum type with optional associated data.

#### 3.2.7 Trait Declarations

```ebnf
trait_decl = [ "pub" ] , "trait" , identifier , [ type_params ] , "{" , { trait_method } , "}" ;

trait_method = "fn" , identifier , "(" , [ trait_params ] , ")" ,
               [ "->" , type_expr ] , [ block ] ;
trait_params = ( "self" , [ "mut" ] , [ "," ] ) , [ parameters ] ;
```

**Semantics**: Nominal subtyping contract.

#### 3.2.8 Impl Blocks

```ebnf
impl_decl = "impl" , [ type_params ] , [ identifier , "for" ] , type_expr ,
            "{" , { impl_method } , "}" ;

impl_method = [ "pub" ] , "fn" , identifier , "(" , [ parameters ] , ")" ,
              [ "->" , type_expr ] , block ;
```

**Semantics**:
- `impl Type { }` - inherent implementation
- `impl Trait for Type { }` - trait implementation

#### 3.2.9 Interface Declarations

```ebnf
interface_decl = "interface" , identifier , "{" , { interface_method } , "}" ;
interface_method = "fn" , identifier , "(" , [ parameters ] , ")" , [ "->" , type_expr ] ;
```

**Semantics**: Structural compatibility contract. The current parser accepts no
generic parameter list, associated-type declaration, variance annotation, or
method body in an interface. Parameter default expressions are allowed by the
shared parameter grammar and are part of the method's compatibility contract.

#### 3.2.10 Type Aliases

```ebnf
type_alias = "type" , identifier , "=" , type_expr ;
```

#### 3.2.11 Import Statements

```ebnf
import_stmt = "import" , module_path , { "." , module_path } ,
              [ "." , "{" , identifier , { "," , identifier } , "}" ] ;

use_stmt = "use" , module_path , { "::" , module_path } ,
           [ "::" , "{" , identifier , { "," , identifier } , "}" ]
           [ "as" , identifier ] ;

module_path = identifier ;
```

### 3.3 Control Flow Statements

```ebnf
control_statement = if_stmt | while_stmt | for_stmt | loop_stmt
                  | return_stmt | break_stmt | continue_stmt | block ;

if_stmt = "if" , expression , block , [ "else" , ( if_stmt | block ) ] ;
while_stmt = "while" , expression , block ;
for_stmt = "for" , identifier , "in" , expression , block ;
loop_stmt = "loop" , block ;
return_stmt = "return" , [ expression ] ;
break_stmt = "break" , [ expression ] ;
continue_stmt = "continue" ;
block = "{" , { statement } , "}" ;
```

### 3.4 Expressions

```ebnf
expression = assignment_expr ;

assignment_expr = conditional_expr , [ assignment_op , assignment_expr ] ;

conditional_expr = or_expr , [ "??" , conditional_expr ] ;

or_expr = and_expr , { "||" , and_expr } ;
and_expr = bitor_expr , { "&&" , bitor_expr } ;
bitor_expr = bitxor_expr , { "|" , bitxor_expr } ;
bitxor_expr = bitand_expr , { "^" , bitand_expr } ;
bitand_expr = equality_expr , { "&" , equality_expr } ;
equality_expr = relational_expr , { ( "==" | "!=" ) , relational_expr } ;
relational_expr = shift_expr , { ( "<" | "<=" | ">" | ">=" ) , shift_expr } ;
shift_expr = additive_expr , { ( "<<" | ">>" | ">>>" ) , additive_expr } ;
additive_expr = multiplicative_expr , { ( "+" | "-" ) , multiplicative_expr } ;
multiplicative_expr = power_expr , { ( "*" | "/" | "%" ) , power_expr } ;
power_expr = unary_expr , [ "**" , power_expr ] ;

unary_expr = ( "-" | "!" | "~" | "++" | "--" ) , unary_expr
           | postfix_expr ;

postfix_expr = primary_expr , { postfix_op } ;
postfix_op = call_expr | index_expr | field_expr | type_op | increment_op | try_op ;

call_expr = "(" , [ arguments ] , ")" ;
arguments = argument , { "," , argument } ;
argument = [ identifier , ":" ] , expression ;

index_expr = "[" , expression , "]" ;
field_expr = ( "." | "?." ) , identifier , [ type_args ] , [ call_expr ] ;
type_args = "::<" , type_expr , { "," , type_expr } , ">" ;

type_op = ( "as" | "is" ) , type_expr ;
increment_op = "++" | "--" ;
try_op = "?" ;
```

#### 3.4.1 Primary Expressions

```ebnf
primary_expr = literal | identifier | "this" | "super" | "self"
             | group_expr | array_expr | tuple_expr | map_expr | struct_expr
             | if_expr | match_expr | lambda_expr | spawn_expr | select_expr
             | range_expr | path_expr | block ;

group_expr = "(" , expression , ")" ;
array_expr = "[" , [ expression , { "," , expression } , [ "," ] ] , "]" ;
tuple_expr = "(" , [ expression , "," , [ expression , { "," , expression } ] ] , ")" ;
map_expr = "{" , [ map_entry , { "," , map_entry } , [ "," ] ] , "}" ;
map_entry = expression , ":" , expression ;
struct_expr = identifier , "{" , [ field_init , { "," , field_init } , [ "," ] ] , "}" ;
field_init = identifier , ":" , expression ;

if_expr = "if" , expression , block , "else" , ( if_expr | block ) ;
match_expr = "match" , expression , "{" , { match_arm } , "}" ;
match_arm = pattern , [ "if" , expression ] , "=>" , expression , [ "," ] ;

lambda_expr = "|" , [ lambda_params ] , "|" , ( expression | block ) ;
lambda_params = lambda_param , { "," , lambda_param } ;
lambda_param = identifier , [ ":" , type_expr ] ;

spawn_expr = "spawn" , expression ;
select_expr = "select" , "{" , { select_arm } , "}" ;
select_arm = select_case , "=>" , expression , [ "," ] ;
select_case = "<-" , expression
            | identifier , "=" , "<-" , expression
            | expression , "->" , expression
            | "_" ;

range_expr = [ expression ] , ( ".." | "..=" ) , [ expression ] ;
path_expr = identifier , { "::" , identifier } ;
```

### 3.5 Patterns

```ebnf
pattern = "_"
        | literal
        | identifier
        | tuple_pattern
        | struct_pattern
        | constructor_pattern
        | range_pattern
        | or_pattern
        | binding_pattern ;

tuple_pattern = "(" , [ pattern , { "," , pattern } ] , ")" ;
struct_pattern = identifier , "{" , [ field_pattern , { "," , field_pattern } ] , "}" ;
constructor_pattern = identifier , [ "::" , identifier ] , [ "(" , [ pattern , { "," , pattern } ] , ")" ] ;
range_pattern = pattern , ( ".." | "..=" ) , pattern ;
or_pattern = pattern , "|" , pattern ;
binding_pattern = identifier , "@" , pattern ;
```

### 3.6 Type Expressions

```ebnf
type_expr = named_type | generic_type | optional_type | function_type
          | tuple_type | array_type | path_type | "Self" ;

named_type = identifier ;
generic_type = identifier , "<" , type_expr , { "," , type_expr } , ">" ;
optional_type = type_expr , "?" ;
function_type = "fn" , "(" , [ type_list ] , ")" , "->" , type_expr ;
tuple_type = "(" , [ type_expr , { "," , type_expr } ] , ")" ;
array_type = "[" , type_expr , "]" ;
path_type = identifier , { "::" , identifier } ;

type_list = type_expr , { "," , type_expr } ;
```

---

## 4. Type System

### 4.1 Type Universe

```
Type ::= Primitive | Compound | UserDefined | Special

Primitive ::= int | int8 | int16 | int32 | int64
            | uint8 | uint16 | uint32 | uint64
            | float | bool | string | char | void

Compound ::= [T] | Tuple<T₁, ..., Tₙ> | Map<K, V>
           | Optional<T> | Result<T, E> | Function<P, R>

UserDefined ::= Struct(name) | Class(name) | Enum(name)
              | Interface(name) | Trait(name) | TypeAlias(name, T)

Special ::= Any | Never | Unknown | TypeParam(name) | Self
```

### 4.2 Primitive Type Sizes

| Type | Size (bytes) | Range |
|------|--------------|-------|
| `int8` | 1 | -128 to 127 |
| `int16` | 2 | -32,768 to 32,767 |
| `int32` | 4 | -2³¹ to 2³¹-1 |
| `int64` / `int` | 8 | -2⁶³ to 2⁶³-1 |
| `uint8` / `byte` | 1 | 0 to 255 |
| `uint16` | 2 | 0 to 65,535 |
| `uint32` | 4 | 0 to 2³²-1 |
| `uint64` | 8 | 0 to 2⁶⁴-1 |
| `float` | 8 | IEEE 754 double |
| `bool` | 1 | true, false |
| `char` | 4 | Unicode scalar |

### 4.3 Type Compatibility

String indexing uses a zero-based Unicode scalar index and produces a
one-character `string`. A negative index or an index past the final scalar is a
runtime error. This is distinct from a `char` literal, whose value is a Unicode
scalar code point.

**Definition**: Type τ₁ is compatible with type τ₂ (written τ₁ ~ τ₂) if values of τ₁ can be used where τ₂ is expected.

```
(* Reflexivity *)
τ ~ τ

(* Any type *)
τ ~ Any
Any ~ τ

(* Unknown type - error recovery *)
τ ~ Unknown
Unknown ~ τ

(* Type parameters *)
TypeParam(a) ~ TypeParam(a)
τ ~ TypeParam(_)  (* Any concrete type matches type parameter *)

(* Optional types *)
null ~ Optional<τ>
Optional<τ₁> ~ Optional<τ₂>  ⟺  τ₁ ~ τ₂
τ ~ Optional<τ>  (* Promotion *)

(* Result types *)
Result<τ₁, ε₁> ~ Result<τ₂, ε₂>  ⟺  τ₁ ~ τ₂ ∧ ε₁ ~ ε₂

(* Mutable containers are invariant to prevent unsound aliases *)
 [τ₁] ~ [τ₂]  ⟺  τ₁ = τ₂
Channel<τ₁> ~ Channel<τ₂>  ⟺  τ₁ ~ τ₂ ∧ τ₂ ~ τ₁
Map<κ₁, τ₁> ~ Map<κ₂, τ₂>  ⟺
    κ₁ ~ κ₂ ∧ κ₂ ~ κ₁ ∧ τ₁ ~ τ₂ ∧ τ₂ ~ τ₁

(* Tuple positions are checked element-by-element *)
(τ₁, ..., τₙ) ~ (σ₁, ..., σₙ)  ⟺  ∀i. τᵢ ~ σᵢ

An array literal may initialize an explicitly annotated array when each
literal element is compatible with the annotation. This is construction of a
new mutable value; an existing array value cannot be assigned across element
types. An explicitly typed `[Animal]` may receive a `Dog` element when `Dog` is
compatible with `Animal`.

(* Numeric coercion *)
int ~ float
float ~ int
IntegerType ~ IntegerType  (* All integer types are compatible *)
IntegerType ~ float

(* Function types *)
fn(P₁) → R₁ ~ fn(P₂) → R₂  ⟺  P₁ ~ P₂ ∧ R₁ ~ R₂

(* Structural interfaces *)
τ ~ Interface(I)  ⟺  every method required by I occurs on τ with a compatible
                         receiver-stripped signature and default-parameter mask
Interface(I₁) ~ Interface(I₂)  ⟺  I₁'s required methods satisfy I₂'s contract
```

### 4.4 Subtyping

```
(* Class inheritance *)
class B extends A  ⟹  B <: A

(* Explicit class interface declarations are validated using the same
   structural method relation; they do not make an otherwise incompatible
   type compatible. *)
class C : I  ⟹  C satisfies Interface(I)

(* Trait implementation *)
impl T for S  ⟹  S <: T  (* for trait bounds *)

(* Never is bottom type *)
Never <: τ  (* for all types τ *)

(* Transitivity *)
τ₁ <: τ₂ ∧ τ₂ <: τ₃  ⟹  τ₁ <: τ₃
```

### 4.5 Type Inference Rules

#### 4.5.1 Literals

```
─────────────────────────
Γ ⊢ n : int              (* integer literal *)

─────────────────────────
Γ ⊢ f : float            (* float literal *)

─────────────────────────
Γ ⊢ "s" : string         (* string literal *)

─────────────────────────
Γ ⊢ 'c' : char           (* char literal *)

─────────────────────────
Γ ⊢ true : bool

─────────────────────────
Γ ⊢ false : bool

─────────────────────────
Γ ⊢ null : null
```

#### 4.5.2 Variables

```
x : τ ∈ Γ
─────────────────────────
Γ ⊢ x : τ
```

#### 4.5.3 Variable Declarations

```
Γ ⊢ e : τ
─────────────────────────────────────
Γ ⊢ let x = e : void    Γ' = Γ, x:τ

Γ ⊢ e : τ    τ ~ τ'
─────────────────────────────────────
Γ ⊢ let x: τ' = e : void    Γ' = Γ, x:τ'
```

#### 4.5.4 Binary Operations

```
Γ ⊢ e₁ : τ₁    Γ ⊢ e₂ : τ₂    τ₁, τ₂ ∈ Numeric    τ = unify(τ₁, τ₂)
──────────────────────────────────────────────────────────────────────
Γ ⊢ e₁ + e₂ : τ    (* Also: -, *, /, %, ** *)

Γ ⊢ e₁ : τ₁    Γ ⊢ e₂ : τ₂    τ₁ ~ τ₂
──────────────────────────────────────────────────────────────────────
Γ ⊢ e₁ == e₂ : bool    (* Also: !=, <, <=, >, >= *)

Γ ⊢ e₁ : bool    Γ ⊢ e₂ : bool
──────────────────────────────────────────────────────────────────────
Γ ⊢ e₁ && e₂ : bool    (* Also: || *)

Γ ⊢ e₁ : τ₁    Γ ⊢ e₂ : τ₂    τ₁, τ₂ ∈ Integer
──────────────────────────────────────────────────────────────────────
Γ ⊢ e₁ & e₂ : int    (* Also: |, ^, <<, >>, >>> *)
```

#### 4.5.5 Function Calls

```
Γ ⊢ f : fn(τ₁, ..., τₙ) → τᵣ    Γ ⊢ eᵢ : τᵢ' ∧ τᵢ' ~ τᵢ  (for i ∈ 1..n)
──────────────────────────────────────────────────────────────────────────────
Γ ⊢ f(e₁, ..., eₙ) : τᵣ
```

#### 4.5.6 Field Access

```
Γ ⊢ e : S    S has field f : τ
─────────────────────────────────────
Γ ⊢ e.f : τ

Γ ⊢ e : Optional<S>    S has field f : τ
─────────────────────────────────────────────
Γ ⊢ e?.f : Optional<τ>
```

#### 4.5.7 Method Calls

```
Γ ⊢ e : τ    τ has method m : fn(Self, τ₁, ..., τₙ) → τᵣ
Γ ⊢ eᵢ : τᵢ' ∧ τᵢ' ~ τᵢ  (for i ∈ 1..n)
──────────────────────────────────────────────────────────────────────────────
Γ ⊢ e.m(e₁, ..., eₙ) : τᵣ
```

#### 4.5.8 If Expressions

```
Γ ⊢ c : bool    Γ ⊢ e₁ : τ₁    Γ ⊢ e₂ : τ₂    τ₁ ~ τ₂
──────────────────────────────────────────────────────────
Γ ⊢ if c { e₁ } else { e₂ } : τ₁
```

#### 4.5.9 Match Expressions

```
Γ ⊢ e : τₛ    Γ, bindings(pᵢ, τₛ) ⊢ eᵢ : τᵢ    all τᵢ compatible
──────────────────────────────────────────────────────────────────────
Γ ⊢ match e { p₁ => e₁, ..., pₙ => eₙ } : τ₁
```

#### 4.5.10 Lambda Expressions

```
Γ, x₁:τ₁, ..., xₙ:τₙ ⊢ e : τᵣ
──────────────────────────────────────────────────────────
Γ ⊢ |x₁: τ₁, ..., xₙ: τₙ| e : fn(τ₁, ..., τₙ) → τᵣ
```

#### 4.5.11 Generics

```
fn f<T>(x: T) → T defined    Γ ⊢ e : τ
──────────────────────────────────────────────
Γ ⊢ f(e) : τ    (* T instantiated to τ *)

fn f<T: Bound>(x: T) → T defined    Γ ⊢ e : τ    τ implements Bound
──────────────────────────────────────────────────────────────────────
Γ ⊢ f(e) : τ
```

### 4.6 Type Checking Phases

The type checker operates in 5 sequential passes:

1. **Register Type Names**: Create placeholder entries for all type definitions
2. **Collect Type Definitions**: Populate fields, methods, variants
3. **Collect Traits and Impls**: Register trait definitions and implementations
4. **Register Function Signatures**: Process all function declarations
5. **Check Statements and Expressions**: Validate types recursively

---

## 5. Semantics

### 5.1 Evaluation Order

Expressions are evaluated **left-to-right** with the following exceptions:
- Assignment operators evaluate the right-hand side first
- Short-circuit operators (`&&`, `||`) may not evaluate the right operand

### 5.2 Variable Binding

```
⟦let x = e⟧(σ) = σ[x ↦ ⟦e⟧(σ)]
⟦var x = e⟧(σ) = σ[x ↦ ref(⟦e⟧(σ))]
```

- `let` bindings are immutable
- `var` bindings are mutable (stored as references)

### 5.3 Control Flow

```
⟦if c { e₁ } else { e₂ }⟧(σ) =
    if ⟦c⟧(σ) = true then ⟦e₁⟧(σ) else ⟦e₂⟧(σ)

⟦while c { e }⟧(σ) =
    if ⟦c⟧(σ) = true then ⟦while c { e }⟧(⟦e⟧(σ)) else σ

⟦for x in iter { e }⟧(σ) =
    fold (λσ', v. ⟦e⟧(σ'[x ↦ v])) σ (⟦iter⟧(σ))
```

### 5.4 Function Calls

```
⟦f(e₁, ..., eₙ)⟧(σ) =
    let vᵢ = ⟦eᵢ⟧(σ) for i ∈ 1..n
    let σ' = σ[p₁ ↦ v₁, ..., pₙ ↦ vₙ]  (* pᵢ are parameter names *)
    ⟦body_of(f)⟧(σ')
```

### 5.5 Pattern Matching

```
match(v, _) = Some(∅)
match(v, x) = Some({x ↦ v})
match(v, literal) = if v = literal then Some(∅) else None
match((v₁, ..., vₙ), (p₁, ..., pₙ)) =
    merge(match(v₁, p₁), ..., match(vₙ, pₙ))
match(Ctor(v₁, ..., vₙ), Ctor(p₁, ..., pₙ)) =
    merge(match(v₁, p₁), ..., match(vₙ, pₙ))
match(v, p₁ | p₂) = match(v, p₁) ∨ match(v, p₂)
```

### 5.6 Error Propagation

```
⟦e?⟧(σ) =
    match ⟦e⟧(σ) with
    | Ok(v) → v
    | Err(e) → return Err(e)  (* Early return *)
```

---

## 6. Concurrency Model

### 6.1 Fibers

Fibers are cooperative green threads scheduled by the runtime.

```
⟦spawn e⟧(σ) =
    let fiber_id = create_fiber(λ(). ⟦e⟧(σ))
    schedule(fiber_id)
    fiber_id
```

**Scheduling**: Round-robin cooperative scheduling. Fibers yield at:
- `yield` statements
- Channel operations (send/receive)
- I/O operations

### 6.2 Channels

```
Channel<T> = {
    buffer: Queue<T>,
    capacity: int,
    senders: WaitQueue,
    receivers: WaitQueue,
}

⟦chan(n)⟧ = Channel { buffer: [], capacity: n, ... }
⟦chan()⟧ = Channel { buffer: [], capacity: 0, ... }  (* Unbuffered *)

⟦send(ch, v)⟧ =
    if ch.buffer.len < ch.capacity then
        ch.buffer.push(v)
    else
        suspend_current_fiber(ch.senders)
        ch.buffer.push(v)
    wake_one(ch.receivers)

⟦recv(ch)⟧ =
    if ch.buffer.len > 0 then
        let v = ch.buffer.pop()
        wake_one(ch.senders)
        v
    else
        suspend_current_fiber(ch.receivers)
        ch.buffer.pop()
```

### 6.3 Select

```
⟦select { case₁ => e₁, ..., caseₙ => eₙ }⟧ =
    loop {
        for i in random_permutation(1..n) {
            if caseᵢ is ready then
                execute caseᵢ
                return ⟦eᵢ⟧
        }
        suspend_until_any_ready()
    }
```

---

## 7. Memory Model

### 7.1 Value Categories

- **Value types**: `int`, `float`, `bool`, `char`, structs, tuples
  - Copied on assignment
  - Stack allocated (when possible)

- **Reference types**: `string`, arrays, maps, classes, closures
  - Reference counted (ARC)
  - Heap allocated

### 7.2 Reference Counting

```
Reference = {
    value: T,
    count: AtomicU32,
}

clone(ref) = ref.count += 1; ref
drop(ref) =
    ref.count -= 1
    if ref.count = 0 then
        deallocate(ref)
```

### 7.3 Cycle Detection

The runtime uses a mark-and-sweep collector for cycles:
1. Track objects that may participate in cycles
2. Periodically scan for unreachable cycles
3. Break cycles and reclaim memory

---

## 8. Standard Library Contracts

### 8.1 Core Functions

```
fn print(value: any) -> void
    (* Writes string representation to stdout *)

fn println(value: any) -> void
    (* Writes string representation to stdout with newline *)

fn len(collection: any) -> int
    (* Returns number of elements *)

fn assert(condition: bool) -> void
    (* Panics if condition is false *)
    requires: condition = true
    ensures: returns normally
```

The string representation written by `print` and `println` is deterministic:
arrays use `[a, b]`, tuples use `(a, b)` (with `(a,)` for a singleton), and
maps, structs, classes, enum values, and `Result` values use `{key: value}`
with fields ordered lexicographically by key. Recursive aggregates are rendered
to a bounded depth and use `...` at a cycle or depth boundary. Opaque runtime
handles do not expose backend addresses or scheduler ids: functions and
closures render as `<function>`, channels as `<channel>`, and fibers as
`<fiber>`. Rendering one value beyond 8 MiB fails instead of emitting partial
or silently truncated output.

### 8.2 Collection Contracts

```
trait Collection<T> {
    fn len(self) -> int
        ensures: result >= 0

    fn is_empty(self) -> bool
        ensures: result = (self.len() = 0)

    fn push(self mut, value: T) -> void
        ensures: self.len() = old(self.len()) + 1

    fn pop(self mut) -> T?
        ensures: self.len() = max(0, old(self.len()) - 1)
}
```

---

## 9. Validation

### 9.1 Conformance Testing

The specification can be validated against the implementation through:

1. **Grammar Validation**
   - Extract EBNF from this specification
   - Generate parser using a parser generator (e.g., LALRPOP, pest)
   - Compare parsing results with `lirac` parser

2. **Type System Validation**
   - Property-based testing of type inference rules
   - Compare type checker output with expected types
   - Test type error detection

3. **Semantic Validation**
   - Execute test programs and verify behavior
   - Compare VM output with specification semantics

### 9.2 Test Suite Structure

```
tests/
├── spec/
│   ├── lexical/           # Token recognition tests
│   ├── syntax/            # Grammar conformance tests
│   ├── types/             # Type inference/checking tests
│   └── semantics/         # Behavioral tests
```

### 9.3 Validation Script

A validation tool can be built to:

```bash
# Proposed command
lira-spec-validate --spec docs/FORMAL_SPECIFICATION.md --impl ./crates

# Output
✓ Lexical: 52/52 keywords recognized
✓ Lexical: 48/48 operators recognized
✓ Syntax: 156/156 grammar productions valid
✓ Types: 89/89 inference rules satisfied
✓ Semantics: 234/234 behavioral tests pass
```

### 9.4 EBNF Extraction

The grammar can be extracted programmatically:

```rust
// Proposed: crates/lira-spec/src/extract.rs
pub fn extract_ebnf(spec_path: &Path) -> Grammar {
    // Parse markdown, extract code blocks marked as `ebnf`
    // Build Grammar structure for validation
}
```

### 9.5 Property-Based Type Tests

```rust
// Example property test for type inference
#[test]
fn prop_int_literal_has_int_type() {
    for n in random_integers() {
        let ast = parse(&format!("{}", n));
        let ty = infer_type(&ast);
        assert_eq!(ty, Type::Int);
    }
}

#[test]
fn prop_optional_promotion() {
    for ty in random_types() {
        // T is compatible with T?
        assert!(is_compatible(&ty, &Type::Optional(Box::new(ty.clone()))));
    }
}
```

---

## Appendix A: Complete Keyword List

| Keyword | Category | Description |
|---------|----------|-------------|
| `abstract` | Modifier | Abstract class/method |
| `as` | Expression | Type cast |
| `async` | Concurrency | Async function |
| `await` | Concurrency | Await async result |
| `bool` | Type | Boolean type |
| `break` | Control | Exit loop |
| `case` | Control | Match case |
| `catch` | Error | Exception handler |
| `char` | Type | Character type |
| `class` | Declaration | Class definition |
| `const` | Declaration | Constant binding |
| `continue` | Control | Next iteration |
| `default` | Control | Default case |
| `else` | Control | Else branch |
| `enum` | Declaration | Enum definition |
| `export` | Module | Export symbol |
| `extends` | Modifier | Inheritance |
| `false` | Literal | Boolean false |
| `finally` | Error | Cleanup block |
| `float` | Type | Float type |
| `fn` | Declaration | Function |
| `for` | Control | For loop |
| `if` | Control | Conditional |
| `impl` | Declaration | Implementation |
| `import` | Module | Import module |
| `in` | Control | For-in iterator |
| `int` | Type | Integer type |
| `interface` | Declaration | Interface |
| `is` | Expression | Type check |
| `let` | Declaration | Immutable binding |
| `loop` | Control | Infinite loop |
| `match` | Control | Pattern match |
| `mod` | Module | Module declaration |
| `mut` | Modifier | Mutable |
| `null` | Literal | Null value |
| `override` | Modifier | Override method |
| `priv` | Modifier | Private visibility |
| `pub` | Modifier | Public visibility |
| `receive` | Concurrency | Channel receive |
| `return` | Control | Return value |
| `select` | Concurrency | Channel select |
| `self` | Expression | Instance reference |
| `Self` | Type | Self type |
| `send` | Concurrency | Channel send |
| `spawn` | Concurrency | Spawn fiber |
| `static` | Modifier | Static member |
| `string` | Type | String type |
| `struct` | Declaration | Struct definition |
| `super` | Expression | Parent reference |
| `this` | Expression | Instance reference |
| `throw` | Error | Throw exception |
| `trait` | Declaration | Trait definition |
| `true` | Literal | Boolean true |
| `try` | Error | Try block |
| `type` | Declaration | Type alias |
| `use` | Module | Use path |
| `var` | Declaration | Mutable binding |
| `void` | Type | Void/unit type |
| `when` | Control | When clause |
| `where` | Modifier | Generic constraints |
| `while` | Control | While loop |

---

## Appendix B: Grammar Summary

**Productions**: ~85 grammar rules
**Keywords**: 52
**Operators**: 48
**Primitive types**: 14

---

## Appendix C: Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2025-12-30 | Initial specification |

---

*This specification is normative for the Lira programming language.*
