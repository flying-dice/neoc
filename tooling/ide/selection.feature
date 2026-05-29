Feature: Selection range

  Expand-selection grows from the smallest syntax node under the cursor outward
  through its enclosing nodes, up to the whole file. Each step is a superset of
  the last.

  Scenario: Expanding from an identifier reaches its call expression
    Given the source:
      """
      fn main() -> number { add(1, 2) }
      """
    When I expand the selection at "add"
    Then the innermost selection is "add"
    And the selection eventually covers "add(1, 2)"

  @chaos
  Scenario: A cursor inside a string literal grows to the whole string, not its words
    Given the source:
      """
      fn greet() -> string { "hello world" }
      """
    When I expand the selection at "hello"
    Then the innermost selection is "hello world"

  @chaos
  Scenario: neoc keywords spelled inside a string are plain text, not syntax
    Given the source:
      """
      fn f() -> string { "return fn struct" }
      """
    When I expand the selection at "fn struct"
    Then the innermost selection is "return fn struct"

  @chaos
  Scenario: A trailing syntax error does not break the chain around a valid call
    Given the source:
      """
      fn main() -> number { sum(1, 2) } %%%
      """
    When I expand the selection at "sum"
    Then the innermost selection is "sum"
    And the selection eventually covers "sum(1, 2)"

  @chaos
  Scenario: A deeply nested call argument expands through each enclosing call
    Given the source:
      """
      fn main() -> number { outer(inner(2, 3), 4) }
      """
    When I expand the selection at "inner"
    Then the innermost selection is "inner"
    And the selection eventually covers "inner(2, 3)"
    And the selection eventually covers "outer(inner(2, 3), 4)"

  @chaos
  Scenario Outline: A lone identifier expands straight to its whole declaration
    Given the source:
      """
      const <name>: number = 1;
      """
    When I expand the selection at "<name>"
    Then the innermost selection is "<name>"
    And the selection eventually covers "const <name>: number = 1;"

    Examples:
      | name  |
      | X     |
      | LIMIT |
      | total |

  @chaos @pending
  Scenario: A document of pure garbage still yields a selection chain
    Given the source:
      """
      %%% ^^^ &&&
      """
    When I expand the selection at "^^^"
    Then the innermost selection is "^^^"
    And the selection eventually covers "%%% ^^^ &&&"
