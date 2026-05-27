Feature: References

  Find-all-references returns every occurrence of the name under the cursor,
  resolved by name across the document. Locations are 1-based (line, column) —
  the editor's own coordinates. The declaration is part of the result unless the
  request excludes it.

  Scenario: All references to a function include its declaration and call sites
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn main() -> number { id(id(1)) }
      """
    When I find all references to "id"
    Then the references are at:
      | line | column |
      | 1    | 4      |
      | 2    | 23     |
      | 2    | 26     |

  Scenario: Excluding the declaration drops the defining name
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn main() -> number { id(id(1)) }
      """
    When I find references to "id" excluding the declaration
    Then the references are at:
      | line | column |
      | 2    | 23     |
      | 2    | 26     |

  Scenario: References to a type span its declaration and every use
    Given the source:
      """
      struct Point { x: number }
      fn make() -> Point { Point { x: 1 } }
      """
    When I find all references to "Point"
    Then the references are at:
      | line | column |
      | 1    | 8      |
      | 2    | 14     |
      | 2    | 22     |
