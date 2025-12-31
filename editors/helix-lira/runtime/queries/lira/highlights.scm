; Lira syntax highlighting for Helix

; Comments
(line_comment) @comment.line
(block_comment) @comment.block

; Doc comments
((line_comment) @comment.documentation
  (#match? @comment.documentation "^///"))
((line_comment) @comment.documentation
  (#match? @comment.documentation "^//!"))

; Literals
(integer_literal) @constant.numeric.integer
(float_literal) @constant.numeric.float
(boolean_literal) @constant.builtin.boolean
(null_literal) @constant.builtin

(string_literal) @string
(raw_string_literal) @string
(multiline_string_literal) @string
(char_literal) @string.special.symbol
(escape_sequence) @constant.character.escape

; String interpolation
(string_interpolation
  "${" @punctuation.special
  "}" @punctuation.special) @embedded

; Keywords
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

[
  "if"
  "else"
  "match"
] @keyword.control.conditional

[
  "while"
  "for"
  "loop"
  "in"
] @keyword.control.repeat

[
  "break"
  "continue"
  "return"
] @keyword.control.return

[
  "try"
  "catch"
  "finally"
] @keyword.control.exception

[
  "spawn"
  "select"
  "async"
] @keyword.coroutine

[
  "import"
  "use"
  "pub"
  "priv"
  "internal"
] @keyword.control.import

[
  "new"
  "extends"
  "abstract"
  "override"
  "static"
] @keyword

[
  "mut"
  "where"
] @keyword.storage.modifier

; Type operators
"as" @keyword.operator
"is" @keyword.operator

; Types
(type_identifier) @type
(primitive_type) @type.builtin
(self_type) @type.builtin

(type_parameter
  name: (type_identifier) @type.parameter)

; Built-in collection types
((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "List" "Map" "Set" "Option" "Result" "Channel"))

; Functions
(function_declaration
  name: (identifier) @function)

(method_declaration
  name: (identifier) @function.method)

(call_expression
  function: (identifier) @function.call)

(method_call_expression
  method: (identifier) @function.method.call)

; Parameters
(parameter
  name: (identifier) @variable.parameter)

(lambda_parameter
  name: (identifier) @variable.parameter)

; Variables
(variable_declaration
  pattern: (identifier) @variable)

(self_expression) @variable.builtin
(this_expression) @variable.builtin
(super_expression) @variable.builtin

; Fields
(member_expression
  member: (identifier) @variable.other.member)

(struct_field
  name: (identifier) @variable.other.member)

(class_field
  name: (identifier) @variable.other.member)

; Enum variants
(enum_variant
  name: (identifier) @constant)

; Declarations
(struct_declaration
  name: (type_identifier) @type)

(class_declaration
  name: (type_identifier) @type)

(enum_declaration
  name: (type_identifier) @type)

(trait_declaration
  name: (type_identifier) @type)

; Operators
[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|" "^" "~" "<<" ">>" ">>>"
  "=" "+=" "-=" "*=" "/=" "%="
  "++" "--"
  ".." "..="
  "?" "??" "?." "->" "=>" "<-" "@" "::"
] @operator

; Punctuation
["(" ")"] @punctuation.bracket
["[" "]"] @punctuation.bracket
["{" "}"] @punctuation.bracket

["," ";" ":" "."] @punctuation.delimiter

; Built-in functions
((identifier) @function.builtin
  (#any-of? @function.builtin
    "print" "println" "debug" "assert"
    "len" "push" "pop" "append"
    "panic" "todo" "unreachable"))

; Wildcards in patterns
(wildcard_pattern) @variable.builtin
