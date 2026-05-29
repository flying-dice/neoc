Feature: Type-checking well-typed source

  The checker accepts source that obeys neoc's static semantics
  (`lang/STATIC.md`): arithmetic on `number`, concatenation on `string`, struct
  field access, generic constructors, exhaustive `match`, and inherent
  `Option`/`Result` methods. A well-typed module produces no diagnostics.

  Scenario: Arithmetic on numbers
    Given the source:
      """
      fn add(a: number, b: number) -> number { a + b }
      """
    When I type-check it
    Then it type-checks

  Scenario: String concatenation with +
    Given the source:
      """
      fn greet(n: string) -> string { "hi " + n }
      """
    When I type-check it
    Then it type-checks

  Scenario: A struct literal and a field read
    Given the source:
      """
      struct Point { x: number, y: number }
      fn origin() -> Point { Point { x: 0, y: 0 } }
      fn px(p: Point) -> number { p.x }
      """
    When I type-check it
    Then it type-checks

  Scenario: An if expression whose branches agree
    Given the source:
      """
      fn pick(c: bool) -> number { if c { 1 } else { 2 } }
      """
    When I type-check it
    Then it type-checks

  Scenario: A generic constructor instantiates the return type
    Given the source:
      """
      fn ok() -> Result<number, string> { Ok(1) }
      """
    When I type-check it
    Then it type-checks

  Scenario: An exhaustive match over an Option
    Given the source:
      """
      fn f(o: Option<number>) -> number { match o { Some(x) => x, None => 0 } }
      """
    When I type-check it
    Then it type-checks

  Scenario: A catch-all arm makes a match exhaustive
    Given the source:
      """
      enum Color { Red, Green, Blue }
      fn f(c: Color) -> number { match c { Green => 1, _ => 0 } }
      """
    When I type-check it
    Then it type-checks

  Scenario: An inherent Option method resolves on its instantiation
    Given the source:
      """
      fn f(o: Option<number>) -> number { o.map(|n| n + 1).unwrap_or(0) }
      """
    When I type-check it
    Then it type-checks

  Scenario: A type alias is transparent
    Given the source:
      """
      type Id = number;
      fn f(i: Id) -> number { i + 1 }
      """
    When I type-check it
    Then it type-checks
