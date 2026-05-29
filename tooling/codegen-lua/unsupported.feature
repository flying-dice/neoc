Feature: Refusing unsupported constructs

  Codegen runs on a parsed-but-unchecked AST. A construct the lowering cannot
  express — a refutable pattern in `let`, a destructuring `let` with no
  initialiser — is refused with an emit diagnostic rather than silently
  producing wrong Lua. The diagnostic names the refused construct in neoc terms,
  never the host AST.

  @chaos
  Scenario: A refutable enum-variant pattern in `let` is refused in neoc terms
    Given the source:
      """
      fn first(r: Result<number, string>) -> number {
        let Ok(x) = r;
        x
      }
      """
    When I emit Lua
    Then emitting reports a diagnostic
    And a diagnostic reports "refutable pattern in `let` is not supported: this pattern can fail to match"

  @chaos
  Scenario: An or-pattern in `let` is refused in neoc terms
    Given the source:
      """
      fn pick(n: number) -> number {
        let 1 | 2 = n;
        n
      }
      """
    When I emit Lua
    Then emitting reports a diagnostic
    And a diagnostic reports "an or-pattern can fail to match"

  @chaos
  Scenario: An irrefutable binding `let` is accepted, not refused
    Given the source:
      """
      fn echo(n: number) -> number {
        let m = n;
        m
      }
      """
    When I emit Lua
    Then the Lua contains "local m = n"
    And emitting reports no diagnostics
