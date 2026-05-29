Feature: In-page transpilation

  The browser entry point is a wasm-bindgen wrapper over `neoc-compiler`: a page
  hands it `.neoc` source text and a target — `lua` or `js` — and gets the
  emitted output back, transpiled entirely in the browser with no server round
  trip. It is the same `transpile` seam the native runtime embeds, compiled to
  wasm.

  This wrapper does not exist yet. `neoc-wasm` is a stub until the compiler graph
  is wasm-clean and the wasm-bindgen surface lands, and a native test cannot
  drive a wasm-only entry regardless. So every scenario here is `@chaos @pending`
  — the runner filters them out (zero run, suite green). They are the contract to
  satisfy when the entry point is built; drop `@pending` and wire the steps to
  the real export then.

  @chaos @pending
  Scenario: A function transpiles to Lua in the browser
    Given the source:
      """
      fn add(a: number, b: number) -> number { a + b }
      """
    And the target is "lua"
    When I transpile it in the browser
    Then it transpiles to:
      """
      local function add(a, b)
          return a + b
      end
      """

  @chaos @pending
  Scenario: The same source transpiles to JavaScript for the same page
    Given the source:
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the target is "js"
    When I transpile it in the browser
    Then it transpiles to:
      """
      export function add(a, b) {
        return a + b;
      }
      """

  @chaos @pending
  Scenario: An unknown target is refused rather than guessed
    Given the source:
      """
      fn main() {}
      """
    And the target is "wasm"
    When I transpile it in the browser
    Then transpilation is refused

  @chaos @pending
  Scenario: Source with a syntax error surfaces a diagnostic, not mangled output
    Given the source:
      """
      fn add(a: number) -> number { a +
      """
    And the target is "lua"
    When I transpile it in the browser
    Then transpilation is refused

  @chaos @pending
  Scenario: An empty source transpiles to empty output
    Given the source:
      """
      """
    And the target is "lua"
    When I transpile it in the browser
    Then it transpiles to:
      """
      """
