Feature: Transpiling

  The compiler lowers neoc source to self-typed Luau through the single
  `transpile` seam. A well-formed module compiles with no diagnostics: Luau emits
  `local function`, picks `..` for string concatenation, and erases nothing.
  (Plain Lua is not a selectable target — downgrade the typed superset through
  the Luau toolchain.)

  Scenario: A function transpiles to Luau
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I transpile to luau
    Then it succeeds
    Then the output is:
      """
      local function add(a: number, b: number): number
        return (a + b)
      end
      """

  Scenario: A pub function is host-reachable, so Luau validates its parameters
    Given the source:
      """
      pub fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I transpile to luau
    Then it succeeds
    Then the output contains "function add(a: number, b: number): number"
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
    When I transpile to luau
    Then it succeeds
    Then the output contains:
      """
      ("hi " .. name)
      """
    Then the output does not contain:
      """
      ("hi " + name)
      """

  Scenario: A constant lowers to a local binding in Luau
    Given the source:
      """
      const MAX: number = 10;
      """
    When I transpile to luau
    Then it succeeds
    Then the output is:
      """
      local MAX: number = 10
      """

  Scenario: A struct emits constructors and validators in Luau
    Given the source:
      """
      struct Point { x: number, y: number }
      fn fst(p: Point) -> number { p.x }
      """
    When I transpile to luau
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
    When I transpile to luau
    Then it succeeds
    Then the output is empty
