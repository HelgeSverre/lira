/**
 * @file Tree-sitter grammar for the Lira programming language
 * @author Helge
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  ASSIGN: 1,
  RANGE: 2,
  OR: 3,
  AND: 4,
  BIT_OR: 5,
  BIT_XOR: 6,
  BIT_AND: 7,
  EQUALITY: 8,
  COMPARISON: 9,
  SHIFT: 10,
  ADD: 11,
  MULTIPLY: 12,
  POWER: 13,
  UNARY: 14,
  POSTFIX: 15,
  CALL: 16,
  MEMBER: 17,
};

module.exports = grammar({
  name: 'lira',

  externals: $ => [],

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  supertypes: $ => [
    $._literal,
  ],

  inline: $ => [
    $._expression_statement,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$.generic_type, $.comparison_expression],
    [$.range_expression],
    [$.block, $.map_expression],
    [$.return_statement, $.return_expression],
    [$.break_statement, $.break_expression],
    [$.continue_statement, $.continue_expression],
  ],

  rules: {
    source_file: $ => repeat($._top_level_item),

    _top_level_item: $ => choice(
      $._declaration,
      $._statement,
    ),

    // =========================================================================
    // DECLARATIONS
    // =========================================================================

    _declaration: $ => choice(
      $.function_declaration,
      $.struct_declaration,
      $.class_declaration,
      $.enum_declaration,
      $.trait_declaration,
      $.interface_declaration,
      $.impl_block,
      $.type_alias,
      $.import_declaration,
      $.use_declaration,
    ),

    // Function declaration
    function_declaration: $ => seq(
      optional($.visibility),
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
      optional($.where_clause),
      choice(
        field('body', $.block),
        seq('=>', field('body', $._expression)),
      ),
    ),

    parameters: $ => seq(
      '(',
      optional(seq(
        commaSep1($.parameter),
        optional(','),
      )),
      ')',
    ),

    parameter: $ => choice(
      // self parameter (method receiver)
      seq(optional('mut'), 'self'),
      // Regular parameter: name, optional type, optional default
      seq(
        optional('mut'),
        field('name', $.identifier),
        optional(seq(':', field('type', $._type))),
        optional(seq('=', field('default', $._expression))),
      ),
      // Type-only parameter (for function types)
      seq(':', field('type', $._type)),
    ),

    // Struct declaration
    struct_declaration: $ => seq(
      optional($.visibility),
      'struct',
      field('name', $.type_identifier),
      optional($.type_parameters),
      optional($.where_clause),
      choice(
        $.struct_body,
        seq('(', commaSep($.tuple_field), ')'),
      ),
    ),

    struct_body: $ => seq(
      '{',
      repeat(choice($.struct_field, $.method_declaration)),
      '}',
    ),

    struct_field: $ => seq(
      optional($.visibility),
      field('name', $.identifier),
      ':',
      field('type', $._type),
      optional(','),
    ),

    tuple_field: $ => seq(
      optional($.visibility),
      field('type', $._type),
    ),

    // Class declaration
    class_declaration: $ => seq(
      optional($.visibility),
      optional('abstract'),
      'class',
      field('name', $.type_identifier),
      optional($.type_parameters),
      optional(seq('extends', field('superclass', $._type))),
      optional(seq(':', commaSep1(field('interfaces', $._type)))),
      optional($.where_clause),
      $.class_body,
    ),

    class_body: $ => seq(
      '{',
      repeat(choice(
        $.class_field,
        $.method_declaration,
      )),
      '}',
    ),

    class_field: $ => seq(
      optional($.visibility),
      choice('let', 'var'),
      field('name', $.identifier),
      ':',
      field('type', $._type),
      optional(seq('=', field('value', $._expression))),
    ),

    method_declaration: $ => seq(
      optional($.visibility),
      optional('static'),
      optional('abstract'),
      optional('override'),
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
      optional($.where_clause),
      optional(choice(
        field('body', $.block),
        seq('=>', field('body', $._expression)),
      )),
    ),

    // Enum declaration
    enum_declaration: $ => seq(
      optional($.visibility),
      'enum',
      field('name', $.type_identifier),
      optional($.type_parameters),
      optional($.where_clause),
      $.enum_body,
    ),

    enum_body: $ => seq(
      '{',
      commaSep($.enum_variant),
      optional(','),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(choice(
        seq('(', commaSep($._type), ')'),
        $.struct_body,
      )),
    ),

    // Trait declaration
    trait_declaration: $ => seq(
      optional($.visibility),
      'trait',
      field('name', $.type_identifier),
      optional($.type_parameters),
      optional(seq(':', commaSep1($.trait_bound))),
      optional($.where_clause),
      $.trait_body,
    ),

    trait_body: $ => seq(
      '{',
      repeat($.trait_method),
      '}',
    ),

    trait_method: $ => seq(
      optional($.visibility),
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional($.type_parameters),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $._type))),
      optional($.where_clause),
      optional(choice(
        field('body', $.block),
        seq('=>', field('body', $._expression)),
      )),
    ),

    // Interface declaration
    interface_declaration: $ => seq(
      optional($.visibility),
      'interface',
      field('name', $.type_identifier),
      optional($.type_parameters),
      optional(seq(':', commaSep1($.trait_bound))),
      optional($.where_clause),
      $.trait_body,
    ),

    // Impl block
    impl_block: $ => seq(
      'impl',
      optional($.type_parameters),
      optional(seq(
        field('trait', $._type),
        'for',
      )),
      field('type', $._type),
      optional($.where_clause),
      $.impl_body,
    ),

    impl_body: $ => seq(
      '{',
      repeat($.method_declaration),
      '}',
    ),

    // Type alias
    type_alias: $ => seq(
      optional($.visibility),
      'type',
      field('name', $.type_identifier),
      optional($.type_parameters),
      '=',
      field('type', $._type),
    ),

    // Import declaration
    import_declaration: $ => seq(
      'import',
      $.import_path,
    ),

    // Import path - module followed by optional suffix
    // We inline the module_path pattern here to avoid conflicts
    import_path: $ => seq(
      optional(repeat1(choice('.', '..'))),
      $.identifier,
      repeat(seq('.', $.identifier)),
      optional(choice(
        seq('as', $.identifier),
        seq('.', $.import_group),
        seq('.', '*'),
      )),
    ),

    module_path: $ => seq(
      optional(repeat1(choice('.', '..'))),
      $.identifier,
      repeat(seq('.', $.identifier)),
    ),

    import_group: $ => seq(
      '{',
      commaSep1(choice(
        seq($.identifier, 'as', $.identifier),
        $.identifier,
      )),
      optional(','),
      '}',
    ),

    // Use declaration (alternative syntax using :: separator)
    use_declaration: $ => seq(
      'use',
      $.use_path,
    ),

    // Use path - inline the pattern to avoid conflicts
    use_path: $ => seq(
      $.identifier,
      repeat(seq('::', $.identifier)),
      optional(choice(
        seq('as', $.identifier),
        seq('::', $.import_group),
        seq('::', '*'),
      )),
    ),

    namespace_path: $ => seq(
      $.identifier,
      repeat(seq('::', $.identifier)),
    ),

    // =========================================================================
    // STATEMENTS
    // =========================================================================

    _statement: $ => choice(
      $._expression_statement,
      $.variable_declaration,
      $.return_statement,
      $.break_statement,
      $.continue_statement,
      $.while_statement,
      $.for_statement,
      $.loop_statement,
      $.try_statement,
      $.spawn_statement,
      $.select_statement,
      $.block,
    ),

    _expression_statement: $ => seq(
      $._expression,
      optional(';'),
    ),

    variable_declaration: $ => seq(
      choice('let', 'var', 'const'),
      field('pattern', $._pattern),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
    ),

    // Statement versions have higher precedence than expression versions
    // Use alias to mark these as statements in output
    return_statement: $ => prec.right(seq(
      'return',
      optional($._expression),
    )),

    break_statement: $ => prec.right(seq(
      'break',
      optional($._expression),
    )),

    continue_statement: $ => 'continue',

    // While statement
    while_statement: $ => seq(
      'while',
      choice(
        field('condition', $._expression),
        seq('let', field('pattern', $._pattern), '=', field('value', $._expression)),
      ),
      field('body', $.block),
    ),

    // For statement
    for_statement: $ => seq(
      'for',
      field('pattern', $._pattern),
      'in',
      field('iterable', $._expression),
      field('body', $.block),
    ),

    // Loop statement
    loop_statement: $ => seq(
      'loop',
      field('body', $.block),
    ),

    match_body: $ => seq(
      '{',
      repeat($.match_arm),
      '}',
    ),

    match_arm: $ => seq(
      field('pattern', $.match_pattern),
      optional(seq('if', field('guard', $._expression))),
      '=>',
      field('body', choice($._expression, $.block)),
      optional(','),
    ),

    match_pattern: $ => choice(
      $._pattern,
      seq($._pattern, repeat1(seq('|', $._pattern))),
    ),

    // Try statement
    try_statement: $ => seq(
      'try',
      field('body', $.block),
      repeat($.catch_clause),
      optional($.finally_clause),
    ),

    catch_clause: $ => seq(
      'catch',
      optional(seq($.type_identifier, optional(seq('as', $.identifier)))),
      field('body', $.block),
    ),

    finally_clause: $ => seq(
      'finally',
      field('body', $.block),
    ),

    // Spawn statement
    spawn_statement: $ => seq(
      'spawn',
      optional($.string_literal),
      field('body', $.block),
    ),

    // Select statement
    select_statement: $ => seq(
      'select',
      $.select_body,
    ),

    select_body: $ => seq(
      '{',
      repeat($.select_arm),
      '}',
    ),

    select_arm: $ => seq(
      choice(
        seq('recv', ':', field('binding', $.identifier), '=', $.receive_expression),
        seq('send', ':', field('channel', $._expression), '<-', field('value', $._expression)),
        'default',
      ),
      '=>',
      field('body', choice($._expression, $.block)),
      optional(','),
    ),

    // Block
    block: $ => seq(
      '{',
      repeat($._statement),
      '}',
    ),

    // =========================================================================
    // EXPRESSIONS
    // =========================================================================

    _expression: $ => choice(
      $._literal,
      $.identifier,
      $.self_expression,
      $.super_expression,
      $.this_expression,
      $.group_expression,
      $.binary_expression,
      $.unary_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.call_expression,
      $.method_call_expression,
      $.member_expression,
      $.index_expression,
      $.array_expression,
      $.tuple_expression,
      $.map_expression,
      $.struct_expression,
      $.lambda_expression,
      $.if_expression,
      $.match_expression,
      $.range_expression,
      $.cast_expression,
      $.type_check_expression,
      $.try_expression,
      $.receive_expression,
      $.send_expression,
      $.optional_chain_expression,
      $.null_coalesce_expression,
      $.increment_expression,
      $.decrement_expression,
      $.new_expression,
      $.path_expression,
      $.generic_function,
      // Control flow expressions (for use in match arms, etc.)
      $.return_expression,
      $.break_expression,
      $.continue_expression,
    ),

    group_expression: $ => seq('(', $._expression, ')'),

    // Binary expressions
    binary_expression: $ => choice(
      prec.left(PREC.OR, seq($._expression, '||', $._expression)),
      prec.left(PREC.AND, seq($._expression, '&&', $._expression)),
      prec.left(PREC.BIT_OR, seq($._expression, '|', $._expression)),
      prec.left(PREC.BIT_XOR, seq($._expression, '^', $._expression)),
      prec.left(PREC.BIT_AND, seq($._expression, '&', $._expression)),
      prec.left(PREC.EQUALITY, seq($._expression, choice('==', '!='), $._expression)),
      prec.left(PREC.COMPARISON, seq($._expression, choice('<', '>', '<=', '>='), $._expression)),
      prec.left(PREC.SHIFT, seq($._expression, choice('<<', '>>', '>>>'), $._expression)),
      prec.left(PREC.ADD, seq($._expression, choice('+', '-'), $._expression)),
      prec.left(PREC.MULTIPLY, seq($._expression, choice('*', '/', '%'), $._expression)),
      prec.right(PREC.POWER, seq($._expression, '**', $._expression)),
    ),

    comparison_expression: $ => prec.left(PREC.COMPARISON, seq(
      $._expression,
      choice('<', '>', '<=', '>='),
      $._expression,
    )),

    // Unary expressions
    unary_expression: $ => prec.right(PREC.UNARY, seq(
      choice('-', '!', '~'),
      $._expression,
    )),

    // Assignment
    assignment_expression: $ => prec.right(PREC.ASSIGN, seq(
      field('left', choice($.identifier, $.member_expression, $.index_expression)),
      '=',
      field('right', $._expression),
    )),

    compound_assignment_expression: $ => prec.right(PREC.ASSIGN, seq(
      field('left', choice($.identifier, $.member_expression, $.index_expression)),
      choice('+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>='),
      field('right', $._expression),
    )),

    // Increment/Decrement
    increment_expression: $ => choice(
      prec.left(PREC.POSTFIX, seq($._expression, '++')),
      prec.right(PREC.UNARY, seq('++', $._expression)),
    ),

    decrement_expression: $ => choice(
      prec.left(PREC.POSTFIX, seq($._expression, '--')),
      prec.right(PREC.UNARY, seq('--', $._expression)),
    ),

    // Call expression
    call_expression: $ => prec(PREC.CALL, seq(
      field('function', $._expression),
      field('arguments', $.arguments),
    )),

    arguments: $ => seq(
      '(',
      optional(seq(
        commaSep1($.argument),
        optional(','),
      )),
      ')',
    ),

    argument: $ => choice(
      $._expression,
      seq(field('name', $.identifier), ':', field('value', $._expression)),
      seq('...', $._expression),
    ),

    // Method call
    method_call_expression: $ => prec(PREC.MEMBER, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      optional($.type_arguments),
      field('arguments', $.arguments),
    )),

    // Member access
    member_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '.',
      field('member', $.identifier),
    )),

    // Optional chaining
    optional_chain_expression: $ => prec.left(PREC.MEMBER, seq(
      field('object', $._expression),
      '?.',
      field('member', $.identifier),
    )),

    // Index expression
    index_expression: $ => prec(PREC.CALL, seq(
      field('object', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    // Array literal
    array_expression: $ => seq(
      '[',
      optional(seq(
        commaSep1($._expression),
        optional(','),
      )),
      ']',
    ),

    // Tuple literal
    tuple_expression: $ => seq(
      '(',
      seq(
        $._expression,
        ',',
        optional(seq(
          commaSep1($._expression),
          optional(','),
        )),
      ),
      ')',
    ),

    // Map literal
    map_expression: $ => seq(
      '{',
      optional(seq(
        commaSep1($.map_entry),
        optional(','),
      )),
      '}',
    ),

    map_entry: $ => seq(
      field('key', choice($.string_literal, $.integer_literal, $.identifier)),
      ':',
      field('value', $._expression),
    ),

    // Struct literal
    struct_expression: $ => prec(PREC.CALL, seq(
      field('type', choice($.type_identifier, $.path_expression)),
      '{',
      optional(seq(
        commaSep1($.struct_field_initializer),
        optional(','),
      )),
      '}',
    )),

    struct_field_initializer: $ => choice(
      seq(field('name', $.identifier), ':', field('value', $._expression)),
      seq('..', $._expression),
      $.identifier,
    ),

    // Lambda expression
    lambda_expression: $ => seq(
      '|',
      optional(commaSep1($.lambda_parameter)),
      '|',
      optional(seq('->', $._type)),
      choice(
        field('body', $.block),
        field('body', $._expression),
      ),
    ),

    lambda_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
    ),

    // If expression
    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $._expression),
      field('consequence', $.block),
      optional(seq('else', choice($.if_expression, $.block))),
    )),

    // Match expression
    match_expression: $ => seq(
      'match',
      field('value', $._expression),
      $.match_body,
    ),

    // Range expression
    // Bounded ranges have higher precedence than unbounded
    range_expression: $ => choice(
      prec.left(PREC.RANGE + 1, seq(field('start', $._expression), '..', field('end', $._expression))),
      prec.left(PREC.RANGE + 1, seq(field('start', $._expression), '..=', field('end', $._expression))),
      prec.left(PREC.RANGE, seq(field('start', $._expression), '..')),
      prec.right(PREC.RANGE, seq('..', field('end', $._expression))),
      prec.right(PREC.RANGE, seq('..=', field('end', $._expression))),
      prec(PREC.RANGE, '..'),
    ),

    // Cast expression
    cast_expression: $ => prec.left(PREC.COMPARISON, seq(
      field('value', $._expression),
      'as',
      field('type', $._type),
    )),

    // Type check expression
    type_check_expression: $ => prec.left(PREC.COMPARISON, seq(
      field('value', $._expression),
      'is',
      field('type', $._type),
    )),

    // Try expression (error propagation)
    try_expression: $ => prec(PREC.POSTFIX, seq(
      $._expression,
      '?',
    )),

    // Channel operations
    receive_expression: $ => prec.right(PREC.UNARY, seq(
      '<-',
      field('channel', $._expression),
    )),

    send_expression: $ => prec.right(PREC.ASSIGN, seq(
      field('value', $._expression),
      '->',
      field('channel', $._expression),
    )),

    // Null coalesce
    null_coalesce_expression: $ => prec.right(PREC.OR, seq(
      field('left', $._expression),
      '??',
      field('right', $._expression),
    )),

    // New expression
    new_expression: $ => prec.left(seq(
      'new',
      field('type', $._type),
      optional($.arguments),
    )),

    // Path expression (for enum variants, etc.)
    path_expression: $ => prec.left(seq(
      $.type_identifier,
      repeat1(seq(
        choice('.', '::'),
        $.identifier,
      )),
    )),

    // Generic function call
    generic_function: $ => prec(PREC.CALL, seq(
      field('function', $.identifier),
      '::',
      $.type_arguments,
    )),

    // Self/this/super
    self_expression: $ => 'self',
    super_expression: $ => 'super',
    this_expression: $ => 'this',

    // Control flow expressions (for use in match arms, closures, etc.)
    return_expression: $ => prec.right(seq('return', optional($._expression))),
    break_expression: $ => prec.right(seq('break', optional($._expression))),
    continue_expression: $ => 'continue',

    // =========================================================================
    // PATTERNS
    // =========================================================================

    _pattern: $ => choice(
      // identifier has precedence over enum_pattern for simple names
      prec(2, $.identifier),
      $.wildcard_pattern,
      $._literal,
      $.tuple_pattern,
      $.struct_pattern,
      $.enum_pattern,
      $.range_pattern,
      $.binding_pattern,
      $.rest_pattern,
    ),

    wildcard_pattern: $ => '_',

    tuple_pattern: $ => seq(
      '(',
      commaSep($._pattern),
      ')',
    ),

    struct_pattern: $ => prec(1, seq(
      optional($.type_identifier),
      '{',
      commaSep(choice(
        $.identifier,
        seq(field('name', $.identifier), ':', field('pattern', $._pattern)),
        $.rest_pattern,
      )),
      optional(','),
      '}',
    )),

    // enum_pattern matches Type, Type::variant, Type.variant, Type(args), Type::variant(args)
    // Has lower base precedence so identifier is preferred for simple names,
    // but the tuple/path variants have higher precedence to match first
    enum_pattern: $ => choice(
      // Type::variant or Type.variant (with optional tuple) - higher precedence
      prec.left(1, seq(
        $.type_identifier,
        seq(choice('.', '::'), $.identifier),
        optional(seq('(', commaSep($._pattern), ')')),
      )),
      // Type(pattern) - higher precedence, requires tuple args
      prec.left(1, seq(
        $.type_identifier,
        seq('(', commaSep($._pattern), ')'),
      )),
      // Bare type identifier - lower precedence, for cases like None
      prec(-1, $.type_identifier),
    ),

    range_pattern: $ => prec.left(1, choice(
      seq($._literal, '..', $._literal),
      seq($._literal, '..=', $._literal),
    )),

    binding_pattern: $ => seq(
      field('name', $.identifier),
      '@',
      field('pattern', $._pattern),
    ),

    rest_pattern: $ => '..',

    // =========================================================================
    // TYPES
    // =========================================================================

    _type: $ => choice(
      $.primitive_type,
      $.type_identifier,
      $.generic_type,
      $.array_type,
      $.tuple_type,
      $.function_type,
      $.optional_type,
      $.path_type,
      $.self_type,
    ),

    primitive_type: $ => choice(
      'int',
      'int8',
      'int16',
      'int32',
      'int64',
      'uint',
      'uint8',
      'uint16',
      'uint32',
      'uint64',
      'float',
      'float32',
      'float64',
      'bool',
      'string',
      'char',
      'void',
    ),

    type_identifier: $ => /[A-Z][a-zA-Z0-9_]*/,

    generic_type: $ => prec(1, seq(
      $.type_identifier,
      $.type_arguments,
    )),

    type_arguments: $ => seq(
      '<',
      commaSep1($._type),
      optional(','),
      '>',
    ),

    array_type: $ => seq('[', $._type, ']'),

    tuple_type: $ => seq(
      '(',
      seq(
        $._type,
        ',',
        optional(seq(commaSep1($._type), optional(','))),
      ),
      ')',
    ),

    function_type: $ => prec.left(seq(
      'fn',
      '(',
      optional(commaSep1($._type)),
      ')',
      optional(seq('->', $._type)),
    )),

    optional_type: $ => prec.left(seq($._type, '?')),

    path_type: $ => seq(
      choice($.identifier, $.type_identifier),
      repeat1(seq('::', choice($.identifier, $.type_identifier))),
    ),

    self_type: $ => 'Self',

    // Type parameters
    type_parameters: $ => seq(
      '<',
      commaSep1($.type_parameter),
      optional(','),
      '>',
    ),

    type_parameter: $ => seq(
      field('name', $.type_identifier),
      optional(seq(':', commaSep1($.trait_bound, '+'))),
    ),

    trait_bound: $ => choice(
      $.type_identifier,
      $.generic_type,
      $.path_type,
    ),

    where_clause: $ => seq(
      'where',
      commaSep1($.where_predicate),
    ),

    where_predicate: $ => seq(
      $.type_identifier,
      ':',
      commaSep1($.trait_bound, '+'),
    ),

    // =========================================================================
    // LITERALS
    // =========================================================================

    _literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.raw_string_literal,
      $.multiline_string_literal,
      $.char_literal,
      $.boolean_literal,
      $.null_literal,
    ),

    integer_literal: $ => token(choice(
      // Decimal with optional suffix
      seq(
        /[0-9][0-9_]*/,
        optional(/[iu](8|16|32|64)?/),
      ),
      // Hexadecimal
      seq(
        /0[xX][0-9a-fA-F][0-9a-fA-F_]*/,
        optional(/[iu](8|16|32|64)?/),
      ),
      // Octal
      seq(
        /0[oO][0-7][0-7_]*/,
        optional(/[iu](8|16|32|64)?/),
      ),
      // Binary
      seq(
        /0[bB][01][01_]*/,
        optional(/[iu](8|16|32|64)?/),
      ),
    )),

    float_literal: $ => token(seq(
      choice(
        seq(/[0-9][0-9_]*/, '.', /[0-9][0-9_]*/),
        seq(/[0-9][0-9_]*/, /[eE][+-]?[0-9]+/),
        seq(/[0-9][0-9_]*/, '.', /[0-9][0-9_]*/, /[eE][+-]?[0-9]+/),
      ),
      optional(/f(32|64)?/),
    )),

    string_literal: $ => seq(
      '"',
      repeat(choice(
        $.string_content,
        $.escape_sequence,
        $.string_interpolation,
      )),
      '"',
    ),

    string_content: $ => token.immediate(prec(1, /[^"\\$]+/)),

    escape_sequence: $ => token.immediate(seq(
      '\\',
      choice(
        /[nrtv\\'"0]/,
        /x[0-9a-fA-F]{2}/,
        /u\{[0-9a-fA-F]+\}/,
      ),
    )),

    string_interpolation: $ => seq(
      '${',
      $._expression,
      '}',
    ),

    raw_string_literal: $ => seq(
      'r"',
      /[^"]*/,
      '"',
    ),

    multiline_string_literal: $ => seq(
      '"""',
      /[^"]*/,
      '"""',
    ),

    char_literal: $ => seq(
      "'",
      choice(
        /[^'\\]/,
        $.escape_sequence,
      ),
      "'",
    ),

    boolean_literal: $ => choice('true', 'false'),

    null_literal: $ => 'null',

    // =========================================================================
    // IDENTIFIERS AND COMMENTS
    // =========================================================================

    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,

    visibility: $ => choice('pub', 'priv', 'internal'),

    line_comment: $ => token(choice(
      seq('//', /.*/),
      seq('///', /.*/),
      seq('//!', /.*/),
    )),

    block_comment: $ => token(seq(
      choice('/*', '/**'),
      /[^*]*\*+([^/*][^*]*\*+)*/,
      '/',
    )),
  },
});

/**
 * Creates a comma-separated list rule (1 or more)
 */
function commaSep1(rule, separator = ',') {
  return seq(rule, repeat(seq(separator, rule)));
}

/**
 * Creates a comma-separated list rule (0 or more)
 */
function commaSep(rule, separator = ',') {
  return optional(commaSep1(rule, separator));
}
