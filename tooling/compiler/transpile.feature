Feature: Transpiling

  The compiler lowers neoc source to Lua 5.1 (the `LuaJIT` dialect) through the
  single `transpile` seam. A well-formed module compiles with no diagnostics: the
  output uses `local function` and picks `..` for string concatenation.

  Scenario: A function transpiles to Lua
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I transpile to lua
    Then it succeeds
    Then the output is:
      """
      local function add(a, b)
        return (a + b)
      end
      """

  Scenario: A pub function is host-reachable, so Lua validates its parameters
    Given the source:
      """
      pub fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I transpile to lua
    Then it succeeds
    Then the output contains "function add(a, b)"
    Then the output contains:
      """
      __check_number(a, "add.a")
      """
    Then the output contains:
      """
      __check_number(b, "add.b")
      """

  Scenario: String + lowers to concatenation
    Given the source:
      """
      fn greet(name: string) -> string {
          "hi " + name
      }
      """
    When I transpile to lua
    Then it succeeds
    Then the output contains:
      """
      ("hi " .. name)
      """
    Then the output does not contain:
      """
      ("hi " + name)
      """

  Scenario: A constant lowers to a local binding in Lua
    Given the source:
      """
      const MAX: number = 10;
      """
    When I transpile to lua
    Then it succeeds
    Then the output is:
      """
      local MAX = 10
      """

  Scenario: A struct emits constructors and validators in Lua
    Given the source:
      """
      struct Point { x: number, y: number }
      fn fst(p: Point) -> number { p.x }
      """
    When I transpile to lua
    Then it succeeds
    Then the output contains "function Point.new(fields)"
    Then the output contains:
      """
      fields.__struct = "Point"
      """

  Scenario: An empty module transpiles to empty output
    Given the source:
      """

      """
    When I transpile to lua
    Then it succeeds
    Then the output is empty
