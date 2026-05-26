Feature: Rename

  Rename rewrites every occurrence of the name under the cursor — declaration
  and uses alike — to the new name. Resolution is name-based across the file,
  the same set find-all-references reports.

  Scenario: Renaming a function updates its declaration and every call
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I rename "fib" to "fibonacci"
    Then the source becomes:
      """
      fn fibonacci(n: number) -> number { fibonacci(n) }
      fn main() -> number { fibonacci(3) }
      """

  Scenario: Renaming a type updates its declaration and every use
    Given the source:
      """
      struct Point { x: number }
      fn make() -> Point { Point { x: 1 } }
      """
    When I rename "Point" to "Vec2"
    Then the source becomes:
      """
      struct Vec2 { x: number }
      fn make() -> Vec2 { Vec2 { x: 1 } }
      """
