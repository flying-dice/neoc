Feature: Emitting JavaScript — sharp edges

  The emitter is infallible: a syntax error is held by the parser, never the
  emitter, and cross-module decisions are made from the resolved graph, not
  guessed. These scenarios pin the edges of ES-module output where a naive
  backend would leak a dangling import or a host-only keyword.

  @chaos
  Scenario: A let mut erases the mut — JavaScript has no such keyword
    Given the source:
      """
      fn main() {
          let mut count = 0;
          count = count + 1;
      }
      """
    When I emit JavaScript
    Then the JS contains "let count = 0;"
    And the JS does not contain "mut"
    And the JS does not contain "const count"

  @chaos
  Scenario: A use that resolves to no binding emits no dangling import
    Given the source:
      """
      use math::add;

      fn main() {
          add(1, 2)
      }
      """
    When I emit JavaScript
    Then the JS does not contain "import"
    And the JS contains "return add(1, 2);"

  @chaos
  Scenario: A source with a syntax error is rejected, not emitted
    Given the source:
      """
      fn oops( {
      """
    When I emit JavaScript
    Then the source is rejected

  @chaos
  Scenario: An empty module emits an empty ES module without problems
    Given the source:
      """
      """
    When I emit JavaScript
    Then the emit reports no problems
    And the JS does not contain "export"

  @chaos
  Scenario: A refutable pattern in `let` is refused in neoc terms
    Given the source:
      """
      fn pick(n: number) -> number {
          let 1 = n;
          n
      }
      """
    When I emit JavaScript
    Then the emit reports "refutable pattern in `let` is not supported: a literal pattern can fail to match"
