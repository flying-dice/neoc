Feature: Goto navigation

  Goto resolves the name under the cursor to a declaration: definition jumps to
  what introduces the name, type-definition jumps to the type of a value, and
  implementation lists the impls of a type or trait. All name-based and
  file-local.

  Scenario: Definition jumps from a call to the function declaration
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I go to the definition of "fib"
    Then it resolves to line 1

  Scenario: Type definition jumps from a value to the declaration of its type
    Given the source:
      """
      struct Point { x: number }
      const ORIGIN: Point = Point { x: 0 }
      fn m() -> number { ORIGIN.x }
      """
    When I go to the type of "ORIGIN"
    Then it resolves to line 1

  Scenario: Implementation lists a type's inherent impl
    Given the source:
      """
      struct Counter { n: number }
      impl Counter { fn inc(self) -> number { self.n } }
      """
    When I go to the implementations of "Counter"
    Then it resolves to line 2

  Scenario: Implementation lists a trait's impl
    Given the source:
      """
      trait Show { fn show(self) -> string; }
      struct P { x: number }
      impl Show for P { fn show(self) -> string { "p" } }
      """
    When I go to the implementations of "Show"
    Then it resolves to line 3
