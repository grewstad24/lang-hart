const PREC = {
  ASSIGN: 1,
  UNION: 2,
  COMPARE: 3,
  ADD: 4,
  MULTIPLY: 5,
  UNARY: 6,
  CAST: 7,
  POSTFIX: 8,
  CALL: 9,
  PRIMARY: 10,
};

module.exports = grammar({
  name: 'hart',

  extras: $ => [
    /\s/,
    $.comment,
  ],

  conflicts: $ => [
    [$.argument_list, $.tuple_literal],
    [$.mut_declaration, $.assignment_expression],
  ],

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.decl_module,
      $.mod_declaration,
      $.export_directive,
      $.import_directive,
      $.function_definition,
      $.def_definition,
      $.extern_function_declaration,
      $.type_definition,
      $.const_declaration,
    ),

    const_declaration: $ => prec.right(PREC.ASSIGN, seq(
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      ':=',
      field('value', $._expression)
    )),

    comment: $ => token(seq('#', /.*/)),

    // Module & Imports/Exports
    decl_module: $ => seq(
      'decl',
      'mod',
      field('name', $.identifier),
      '{',
      optional(choice(
        seq($.export_directive, optional($.import_directive)),
        seq($.import_directive, optional($.export_directive)),
      )),
      '}'
    ),

    mod_declaration: $ => seq(
      'mod',
      field('name', $.identifier)
    ),

    export_directive: $ => choice(
      seq(
        'export',
        '{',
        repeat(seq($.export_group, optional(','))),
        '}'
      ),
      seq(
        'export',
        '{',
        commaSep($.identifier),
        optional(','),
        '}',
        optional(seq('to', field('target', $.namespace_path)))
      )
    ),

    export_group: $ => seq(
      '{',
      commaSep($.identifier),
      optional(','),
      '}',
      optional(seq('to', field('target', $.namespace_path)))
    ),

    import_directive: $ => choice(
      seq(
        'import',
        '{',
        commaSep($.namespace_path),
        optional(','),
        '}'
      ),
      seq(
        'import',
        '{',
        commaSep($.identifier),
        optional(','),
        '}',
        'from',
        field('source', $.namespace_path)
      )
    ),

    namespace_path: $ => token(seq(
      '.',
      optional(seq(
        /[a-zA-Z_][a-zA-Z0-9_]*/,
        repeat(seq('.', /[a-zA-Z_][a-zA-Z0-9_]*/))
      ))
    )),

    // Type Definitions
    type_definition: $ => seq(
      'type',
      field('name', $.type_identifier),
      optional(field('type_parameters', $.type_parameters)),
      ':=',
      field('type', $._type)
    ),

    type_parameters: $ => seq(
      '<',
      commaSep1($.type_identifier),
      optional(','),
      '>'
    ),

    // Functions
    function_definition: $ => seq(
      'func',
      field('name', alias($.identifier, $.function_identifier)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block)
    ),

    def_definition: $ => seq(
      'def',
      field('name', alias($.identifier, $.function_identifier)),
      field('parameters', $.def_parameter_list),
      optional(seq('->', field('return_type', $._type))),
      field('body', $.block)
    ),

    def_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $._type)))
    ),

    def_parameter_list: $ => seq(
      '(',
      commaSep($.def_parameter),
      optional(','),
      ')'
    ),

    extern_function_declaration: $ => seq(
      'extern',
      'func',
      field('name', alias($.identifier, $.function_identifier)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type)))
    ),

    parameter_list: $ => seq(
      '(',
      commaSep(choice($.parameter, $.variadic_parameter)),
      optional(','),
      ')'
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type)
    ),

    variadic_parameter: $ => '...',

    // Types
    _type: $ => choice(
      $.primitive_type,
      $.generic_type,
      $.type_identifier,
      $.pointer_type,
      $.array_type,
      $.struct_type,
      $.tuple_type,
      $.union_type,
      $.function_type,
      $.symbol_literal,
      $.parenthesized_type,
    ),

    function_type: $ => seq(
      'func',
      '(',
      optional(seq(
        commaSep(choice(
          seq(optional(seq($.identifier, ':')), $._type),
          $.variadic_parameter
        )),
        optional(',')
      )),
      ')',
      optional(seq('->', field('return_type', $._type)))
    ),

    generic_type: $ => prec(PREC.PRIMARY, seq(
      field('name', $.type_identifier),
      field('type_arguments', $.type_arguments)
    )),

    type_arguments: $ => seq(
      '<',
      commaSep1($._type),
      optional(','),
      '>'
    ),

    primitive_type: $ => choice(
      's16', 's32', 's64',
      'u8', 'u16', 'u32', 'u64',
      'usize', 'ssize',
      'opaque', 'never'
    ),

    type_identifier: $ => token(/[a-zA-Z_][a-zA-Z0-9_]*/),

    pointer_type: $ => prec(PREC.UNARY, seq('*', optional('mut'), $._type)),

    array_type: $ => prec(PREC.UNARY, seq(
      '[',
      field('size', $.integer_literal),
      ']',
      field('element', $._type)
    )),

    struct_type: $ => seq(
      '{',
      repeat1(seq($.struct_field_type, optional(','))),
      '}'
    ),

    tuple_type: $ => choice(
      seq('(', ')'),
      seq(
        '(',
        $._type,
        repeat1(seq(',', $._type)),
        optional(','),
        ')'
      )
    ),

    struct_field_type: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type)
    ),

    union_type: $ => prec.left(PREC.UNION, seq(
      field('left', $._type),
      '|',
      field('right', $._type)
    )),

    parenthesized_type: $ => seq('(', $._type, ')'),

    // Block
    block: $ => prec(PREC.PRIMARY - 1, seq(
      '{',
      repeat($._expression),
      '}'
    )),

    // Expressions
    _expression: $ => choice(
      $._non_assignment_expression,
      $.assignment_expression,
    ),

    _non_assignment_expression: $ => choice(
      $.const_declaration,
      $.mut_declaration,
      $.binary_expression,
      $.cast_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.dereference_expression,
      $.address_of_expression,
      $.index_expression,
      $.if_expression,
      $.switch_expression,
      $.match_expression,
      $.repeat_expression,
      $.for_expression,
      $.return_expression,
      $.record_literal,
      $.slice_expression,
      $.atom_expression,
      $.generalise_expression,
      $.sizeof_expression,
      $.parenthesized_expression,
      prec(1, $.struct_literal),
      $.tuple_literal,
      $.array_literal,
      $.block,
      $._literal,
      $.identifier,
    ),

    mut_declaration: $ => prec.right(PREC.ASSIGN, seq(
      'mut',
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      optional(seq(':=', field('value', $._expression)))
    )),

    assignment_expression: $ => prec.right(PREC.ASSIGN, seq(
      field('left', choice($.identifier, $.dereference_expression, $.field_expression, $.index_expression)),
      ':=',
      field('right', $._expression)
    )),

    binary_expression: $ => {
      const table = [
        [PREC.COMPARE, choice('=', '!=', '<', '<=', '>', '>=')],
        [PREC.ADD, choice('+', '-')],
        [PREC.MULTIPLY, choice('*', '/', '%')],
      ];

      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          field('operator', operator),
          field('right', $._expression)
        ))
      ));
    },

    cast_expression: $ => prec.left(PREC.CAST, seq(
      field('expression', $._expression),
      'as',
      field('type', $._type)
    )),

    field_expression: $ => prec(PREC.POSTFIX, seq(
      field('expression', $._expression),
      '.',
      field('field', choice($.identifier, $.integer_literal))
    )),

    dereference_expression: $ => prec(PREC.POSTFIX, seq(
      field('expression', $._expression),
      '.',
      '*'
    )),

    address_of_expression: $ => prec(PREC.POSTFIX, seq(
      field('expression', $._expression),
      choice(seq('.', '&mut'), seq('.', '&'))
    )),

    index_expression: $ => prec(PREC.POSTFIX, seq(
      field('expression', $._expression),
      '[',
      field('index', $._expression),
      ']'
    )),

    call_expression: $ => prec(PREC.CALL, seq(
      field('function', choice(prec(1, alias($.identifier, $.function_identifier)), $._expression)),
      field('arguments', $.argument_list)
    )),

    method_call_expression: $ => prec(PREC.CALL + 1, seq(
      field('receiver', $._expression),
      '.',
      field('method', alias($.identifier, $.function_identifier)),
      field('arguments', $.argument_list)
    )),

    argument_list: $ => seq(
      '(',
      commaSep($._expression),
      optional(','),
      ')'
    ),

    if_expression: $ => seq(
      'if',
      '{',
      repeat($.if_arm),
      '}'
    ),

    if_arm: $ => prec.right(PREC.CALL + 2, seq(
      field('condition', choice($._expression, 'else')),
      '=>',
      field('body', $._expression),
      optional(choice(',', ';'))
    )),

    switch_expression: $ => seq(
      'switch',
      field('value', $._expression),
      '{',
      repeat($.switch_arm),
      '}'
    ),

    switch_arm: $ => prec.right(PREC.CALL + 2, seq(
      field('value', choice($._expression, 'else')),
      '=>',
      field('body', $._expression),
      optional(choice(',', ';'))
    )),

    match_expression: $ => seq(
      'match',
      field('value', $._expression),
      optional(seq(
        'using',
        field('binder', $.identifier)
      )),
      '{',
      repeat($.match_arm),
      '}'
    ),

    match_arm: $ => prec.right(PREC.CALL + 2, seq(
      field('pattern', choice($._type, 'else')),
      '=>',
      field('body', $._expression),
      optional(choice(',', ';'))
    )),

    repeat_expression: $ => prec.right(seq(
      'repeat',
      field('count', $._expression),
      optional(seq('using', field('binder', $.identifier))),
      field('body', $.block)
    )),

    for_expression: $ => prec.right(seq(
      'for',
      field('iterable', $._expression),
      optional(seq('using', field('binder', $.identifier))),
      field('body', $.block)
    )),

    return_expression: $ => prec.right(seq(
      'return',
      optional($._expression)
    )),

    sizeof_expression: $ => prec(PREC.PRIMARY, seq(
      '@sizeOf',
      field('value', $._expression)
    )),

    record_literal: $ => prec(PREC.PRIMARY, seq(
      '@record',
      $.struct_literal
    )),

    slice_expression: $ => prec(PREC.PRIMARY, seq(
      '@slice',
      field('value', $._expression)
    )),

    atom_expression: $ => prec(PREC.PRIMARY, seq(
      '@atom',
      field('value', choice($.symbol_literal, $._expression))
    )),

    generalise_expression: $ => prec(PREC.PRIMARY, seq(
      '@generalise',
      field('value', $._expression)
    )),

    parenthesized_expression: $ => prec(PREC.PRIMARY, seq(
      '(',
      $._non_assignment_expression,
      ')'
    )),

    struct_literal: $ => prec(PREC.PRIMARY, seq(
      '{',
      repeat1(seq($.struct_field_init, optional(','))),
      '}'
    )),

    tuple_literal: $ => choice(
      seq('(', ')'),
      seq(
        '(',
        $._expression,
        repeat1(seq(',', $._expression)),
        optional(','),
        ')'
      )
    ),

    struct_field_init: $ => prec(PREC.ASSIGN + 1, seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression)
    )),

    array_literal: $ => seq(
      '[',
      commaSep($._expression),
      optional(','),
      ']'
    ),

    // Literals
    _literal: $ => choice(
      $.integer_literal,
      $.string_literal,
      $.symbol_literal,
    ),

    integer_literal: $ => token(choice(
      /0x[0-9a-fA-F]+/,
      /[0-9]+/
    )),

    string_literal: $ => seq(
      '"',
      repeat(choice(
        token.immediate(prec(1, /[^"\\\n]+/)),
        $.escape_sequence
      )),
      token.immediate('"')
    ),

    escape_sequence: $ => token.immediate(prec(1, seq('\\', choice(
      /[^xuU]/,
      /\d{2,3}/,
      /x[0-9a-fA-F]{2}/,
      /u[0-9a-fA-F]{4}/,
      /u\{[0-9a-fA-F]+\}/,
      /U[0-9a-fA-F]{8}/
    )))),

    symbol_literal: $ => token(seq(':', /[a-zA-Z_][a-zA-Z0-9_]*/)),

    identifier: $ => token(/[a-zA-Z_][a-zA-Z0-9_]*/),
  }
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
