; Lira scope tracking queries for tree-sitter

; ============================================================================
; SCOPES
; ============================================================================

; File scope
(source_file) @local.scope

; Function scopes
(function_declaration) @local.scope
(method_declaration) @local.scope
(trait_method) @local.scope
(lambda_expression) @local.scope

; Block scopes
(block) @local.scope

; Control flow scopes
(if_expression) @local.scope
(while_statement) @local.scope
(for_statement) @local.scope
(loop_statement) @local.scope
(match_arm) @local.scope

; Try-catch scopes
(try_statement) @local.scope
(catch_clause) @local.scope

; Concurrency scopes
(spawn_statement) @local.scope
(select_arm) @local.scope

; ============================================================================
; DEFINITIONS
; ============================================================================

; Function definitions
(function_declaration
  name: (identifier) @local.definition.function)

(method_declaration
  name: (identifier) @local.definition.method)

(trait_method
  name: (identifier) @local.definition.method)

; Type definitions
(struct_declaration
  name: (type_identifier) @local.definition.type)

(class_declaration
  name: (type_identifier) @local.definition.type)

(enum_declaration
  name: (type_identifier) @local.definition.type)

(trait_declaration
  name: (type_identifier) @local.definition.type)

(interface_declaration
  name: (type_identifier) @local.definition.type)

(type_alias
  name: (type_identifier) @local.definition.type)

; Type parameters
(type_parameter
  name: (type_identifier) @local.definition.type)

; Variable definitions
(variable_declaration
  pattern: (identifier) @local.definition.var)

; Parameters
(parameter
  name: (identifier) @local.definition.parameter)

(lambda_parameter
  name: (identifier) @local.definition.parameter)

; For loop bindings
(for_statement
  pattern: (identifier) @local.definition.var)

; Match arm bindings
(binding_pattern
  name: (identifier) @local.definition.var)

; Catch bindings
(catch_clause
  (identifier) @local.definition.var)

; Field definitions
(struct_field
  name: (identifier) @local.definition.field)

(class_field
  name: (identifier) @local.definition.field)

; Enum variants
(enum_variant
  name: (identifier) @local.definition.constant)

; Import definitions
(import_path
  (identifier) @local.definition.import)

; ============================================================================
; REFERENCES
; ============================================================================

; Variable references
(identifier) @local.reference

; Type references
(type_identifier) @local.reference

; ============================================================================
; IMPORTS
; ============================================================================

; Imported names are definitions in current scope
(import_declaration
  (import_path
    (identifier) @local.definition.import))

(use_declaration
  (use_path
    (identifier) @local.definition.import))
