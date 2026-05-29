Feature: Parsing

  The tree-sitter grammar turns neoc source into a syntax tree. Valid source
  parses cleanly — no error nodes — to the shape `lang/SYNTAX.md` describes, with
  `source_file` at the root. Malformed source recovers into ERROR and MISSING
  nodes rather than aborting the parse, so an editor can still reason about a
  half-written file.

  Scenario: A function with a let binding parses cleanly
    Given the source:
      """
      fn main() { let x = 1; }
      """
    When I parse with the grammar
    Then there are no error nodes
    And the root node kind is "source_file"
    And the syntax tree is:
      """
      (source_file (item (function_declaration name: (identifier) parameters: (parameter_list) body: (block (statement (let_statement pattern: (binding_pattern (identifier)) value: (integer_literal)))))))
      """

  Scenario: A struct declaration parses cleanly
    Given the source:
      """
      struct Point { x: number, y: number }
      """
    When I parse with the grammar
    Then there are no error nodes
    And the syntax tree is:
      """
      (source_file (item (struct_declaration name: (identifier) body: (field_declaration_list (field_declaration name: (identifier) type: (type_path (identifier))) (field_declaration name: (identifier) type: (type_path (identifier)))))))
      """

  Scenario: A struct literal in a condition needs parentheses
    Given the source:
      """
      fn f() { if (Point { x: 1 }).x > 0 { go(); } }
      """
    When I parse with the grammar
    Then there are no error nodes

  Scenario: An empty source is a clean, empty tree
    Given an empty source
    When I parse with the grammar
    Then there are no error nodes
    And the root node kind is "source_file"
    And the syntax tree is:
      """
      (source_file)
      """

  Scenario: A comment-only source carries the comment, no errors
    Given the source:
      """
      // just a comment
      """
    When I parse with the grammar
    Then there are no error nodes
    And the syntax tree is:
      """
      (source_file (line_comment))
      """

  @chaos
  Scenario: A garbage token sequence recovers into error nodes
    Given the source:
      """
      fn 123 not valid
      """
    When I parse with the grammar
    Then there are error nodes
    But the root node kind is "source_file"

  @chaos
  Scenario: A missing closing brace yields a MISSING node, not an abort
    Given the source:
      """
      fn main() { let x = 1;
      """
    When I parse with the grammar
    Then there are error nodes
    And the syntax tree is:
      """
      (source_file (item (function_declaration name: (identifier) parameters: (parameter_list) body: (block (statement (let_statement pattern: (binding_pattern (identifier)) value: (integer_literal))) (MISSING "}")))))
      """
