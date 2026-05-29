Feature: Reporting type errors

  Ill-typed source is refused with a specific diagnostic. Every message speaks
  neoc — `number`, `string`, `bool`, user-defined `struct`s and `enum`s — and
  never names a host-language type. Each expected substring here is the verbatim
  message the checker emits (`neoc-types/src/infer`).

  @chaos
  Scenario: An unknown name is reported
    Given the source:
      """
      fn f() -> number { missing }
      """
    When I type-check it
    Then a type error reports "unknown name `missing`"

  @chaos
  Scenario: A return type that disagrees with the body
    Given the source:
      """
      fn f() -> number { "oops" }
      """
    When I type-check it
    Then a type error reports "function body expects `number`, found `string`"

  @chaos
  Scenario: Calling a function with too few arguments
    Given the source:
      """
      fn add(a: number, b: number) -> number { a + b }
      fn f() -> number { add(1) }
      """
    When I type-check it
    Then a type error reports "this call passes 1 argument(s) but the function takes 2"

  @chaos
  Scenario: An argument of the wrong type
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn f() -> number { id("no") }
      """
    When I type-check it
    Then a type error reports "argument 1 expects `number`, found `string`"

  @chaos
  Scenario: Adding two bools is rejected
    Given the source:
      """
      fn f(a: bool, b: bool) -> bool { a + b }
      """
    When I type-check it
    Then a type error reports "`+` expects two `number`s or two `string`s, found `bool` and `bool`"

  @chaos
  Scenario: Calling a non-function value
    Given the source:
      """
      fn f() -> number { let x = 5; x(1) }
      """
    When I type-check it
    Then a type error reports "`number` is not callable"

  @chaos
  Scenario: Reading a field a struct does not have
    Given the source:
      """
      struct Point { x: number, y: number }
      fn f(p: Point) -> number { p.z }
      """
    When I type-check it
    Then a type error reports "struct `Point` has no field `z`"

  @chaos
  Scenario: Calling a method a struct does not have
    Given the source:
      """
      struct P { n: number }
      fn f(p: P) -> number { p.nope() }
      """
    When I type-check it
    Then a type error reports "type `P` has no method `nope`"

  @chaos
  Scenario: A list whose elements disagree
    Given the source:
      """
      fn f() -> [number] { [1, "two"] }
      """
    When I type-check it
    Then a type error reports "list elements disagree: `number` vs `string`"

  @chaos
  Scenario: A non-exhaustive match over an enum names the missing variant
    Given the source:
      """
      enum Color { Red, Green, Blue }
      fn f(c: Color) -> number { match c { Green => 1, Blue => 2 } }
      """
    When I type-check it
    Then a type error reports "this match on `Color` is not exhaustive; missing variant(s): Red"

  @chaos
  Scenario: The if branches disagree
    Given the source:
      """
      fn f(c: bool) -> number { if c { 1 } else { "x" } }
      """
    When I type-check it
    Then a type error reports "`if` branches disagree: `number` vs `string`"
