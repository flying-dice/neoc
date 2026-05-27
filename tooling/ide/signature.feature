Feature: Signature help

  While the cursor is inside a call's argument list, signature help shows the
  callee's signature — with parameter names, neoc-flavoured — and marks the
  active parameter (0-based) by the cursor's position among the arguments.

  Scenario: The first argument marks the first parameter active
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2) }
      """
    When I request signature help after "add("
    Then the active signature is "fn add(a: number, b: number) -> number"
    And the active parameter is 0

  Scenario: The cursor past a comma advances the active parameter
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2) }
      """
    When I request signature help after "add(1, "
    Then the active parameter is 1

  Scenario: A nested call resolves to the inner callee
    Given the source:
      """
      fn inner(a: number) -> number { a }
      fn outer(b: number) -> number { b }
      fn main() -> number { outer(inner(1)) }
      """
    When I request signature help after "inner("
    Then the active signature is "fn inner(a: number) -> number"

  Scenario: Outside any call there is no signature help
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2) }
      """
    When I request signature help after "fn main"
    Then there is no signature help
