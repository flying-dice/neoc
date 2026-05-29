Feature: Parser rejects malformed source

  Malformed source must be reported, never silently accepted — a clean parse of
  broken source would let it slip through to the checker and emitter. The parser
  recovers from each error and reports one diagnostic per malformed region, so a
  single mistake yields one error, not a cascade. The reported message is
  host-agnostic: "syntax error", in neoc's own voice.

  @chaos
  Scenario: An unbalanced opening brace is reported
    Given the source:
      """
      fn add(a: number) -> number {
          a +
      """
    When I parse it
    Then parsing reports an error
    And a parse error mentions "syntax error"

  @chaos
  Scenario: A stray closing brace is reported
    Given the source:
      """
      fn add() {}
      }
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: Garbage tokens are reported
    Given the source:
      """
      @#$ %^& fn
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: An incomplete function with no body is reported
    Given the source:
      """
      fn add(a: number) -> number
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A function signature cut off mid-parameter is reported
    Given the source:
      """
      fn add(a: number
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A dangling let with no binding is reported
    Given the source:
      """
      fn f() {
          let
      }
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A struct field with no type is reported
    Given the source:
      """
      struct P {
          x:
      }
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A lone operator is reported
    Given the source:
      """
      +
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: An operator standing alone in a body is reported
    Given the source:
      """
      fn f() {
          +
      }
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: An unterminated string is reported
    Given the source:
      """
      fn f() {
          let s = "oops
      }
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A bare keyword with nothing after it is reported
    Given the source:
      """
      struct
      """
    When I parse it
    Then parsing reports an error

  @chaos
  Scenario: A non-ASCII identifier is reported
    Given the source:
      """
      fn café() -> number {
          1
      }
      """
    When I parse it
    Then parsing reports an error

  # Tree-sitter currently recovers from `1..2.3` and accepts it as a clean
  # parse, so the malformed range literal slips through to later stages. The
  # parser should reject a malformed numeric/range literal up front with a
  # "syntax error". Unblock when the grammar tightens range/number parsing.
  @chaos @pending
  Scenario: A malformed range-and-number literal is reported
    Given the source:
      """
      fn main() {
          let x = 1..2.3;
      }
      """
    When I parse it
    Then parsing reports an error

  # Tree-sitter currently accepts a keyword in the function-name position
  # (`fn fn() {}`) as a clean parse. Using a reserved word where a name is
  # required should be a "syntax error". Unblock when the grammar forbids
  # reserved words in name positions.
  @chaos @pending
  Scenario: A reserved word used as a function name is reported
    Given the source:
      """
      fn fn() {}
      """
    When I parse it
    Then parsing reports an error
