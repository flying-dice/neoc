/**
 * neoc tree-sitter grammar.
 *
 * Mirrors `lang/SYNTAX.md`, which is canonical. This file is one
 * implementation of that grammar; where the two disagree, the spec wins
 * and this file is the bug.
 *
 * neoc is Rust without lifetimes or ownership. The grammar is therefore
 * Rust-shaped: `fn`, `struct`, `enum`, `trait`, `impl`, `match`, `if`/`else`
 * expressions, closures `|x| e`, `use` paths, `::` qualification. No
 * ternary, no arrow functions, no `import`/`from`, no `null`, no anonymous
 * `A | B` unions.
 *
 * Regenerate `src/parser.c` with `tree-sitter generate` after editing.
 */

const PREC = {
  assign: 1,
  or: 2,
  and: 3,
  compare: 4,
  range: 5,
  add: 6,
  mul: 7,
  unary: 8,
  try: 9,
  call: 10,
  field: 11,
  method: 12,
  path: 13,
};

module.exports = grammar({
  name: 'neoc',

  word: $ => $.identifier,

  extras: $ => [/\s/, $.line_comment, $.block_comment, $.doc_comment],

  // A non-final `match` arm and the final arm share their `pattern => value`
  // prefix; after a block-ending value the parser cannot tell which it is until
  // it sees whether another arm or the closing `}` follows. GLR explores both
  // and the following token settles it.
  conflicts: $ => [
    [$._expr_no_assign, $.match_arm],
  ],

  rules: {
    source_file: $ => repeat($.item),

    // ---- items ----------------------------------------------------------

    item: $ => seq(
      repeat($.attribute),
      optional('pub'),
      choice(
        $.use_declaration,
        $.const_declaration,
        $.type_alias,
        $.struct_declaration,
        $.enum_declaration,
        $.trait_declaration,
        $.impl_block,
        $.function_declaration,
        $.extern_block,
      ),
    ),

    use_declaration: $ => seq(
      'use',
      field('path', $.use_path),
      optional(seq('as', field('alias', $.identifier))),
      ';',
    ),

    use_path: $ => sep1($.identifier, '::'),

    const_declaration: $ => seq(
      'const',
      field('name', $.identifier),
      ':',
      field('type', $._type),
      '=',
      field('value', $._expression),
      ';',
    ),

    type_alias: $ => seq(
      'type',
      field('name', $.identifier),
      optional(field('generics', $.generic_params)),
      '=',
      field('value', $._type),
      ';',
    ),

    struct_declaration: $ => seq(
      'struct',
      field('name', $.identifier),
      optional(field('generics', $.generic_params)),
      choice(
        ';',                                              // unit
        seq('(', field('element', $._type), ')', ';'),    // newtype
        field('body', $.field_declaration_list),          // named
      ),
    ),

    field_declaration_list: $ => seq(
      '{',
      sepTrailing($.field_declaration, ','),
      '}',
    ),

    field_declaration: $ => seq(
      repeat($.attribute),
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      optional(field('generics', $.generic_params)),
      '{',
      sepTrailing($.variant, ','),
      '}',
    ),

    variant: $ => seq(
      repeat($.attribute),
      field('name', $.identifier),
      optional(choice(
        seq('(', sepTrailing($._type, ','), ')'),         // tuple variant
        field('body', $.field_declaration_list),          // struct variant
      )),
    ),

    trait_declaration: $ => seq(
      'trait',
      field('name', $.identifier),
      optional(field('generics', $.generic_params)),
      optional(seq(':', field('supertraits', $.bounds))),
      '{',
      repeat($.trait_item),
      '}',
    ),

    trait_item: $ => choice(
      seq($._function_signature, choice(';', field('body', $.block))),
      $.associated_type,
    ),

    associated_type: $ => seq('type', field('name', $.identifier), ';'),

    impl_block: $ => seq(
      'impl',
      optional(field('generics', $.generic_params)),
      choice(
        seq(field('trait', $.type_path), 'for', field('type', $._type)),
        field('type', $._type),
      ),
      '{',
      repeat($.impl_item),
      '}',
    ),

    impl_item: $ => choice(
      seq(repeat($.attribute), $.function_declaration),
      $.associated_type_binding,
    ),

    associated_type_binding: $ => seq(
      'type',
      field('name', $.identifier),
      '=',
      field('value', $._type),
      ';',
    ),

    function_declaration: $ => seq(
      $._function_signature,
      field('body', $.block),
    ),

    _function_signature: $ => seq(
      'fn',
      field('name', $.identifier),
      optional(field('generics', $.generic_params)),
      field('parameters', $.parameter_list),
      optional(seq('->', field('return_type', $._type))),
      optional(field('where', $.where_clause)),
    ),

    parameter_list: $ => seq(
      '(',
      optional(seq(
        choice($.self_parameter, $.parameter),
        repeat(seq(',', $.parameter)),
        optional(','),
      )),
      ')',
    ),

    self_parameter: $ => 'self',

    parameter: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // An `extern { … }` block: the surface of the external target
    // (`lang/EXTERN.md`). A region of bodyless items — `mod` / `struct` /
    // `impl` / `fn`. No abi string, one target per compilation. The block takes
    // no `pub`; its items may.
    extern_block: $ => seq(
      'extern',
      '{',
      repeat($.extern_item),
      '}',
    ),

    extern_item: $ => seq(
      repeat($.attribute),
      optional('pub'),
      choice(
        $.extern_mod,
        $.struct_declaration,
        $.extern_impl,
        $.extern_fn,
      ),
    ),

    // A named target-module scope; nests further extern items.
    extern_mod: $ => seq(
      'mod',
      field('name', $.identifier),
      '{',
      repeat($.extern_item),
      '}',
    ),

    // A bodyless inherent method block: method signatures only, no bodies.
    extern_impl: $ => seq(
      'impl',
      optional(field('generics', $.generic_params)),
      field('type', $._type),
      '{',
      repeat($.extern_method),
      '}',
    ),

    extern_method: $ => seq(
      repeat($.attribute),
      optional('pub'),
      $._function_signature,
      ';',
    ),

    // A bodyless function signature.
    extern_fn: $ => seq($._function_signature, ';'),

    // ---- generics -------------------------------------------------------

    generic_params: $ => seq(
      '<',
      sepTrailing($.generic_param, ','),
      '>',
    ),

    generic_param: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('bounds', $.bounds))),
    ),

    bounds: $ => sep1($.type_path, '+'),

    where_clause: $ => seq(
      'where',
      sepTrailing($.where_predicate, ','),
    ),

    where_predicate: $ => seq(
      field('type', $.identifier),
      ':',
      field('bounds', $.bounds),
    ),

    // ---- types ----------------------------------------------------------

    _type: $ => choice(
      $.unit_type,
      $.list_type,
      $.function_type,
      $.self_type,
      $.type_path,
    ),

    unit_type: $ => seq('(', ')'),

    list_type: $ => seq('[', $._type, ']'),

    function_type: $ => prec.right(seq(
      'fn',
      '(',
      sepTrailing($._type, ','),
      ')',
      optional(seq('->', $._type)),
    )),

    self_type: $ => seq('Self', optional(seq('::', field('assoc', $.identifier)))),

    type_path: $ => seq(
      sep1(seq($.identifier, optional($.type_arguments)), '::'),
    ),

    type_arguments: $ => seq(
      '<',
      sepTrailing($._type, ','),
      '>',
    ),

    // ---- expressions ----------------------------------------------------

    _expression: $ => choice(
      $.assignment_expression,
      $._expr_no_assign,
    ),

    // The full non-assignment expression set is the union of two disjoint
    // halves split on whether the expression closes with a `}`. The halves let
    // a `match` arm tell a self-delimiting value (comma optional) from one that
    // needs a comma to mark the arm boundary, without the two overlapping — so
    // no arm is silently swallowed. Every other context uses this union and so
    // still accepts a block-ending expression (`let x = if c { a } else { b }`).
    _expr_no_assign: $ => choice(
      $._expr_not_ending_with_block,
      $._expr_ending_with_block,
    ),

    // Expressions that close with a `}` (or are otherwise self-delimiting in arm
    // position): a `match` arm carrying one needs no trailing comma.
    _expr_ending_with_block: $ => choice(
      $.if_expression,
      $.match_expression,
      $.while_expression,
      $.for_expression,
      $.loop_expression,
      $.block,
    ),

    // Every other expression. In `match` arm position one of these needs a
    // trailing comma, or `0 => g` `(1) => 2` would read as a call `g(1)` and
    // collapse two arms into one.
    _expr_not_ending_with_block: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.range_expression,
      $.try_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.closure_expression,
      $.struct_literal,
      $.list_literal,
      $.parenthesized_expression,
      $.unit_expression,
      $.return_expression,
      $.break_expression,
      $.continue_expression,
      $.path,
      $._literal,
    ),

    assignment_expression: $ => prec.right(PREC.assign, seq(
      field('left', $._expr_no_assign),
      field('operator', choice('=', '+=', '-=', '*=', '/=', '%=')),
      field('right', $._expression),
    )),

    binary_expression: $ => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.compare, choice('==', '!=', '<', '>', '<=', '>=')],
        [PREC.add, choice('+', '-')],
        [PREC.mul, choice('*', '/', '%')],
      ];
      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._expr_no_assign),
          field('operator', operator),
          field('right', $._expr_no_assign),
        )),
      ));
    },

    unary_expression: $ => prec(PREC.unary, seq(
      field('operator', choice('!', '-')),
      field('operand', $._expr_no_assign),
    )),

    range_expression: $ => prec.left(PREC.range, seq(
      field('start', $._expr_no_assign),
      field('operator', choice('..', '..=')),
      field('end', $._expr_no_assign),
    )),

    try_expression: $ => prec(PREC.try, seq(
      field('operand', $._expr_no_assign),
      '?',
    )),

    call_expression: $ => prec(PREC.call, seq(
      field('function', $._expr_no_assign),
      field('arguments', $.argument_list),
    )),

    method_call_expression: $ => prec(PREC.method, seq(
      field('receiver', $._expr_no_assign),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    field_expression: $ => prec(PREC.field, seq(
      field('value', $._expr_no_assign),
      '.',
      field('field', choice($.identifier, $.integer_literal)),
    )),

    index_expression: $ => prec(PREC.field, seq(
      field('value', $._expr_no_assign),
      '[',
      field('index', $._expression),
      ']',
    )),

    argument_list: $ => seq(
      '(',
      sepTrailing($._expression, ','),
      ')',
    ),

    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $._cond_expr),
      field('consequence', $.block),
      optional(seq('else', field('alternative', choice($.block, $.if_expression)))),
    )),

    match_expression: $ => seq(
      'match',
      field('scrutinee', $._cond_expr),
      '{',
      // Non-final arms repeat; the final arm is parsed distinctly so its comma
      // is optional regardless of its body. `last_match_arm` is aliased back to
      // `match_arm` so the CST (and the AST adapter) sees one arm node kind.
      optional(seq(
        repeat($.match_arm),
        alias($.last_match_arm, $.match_arm),
      )),
      '}',
    ),

    // A non-final arm. A block-ending value delimits itself with its `}`, so its
    // comma is optional; any other value needs the comma to separate it from the
    // next arm. The two value categories are disjoint, so there is no ambiguity
    // and no arm is silently absorbed into the previous one.
    match_arm: $ => seq(
      field('pattern', $._pattern),
      optional(seq('if', field('guard', $._expression))),
      '=>',
      choice(
        seq(field('value', $._expr_not_ending_with_block), ','),
        seq(field('value', $._expr_ending_with_block), optional(',')),
      ),
    ),

    // The final arm: any value, trailing comma optional.
    last_match_arm: $ => seq(
      field('pattern', $._pattern),
      optional(seq('if', field('guard', $._expression))),
      '=>',
      field('value', $._expression),
      optional(','),
    ),

    while_expression: $ => seq(
      'while',
      field('condition', $._cond_expr),
      field('body', $.block),
    ),

    for_expression: $ => seq(
      'for',
      field('pattern', $._pattern),
      'in',
      field('iterable', $._cond_expr),
      field('body', $.block),
    ),

    loop_expression: $ => seq(
      'loop',
      field('body', $.block),
    ),

    closure_expression: $ => prec.right(seq(
      field('parameters', $.closure_parameters),
      field('body', $._expression),
    )),

    closure_parameters: $ => seq(
      '|',
      sepTrailing($.closure_parameter, ','),
      '|',
    ),

    closure_parameter: $ => seq(
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
    ),

    block: $ => seq(
      '{',
      $._block_tail,
    ),

    // The brace body of a block: statements then an optional trailing
    // expression that is the block's value. Split out so it can share a
    // GLR conflict with struct_literal.
    _block_tail: $ => seq(
      repeat($.statement),
      optional(field('value', $._expr_no_assign)),
      '}',
    ),

    struct_literal: $ => prec(PREC.path, seq(
      field('type', $.path),
      '{',
      sepTrailing($.field_initializer, ','),
      optional(seq('..', field('base', $._expression))),
      '}',
    )),

    field_initializer: $ => choice(
      seq(field('name', $.identifier), ':', field('value', $._expression)),
      field('name', $.identifier),
    ),

    list_literal: $ => seq(
      '[',
      sepTrailing($._expression, ','),
      ']',
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    unit_expression: $ => prec(1, seq('(', ')')),

    return_expression: $ => prec.right(seq('return', optional($._expression))),
    break_expression: $ => 'break',
    continue_expression: $ => 'continue',

    // A value path: `x`, `Foo`, `module::Foo::bar`. No `<...>` type
    // arguments here — that lives in `type_path` only, so expression `<`
    // stays unambiguously comparison.
    path: $ => prec.left(PREC.path, sep1($.identifier, '::')),

    // Condition expressions: the head of `if` / `while` / `for ... in` /
    // `match`. A genuinely separate hierarchy (not a filtered view of the
    // main expressions) so the trailing `{` is unambiguously the block /
    // arm list, never a struct literal. tree-sitter merges LR states, so
    // reusing the main `path` would re-admit the struct-literal `{` shift;
    // hence the dedicated `cond_path` and operator rules below. To use a
    // struct literal in these positions, parenthesize it. Rust's rule.
    _cond_expr: $ => choice(
      $.cond_binary_expression,
      $.cond_unary_expression,
      $.cond_range_expression,
      $.cond_try_expression,
      $.cond_call_expression,
      $.cond_method_call_expression,
      $.cond_field_expression,
      $.cond_index_expression,
      $.parenthesized_expression,
      $.unit_expression,
      $.list_literal,
      $.cond_path,
      $._literal,
    ),

    cond_path: $ => prec.left(PREC.path, sep1($.identifier, '::')),

    cond_binary_expression: $ => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.compare, choice('==', '!=', '<', '>', '<=', '>=')],
        [PREC.add, choice('+', '-')],
        [PREC.mul, choice('*', '/', '%')],
      ];
      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._cond_expr),
          field('operator', operator),
          field('right', $._cond_expr),
        )),
      ));
    },

    cond_unary_expression: $ => prec(PREC.unary, seq(
      field('operator', choice('!', '-')),
      field('operand', $._cond_expr),
    )),

    cond_range_expression: $ => prec.left(PREC.range, seq(
      field('start', $._cond_expr),
      field('operator', choice('..', '..=')),
      field('end', $._cond_expr),
    )),

    cond_try_expression: $ => prec(PREC.try, seq(
      field('operand', $._cond_expr),
      '?',
    )),

    cond_call_expression: $ => prec(PREC.call, seq(
      field('function', $._cond_expr),
      field('arguments', $.argument_list),
    )),

    cond_method_call_expression: $ => prec(PREC.method, seq(
      field('receiver', $._cond_expr),
      '.',
      field('method', $.identifier),
      field('arguments', $.argument_list),
    )),

    cond_field_expression: $ => prec(PREC.field, seq(
      field('value', $._cond_expr),
      '.',
      field('field', choice($.identifier, $.integer_literal)),
    )),

    cond_index_expression: $ => prec(PREC.field, seq(
      field('value', $._cond_expr),
      '[',
      field('index', $._expression),
      ']',
    )),

    // ---- patterns -------------------------------------------------------

    _pattern: $ => choice(
      $.or_pattern,
      $._pattern_no_or,
    ),

    or_pattern: $ => prec.left(seq(
      $._pattern_no_or,
      repeat1(seq('|', $._pattern_no_or)),
    )),

    _pattern_no_or: $ => choice(
      $.wildcard_pattern,
      $.literal_pattern,
      $.struct_pattern,
      $.tuple_pattern,
      $.path_pattern,
      $.binding_pattern,
    ),

    wildcard_pattern: $ => '_',

    literal_pattern: $ => $._literal,

    // A lone identifier. May be a fresh binding or a unit struct / unit
    // variant — the grammar can't tell, so it yields a binding_pattern and
    // the adapter reinterprets it as a constant pattern when the name
    // resolves to a unit struct/variant. This is how Rust handles `None`.
    binding_pattern: $ => $.identifier,

    // Multi-segment only: `Color::Red`. A single segment is a
    // binding_pattern (above); two or more name a unit variant.
    path_pattern: $ => prec(1, seq($.identifier, repeat1(seq('::', $.identifier)))),

    pattern_path: $ => sep1($.identifier, '::'),

    struct_pattern: $ => seq(
      field('type', $.pattern_path),
      '{',
      sepTrailing($.field_pattern, ','),
      optional('..'),
      '}',
    ),

    field_pattern: $ => choice(
      seq(field('name', $.identifier), ':', field('pattern', $._pattern)),
      field('name', $.identifier),
    ),

    tuple_pattern: $ => seq(
      field('type', $.pattern_path),
      '(',
      sepTrailing($._pattern, ','),
      ')',
    ),

    // ---- statements -----------------------------------------------------

    statement: $ => choice(
      $.let_statement,
      $.item,
      $.block_statement,
      $.expression_statement,
    ),

    // Block-like expressions (`if`, `match`, loops, bare block) used for
    // effect as statements need no trailing `;` — Rust's rule. A trailing
    // `;` is still accepted.
    block_statement: $ => prec(1, seq(
      choice(
        $.if_expression,
        $.match_expression,
        $.while_expression,
        $.for_expression,
        $.loop_expression,
        $.block,
      ),
      optional(';'),
    )),

    let_statement: $ => seq(
      'let',
      field('pattern', $._pattern),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      ';',
    ),

    expression_statement: $ => seq($._expression, ';'),

    // ---- attributes -----------------------------------------------------

    attribute: $ => seq(
      '#',
      '[',
      sepTrailing($.attribute_item, ','),
      ']',
    ),

    attribute_item: $ => seq(
      field('name', $.identifier),
      optional(seq(
        '(',
        sepTrailing($._attribute_arg, ','),
        ')',
      )),
    ),

    _attribute_arg: $ => choice(
      $.attribute_named_arg,
      $.attribute_item,
      $._literal,
    ),

    // A Rust-faithful key-value attribute argument: `key = value`, e.g.
    // `#[link(name = "color")]`.
    attribute_named_arg: $ => seq(
      field('key', $.identifier),
      '=',
      field('value', $._literal),
    ),

    // ---- literals & lexical --------------------------------------------

    _literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.boolean_literal,
    ),

    integer_literal: $ => token(choice(
      /0x[0-9a-fA-F_]+/,
      /0o[0-7_]+/,
      /0b[01_]+/,
      /[0-9][0-9_]*/,
    )),

    float_literal: $ => token(choice(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9_]+)?/,
      /[0-9][0-9_]*[eE][+-]?[0-9_]+/,
    )),

    string_literal: $ => choice(
      seq('"', repeat(choice($.string_content, $.escape_sequence)), '"'),
      seq(/r#*"/, repeat($.raw_string_content), /"#*/),
    ),

    string_content: $ => token.immediate(prec(1, /[^"\\]+/)),

    raw_string_content: $ => token.immediate(prec(1, /[^"]+/)),

    escape_sequence: $ => token.immediate(seq('\\', choice(
      /[nrt0\\"]/,
      /x[0-9a-fA-F]{2}/,
      /u\{[0-9a-fA-F]+\}/,
    ))),

    boolean_literal: $ => choice('true', 'false'),

    // One identifier token for every name — values, fields, types, traits,
    // variants. Casing is convention (snake_case values, UpperCamelCase
    // types, SCREAMING_SNAKE_CASE consts), not lexis. Distinguishing a
    // binding pattern from a unit-variant pattern is deferred to name
    // resolution in the adapter, exactly as Rust does.
    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    line_comment: $ => token(seq('//', /[^\n]*/)),

    block_comment: $ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),

    doc_comment: $ => token(choice(
      seq('///', /[^\n]*/),
      seq('/**', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
    )),
  },
});

/** One or more `rule`, separated by `separator`. No trailing separator. */
function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}

/** Zero or more `rule` separated by `separator`, optional trailing separator. */
function sepTrailing(rule, separator) {
  return optional(seq(rule, repeat(seq(separator, rule)), optional(separator)));
}
