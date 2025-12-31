; Lira syntax highlighting queries for tree-sitter

; ============================================================================
; COMMENTS
; ============================================================================

(line_comment) @comment.line
(block_comment) @comment.block

; Doc comments get special treatment
((line_comment) @comment.documentation
  (#match? @comment.documentation "^///"))

((line_comment) @comment.documentation
  (#match? @comment.documentation "^//!"))

((block_comment) @comment.documentation
  (#match? @comment.documentation "^/\\*\\*"))

; ============================================================================
; LITERALS
; ============================================================================

(integer_literal) @constant.numeric.integer
(float_literal) @constant.numeric.float
(boolean_literal) @constant.builtin.boolean
(null_literal) @constant.builtin

(string_literal) @string
(raw_string_literal) @string
(multiline_string_literal) @string
(char_literal) @string.special.symbol

(escape_sequence) @constant.character.escape
(string_interpolation
  "${" @punctuation.special
  "}" @punctuation.special) @embedded

; ============================================================================
; KEYWORDS
; ============================================================================

; Declaration keywords
[
  "fn"
  "let"
  "var"
  "const"
  "struct"
  "class"
  "enum"
  "trait"
  "interface"
  "impl"
  "type"
] @keyword

; Control flow
[
  "if"
  "else"
  "match"
  "while"
  "for"
  "loop"
  "in"
  "break"
  "continue"
  "return"
] @keyword.control

; Exception handling
[
  "try"
  "catch"
  "finally"
  "throw"
] @keyword.control.exception

; Concurrency
[
  "spawn"
  "select"
  "async"
  "await"
] @keyword.coroutine

; Module keywords
[
  "import"
  "use"
  "pub"
  "priv"
  "internal"
] @keyword.import

; Object-oriented
[
  "new"
  "extends"
  "abstract"
  "override"
  "static"
] @keyword

; Type modifiers
[
  "mut"
  "where"
] @keyword.modifier

; ============================================================================
; TYPES
; ============================================================================

(type_identifier) @type
(primitive_type) @type.builtin
(self_type) @type.builtin

; Generic type parameters
(type_parameter
  name: (type_identifier) @type.parameter)

(generic_type
  (type_identifier) @type)

; Built-in collection types
((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "List" "Map" "Set" "Option" "Result" "Channel"))

; ============================================================================
; FUNCTIONS
; ============================================================================

; Function declarations
(function_declaration
  name: (identifier) @function)

(method_declaration
  name: (identifier) @function.method)

(trait_method
  name: (identifier) @function.method)

; Function calls
(call_expression
  function: (identifier) @function.call)

(method_call_expression
  method: (identifier) @function.method.call)

; Lambda parameters
(lambda_parameter
  name: (identifier) @variable.parameter)

; Parameters
(parameter
  name: (identifier) @variable.parameter)

; ============================================================================
; VARIABLES AND IDENTIFIERS
; ============================================================================

; Variable declarations
(variable_declaration
  pattern: (identifier) @variable)

; Self/this/super
(self_expression) @variable.builtin
(this_expression) @variable.builtin
(super_expression) @variable.builtin

; Field access
(member_expression
  member: (identifier) @variable.member)

(optional_chain_expression
  member: (identifier) @variable.member)

; Struct fields
(struct_field
  name: (identifier) @variable.member)

(class_field
  name: (identifier) @variable.member)

(struct_field_initializer
  name: (identifier) @variable.member)

; Identifiers in patterns
(wildcard_pattern) @variable.builtin

; Enum variants
(enum_variant
  name: (identifier) @constant)

; Path expressions (enum variants, etc.)
(path_expression
  (type_identifier) @type
  (identifier) @constant)

; Named arguments
(argument
  name: (identifier) @variable.parameter)

; ============================================================================
; DECLARATIONS
; ============================================================================

; Struct/class/enum/trait names
(struct_declaration
  name: (type_identifier) @type.definition)

(class_declaration
  name: (type_identifier) @type.definition)

(enum_declaration
  name: (type_identifier) @type.definition)

(trait_declaration
  name: (type_identifier) @type.definition)

(interface_declaration
  name: (type_identifier) @type.definition)

(type_alias
  name: (type_identifier) @type.definition)

; Impl blocks
(impl_block
  trait: (type_identifier) @type)

(impl_block
  type: (type_identifier) @type)

; ============================================================================
; OPERATORS
; ============================================================================

; Arithmetic
[
  "+"
  "-"
  "*"
  "/"
  "%"
  "**"
] @operator

; Comparison
[
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
] @operator

; Logical
[
  "&&"
  "||"
  "!"
] @operator

; Bitwise
[
  "&"
  "|"
  "^"
  "~"
  "<<"
  ">>"
  ">>>"
] @operator

; Assignment
[
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  "&="
  "|="
  "^="
  "<<="
  ">>="
] @operator

; Increment/Decrement
[
  "++"
  "--"
] @operator

; Range
[
  ".."
  "..="
] @operator

; Special operators
[
  "?"
  "??"
  "?."
  "->"
  "=>"
  "<-"
  "@"
  "::"
] @operator

; Type operators
"as" @keyword.operator
"is" @keyword.operator

; ============================================================================
; PUNCTUATION
; ============================================================================

; Brackets
[
  "("
  ")"
] @punctuation.bracket

[
  "["
  "]"
] @punctuation.bracket

[
  "{"
  "}"
] @punctuation.bracket

; Delimiters
[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter

; ============================================================================
; SPECIAL CASES
; ============================================================================

; Built-in functions
((identifier) @function.builtin
  (#any-of? @function.builtin
    "print" "println" "debug" "assert"
    "len" "push" "pop" "append"
    "panic" "todo" "unreachable"))

; Import paths
(module_path
  (identifier) @module)

(namespace_path
  (identifier) @module)

; Error handling
(try_expression
  "?" @operator)

; Channel operations
(receive_expression
  "<-" @operator)

(send_expression
  "->" @operator)
