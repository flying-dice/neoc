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

  @chaos
  Scenario: Definition of a called name that was never declared resolves to nothing
    Given the source:
      """
      fn main() -> number { ghost() }
      """
    When I go to the definition of "ghost"
    Then it resolves to nothing

  @chaos
  Scenario: Definition of an undeclared name used twice still resolves to nothing
    Given the source:
      """
      fn main() -> number { mystery() + mystery() }
      """
    When I go to the definition of "mystery"
    Then it resolves to nothing

  @chaos
  Scenario: Definition of a bare value with no binding resolves to nothing
    Given the source:
      """
      fn main() -> number { phantom }
      """
    When I go to the definition of "phantom"
    Then it resolves to nothing

  @chaos
  Scenario Outline: Type definition of a const whose type is a primitive resolves to nothing
    Given the source:
      """
      const VALUE: <type> = <literal>
      fn read() -> <type> { VALUE }
      """
    When I go to the type of "VALUE"
    Then it resolves to nothing

    Examples:
      | type   | literal |
      | number | 9       |
      | string | "x"     |
      | bool   | true    |

  @chaos
  Scenario: Type definition of a local bound to a number literal resolves to nothing
    Given the source:
      """
      fn total() -> number { let n = 41; n }
      """
    When I go to the type of "n"
    Then it resolves to nothing

  @chaos
  Scenario: Type definition of a parameter annotated with a primitive resolves to nothing
    Given the source:
      """
      fn double(amount: number) -> number { amount + amount }
      """
    When I go to the type of "amount"
    Then it resolves to nothing

  @chaos
  Scenario: Type definition of an undeclared value resolves to nothing
    Given the source:
      """
      fn main() -> number { unknown }
      """
    When I go to the type of "unknown"
    Then it resolves to nothing

  @chaos
  Scenario Outline: Implementation of a type with no impl block resolves to nothing
    Given the source:
      """
      <decl>
      """
    When I go to the implementations of "<name>"
    Then it resolves to nothing

    Examples:
      | name    | decl                                                          |
      | Lonely  | struct Lonely { x: number }                                   |
      | Color   | enum Color { Red, Green }                                     |

  @chaos
  Scenario: Implementation of a trait with no implementor resolves to nothing
    Given the source:
      """
      trait Drawable { fn draw(self) -> string; }
      struct Widget { id: number }
      """
    When I go to the implementations of "Drawable"
    Then it resolves to nothing

  @chaos
  Scenario: Implementation of a free function resolves to nothing
    Given the source:
      """
      fn helper() -> number { 1 }
      fn run() -> number { helper() }
      """
    When I go to the implementations of "helper"
    Then it resolves to nothing
