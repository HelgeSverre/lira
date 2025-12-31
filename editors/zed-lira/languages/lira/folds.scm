; Lira code folding queries for tree-sitter

; ============================================================================
; DECLARATIONS
; ============================================================================

; Function bodies
(function_declaration
  body: (block) @fold)

(method_declaration
  body: (block) @fold)

(trait_method
  body: (block) @fold)

; Struct/class/enum bodies
(struct_body) @fold

(class_body) @fold

(enum_body) @fold

(trait_body) @fold

(impl_body) @fold

; ============================================================================
; CONTROL FLOW
; ============================================================================

; Blocks
(block) @fold

; If expressions
(if_expression
  consequence: (block) @fold)

; Loops
(while_statement
  body: (block) @fold)

(for_statement
  body: (block) @fold)

(loop_statement
  body: (block) @fold)

; Match
(match_body) @fold

; Try-catch
(try_statement
  body: (block) @fold)

(catch_clause
  body: (block) @fold)

(finally_clause
  body: (block) @fold)

; ============================================================================
; CONCURRENCY
; ============================================================================

(spawn_statement
  body: (block) @fold)

(select_body) @fold

; ============================================================================
; EXPRESSIONS
; ============================================================================

; Lambda bodies
(lambda_expression
  body: (block) @fold)

; Array literals (large ones)
(array_expression) @fold

; Map literals
(map_expression) @fold

; ============================================================================
; IMPORTS
; ============================================================================

(import_group) @fold

; ============================================================================
; COMMENTS
; ============================================================================

(block_comment) @fold
