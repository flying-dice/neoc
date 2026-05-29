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

  # --- chaos: unhappy paths ---

  @chaos
  Scenario: An empty argument list still marks the first parameter active
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add() }
      """
    When I request signature help after "add("
    Then the active signature is "fn add(a: number, b: number) -> number"
    And the active parameter is 0

  @chaos
  Scenario Outline: More arguments than parameters clamp to the last parameter
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn main() -> number { id(1, 2, 3) }
      """
    When I request signature help after "<marker>"
    Then the active signature is "fn id(x: number) -> number"
    And the active parameter is 0

    Examples:
      | marker  |
      | id(1,   |
      | id(1, 2 |

  @chaos
  Scenario: A trailing comma with no further argument clamps to the last parameter
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2,) }
      """
    When I request signature help after "add(1, 2,"
    Then the active parameter is 1

  @chaos
  Scenario: A zero-parameter call shows its signature with no active parameter
    Given the source:
      """
      fn now() -> number { 0 }
      fn main() -> number { now() }
      """
    When I request signature help after "now("
    Then the active signature is "fn now() -> number"

  @chaos
  Scenario: A call to an undeclared function gives no signature help
    Given the source:
      """
      fn main() -> number { ghost(1) }
      """
    When I request signature help after "ghost("
    Then there is no signature help

  @chaos
  Scenario: A method call gives no signature help yet
    Given the source:
      """
      struct S { x: number }
      impl S { fn go(self, n: number) -> number { n } }
      fn main(s: S) -> number { s.go(1) }
      """
    When I request signature help after "go("
    Then there is no signature help

  @chaos
  Scenario: The active parameter follows the cursor across a multi-line argument list
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number {
        add(
          1,
          2,
        )
      }
      """
    When I request signature help after "1,"
    Then the active parameter is 1

  # --- chaos: intended but unverified (filtered by the runner) ---

  @chaos @pending
  Scenario: A tuple-struct constructor call gives no signature help
    Given the source:
      """
      struct Wrap(number);
      fn main() -> number { Wrap(1) }
      """
    When I request signature help after "Wrap("
    Then there is no signature help

  @chaos @pending
  Scenario: A const used as a callee gives no signature help
    Given the source:
      """
      const LIMIT: number = 10;
      fn main() -> number { LIMIT(1) }
      """
    When I request signature help after "LIMIT("
    Then there is no signature help

  @chaos @pending
  Scenario: A path-qualified callee resolves to the same-named free function
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { Math::add(1, 2) }
      """
    When I request signature help after "Math::add("
    Then the active signature is "fn add(a: number, b: number) -> number"
    And the active parameter is 0

  @chaos @pending
  Scenario: The cursor just past the closing paren leaves the argument list
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2) }
      """
    When I request signature help after "add(1, 2)"
    Then there is no signature help
