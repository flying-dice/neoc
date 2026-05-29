Feature: Hover

  The hover handler renders a neoc-flavoured signature for the item under the
  cursor. Every rendered string speaks neoc, never the host language.

  Scenario: Hover on a generic function declaration
    Given the source:
      """
      fn first<T>(a: T, b: T) -> T { a }
      """
    When I hover on "first" at its declaration
    Then the hover shows:
      """
      fn first<T>(a: T, b: T) -> T
      """

  Scenario: Hover on a local binding shows its inferred type
    Given the source:
      """
      fn main() -> number {
        let total = 1 + 2
        total
      }
      """
    When I hover on "total" at the call site
    Then the hover shows:
      """
      total: number
      """

  Scenario: Hover at a call site instantiates type parameters
    Given the source:
      """
      fn first<T>(a: T, b: T) -> T { a }
      fn main() { println(first(7, 9)); }
      """
    When I hover on "first" at the call site
    Then the hover shows:
      """
      fn first<number>(a: number, b: number) -> number
      """

  @chaos
  Scenario: Hover on a function declaration with three type parameters
    Given the source:
      """
      fn pick<A, B, C>(a: A, b: B, c: C) -> C { c }
      """
    When I hover on "pick" at its declaration
    Then the hover shows:
      """
      fn pick<A, B, C>(a: A, b: B, c: C) -> C
      """

  @chaos
  Scenario: Hover at a call site instantiates a type parameter to string
    Given the source:
      """
      fn wrap<T>(x: T) -> T { x }
      fn main() { println(wrap("hi")); }
      """
    When I hover on "wrap" at the call site
    Then the hover shows:
      """
      fn wrap<string>(x: string) -> string
      """

  @chaos
  Scenario: Hover on a const at its declaration shows its type
    Given the source:
      """
      const MAX: number = 100;
      fn main() -> number { MAX }
      """
    When I hover on "MAX" at its declaration
    Then the hover shows:
      """
      MAX: number
      """

  @chaos
  Scenario: Hover on a type alias renders its definition
    Given the source:
      """
      type Id = number;
      fn main(x: Id) -> Id { x }
      """
    When I hover on "Id" at its declaration
    Then the hover shows:
      """
      type Id = number
      """

  @chaos
  Scenario Outline: Hover on a fieldless struct renders its declared form
    Given the source:
      """
      <decl>
      fn main() -> <name> { <expr> }
      """
    When I hover on "<name>" at its declaration
    Then the hover shows:
      """
      <decl>
      """

    Examples:
      | name   | decl                | expr      |
      | Marker | struct Marker;      | Marker    |
      | Wrap   | struct Wrap(number);| Wrap(1)   |

  @chaos
  Scenario: Hover on a multi-variant enum lists every variant
    Given the source:
      """
      enum Dir { North, South, East, West }
      fn main() -> Dir { North }
      """
    When I hover on "Dir" at its declaration
    Then the hover shows:
      """
      enum Dir { North, South, East, West }
      """

  @chaos
  Scenario: Hover on a local bound from a call result shows the returned type
    Given the source:
      """
      fn make() -> bool { true }
      fn main() -> bool {
        let ready = make()
        ready
      }
      """
    When I hover on "ready" at the call site
    Then the hover shows:
      """
      ready: bool
      """

  @chaos
  Scenario: Hover on a method taking only self shows its signature
    Given the source:
      """
      struct Counter { n: number }
      impl Counter {
        fn get(self) -> number { self.n }
      }
      fn main(c: Counter) -> number { c.get() }
      """
    When I hover on "get" at its declaration
    Then the hover shows:
      """
      fn get(self) -> number
      """
