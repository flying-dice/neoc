Feature: Completion details

  A completion item carries only its label and kind until the editor focuses it.
  Resolve then fills in the signature (detail) and the `///` documentation —
  neoc-flavoured, the same rendering hover shows. Keywords and members name no
  top-level item, so resolve leaves them untouched.

  Scenario: Resolving a function fills its signature and documentation
    Given the source:
      """
      /// Adds one.
      fn inc(n: number) -> number { n }
      fn main() -> number { inc(1) }
      """
    When I resolve the completion "inc"
    Then the completion detail is "fn inc(number) -> number"
    And the completion documentation is "Adds one."

  Scenario: Resolving a struct fills its signature
    Given the source:
      """
      struct Point { x: number, y: number }
      """
    When I resolve the completion "Point"
    Then the completion detail is "struct Point { x: number, y: number }"

  Scenario: Resolving a keyword adds no detail
    Given the source:
      """
      fn main() {}
      """
    When I resolve the completion "fn"
    Then the completion has no detail
