Feature: Emitting Lua

  The emitter lowers a parsed neoc module to Lua 5.1 (`lang/DYNAMIC.md`). A
  private free function becomes a `local function`; a `pub` one becomes a global
  `function` and validates its typed parameters at the host boundary. Structs,
  enums, and consts each lower to their documented runtime shape. Snippets are
  matched by substring — the emitted file also carries the runtime preamble
  (the `__check_*` validators, the `Result`/`Option` prelude) — so a scenario
  asserts the construct it is about, not the whole file.

  Scenario: A private function emits a local Lua function
    Given the source:
      """
      fn add(a: number, b: number) -> number {
        a + b
      }
      """
    When I emit Lua
    Then the Lua is:
      """
      local function add(a, b)
        return (a + b)
      end
      """

  Scenario: A pub function emits a global Lua function
    Given the source:
      """
      pub fn add(a: number, b: number) -> number {
        a + b
      }
      """
    When I emit Lua
    Then the Lua contains "function add(a, b)"
    And the Lua does not contain "local function add"

  Scenario: A pub function validates its typed parameters at the host boundary
    Given the source:
      """
      pub fn add(a: number, b: number) -> number {
        a + b
      }
      """
    When I emit Lua
    Then the Lua contains:
      """
      __check_number(a, "add.a")
      """
    And the Lua contains:
      """
      __check_number(b, "add.b")
      """

  Scenario: A unit struct lowers to a tagged table
    Given the source:
      """
      pub struct Marker;
      """
    When I emit Lua
    Then the Lua is:
      """
      Marker = { __struct = "Marker" }
      """

  Scenario: A named struct lowers to a constructor with a metatable
    Given the source:
      """
      pub struct Point {
        x: number,
        y: number,
      }
      """
    When I emit Lua
    Then the Lua contains "Point.__index = Point"
    And the Lua contains "function Point.new(fields)"
    And the Lua contains:
      """
      fields.__struct = "Point"
      """

  Scenario: A pub const lowers to a global assignment
    Given the source:
      """
      pub const ANSWER: number = 42;
      """
    When I emit Lua
    Then the Lua is:
      """
      ANSWER = 42
      """

  Scenario: A plain enum emits no standalone Lua
    Given the source:
      """
      enum Color {
        Red,
        Green,
      }
      """
    When I emit Lua
    Then the Lua is empty
    And emitting reports no diagnostics

  Scenario: A method lowers to a colon-dispatched function on the type table
    Given the source:
      """
      struct Counter {
        n: number,
      }

      impl Counter {
        fn bump(self) -> number {
          self.n
        }
      }
      """
    When I emit Lua
    Then the Lua contains "function Counter.bump(self)"
    And emitting reports no diagnostics
