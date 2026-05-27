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
