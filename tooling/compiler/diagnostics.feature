Feature: Diagnostics

  Compilation is staged — parse, then type, then emit — and every problem is
  tagged with the stage that found it. A diagnostic speaks neoc: it names neoc
  types (`number`, `string`) and neoc constructs, never the host language. These
  scenarios pin the unhappy paths: a syntax error, an unresolved name, a type
  mismatch, and a missing method, each reported at its own stage.

  @chaos
  Scenario: Unbalanced braces are a parse error
    Given the source:
      """
      fn add(a: number) -> number { a +
      """
    When I transpile to luau
    Then it fails
    Then a parse diagnostic reports "syntax error"

  @chaos
  Scenario: An unknown name is a type error
    Given the source:
      """
      fn f() -> number {
          nope
      }
      """
    When I transpile to luau
    Then it fails
    Then a type diagnostic reports "unknown name `nope`"

  @chaos
  Scenario: A return-type mismatch names neoc types, not host types
    Given the source:
      """
      fn f() -> number {
          "x"
      }
      """
    When I transpile to luau
    Then it fails
    Then a type diagnostic reports "expects `number`, found `string`"

  @chaos
  Scenario: Calling an absent method on a number is a type error
    Given the source:
      """
      fn f(n: number) -> number {
          n.frobnicate()
      }
      """
    When I transpile to luau
    Then it fails
    Then a type diagnostic reports "type `number` has no method `frobnicate`"

  @chaos
  Scenario: A parse error reports against any target
    Given the source:
      """
      fn add(a: number) -> number { a +
      """
    When I transpile to ts
    Then it fails
    Then a parse diagnostic reports "syntax error"
