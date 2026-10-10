Feature: Deriving on a struct

  A built-in derive reads a struct's fields straight from the AST and emits a
  Lua 5.1 method on the type's table (`lang/DYNAMIC.md` §6). Clone rebuilds the
  value behind its metatable; Eq compares each field; Debug prints the named
  form `Name { f = … }`; Display prints the bare values. A newtype struct
  carries its single field at index `1`.

  Scenario: Clone on a named struct copies every field behind the metatable
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Clone
    Then the emitted Lua is:
      """
      function Point.clone(self)
        return setmetatable({ __struct = "Point", x = self.x, y = self.y }, Point)
      end
      """

  Scenario: Clone on a newtype struct copies the field at index 1
    Given the struct:
      """
      struct Meters(number);
      """
    When I derive Clone
    Then it emits the line "  return setmetatable({ __struct = "Meters", self[1] }, Meters)"

  Scenario: Eq on a named struct compares the tag then every field
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Eq
    Then the emitted Lua is:
      """
      function Point.eq(a, b)
        return a.__struct == b.__struct and a.x == b.x and a.y == b.y
      end
      """

  Scenario: Debug on a named struct prints the named form
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Debug
    Then it emits the line "  return "Point { " .. "x = " .. tostring(self.x) .. ", " .. "y = " .. tostring(self.y) .. " }""

  Scenario: Debug on a newtype struct prints the positional form
    Given the struct:
      """
      struct Meters(number);
      """
    When I derive Debug
    Then it emits the line "  return "Meters(" .. tostring(self[1]) .. ")""

  Scenario: Display on a named struct prints the bare values
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Display
    Then the emitted Lua is:
      """
      function Point.display(self)
        return tostring(self.x) .. ", " .. tostring(self.y)
      end
      """

  Scenario: Eq and Display attach differently named methods
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Eq
    Then it emits the line "function Point.eq(a, b)"

  @chaos
  Scenario: Deriving an unknown trait is rejected
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Ord
    Then deriving is rejected
    And the rejection message is "unknown derive `Ord`; the derivable traits are Clone, Eq, Debug, Display"

  @chaos
  Scenario: A lowercase derive name is rejected — the trait names are exact
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive clone
    Then deriving is rejected
    And the rejection message is "unknown derive `clone`; the derivable traits are Clone, Eq, Debug, Display"

  Scenario: Eq and Debug send aggregate fields through the structural helpers
    Given the struct:
      """
      struct Outer { n: number, i: Inner, o: Option<number>, xs: [number] }
      """
    When I derive Eq
    Then it emits the line "  return a.__struct == b.__struct and a.n == b.n and __eq(a.i, b.i) and __eq(a.o, b.o) and __eq(a.xs, b.xs)"
    And the derive uses the helper "Eq"
    When I derive Debug
    Then the derive contains "tostring(self.n)"
    And the derive contains "__debug(self.i)"
    And the derive contains "__debug(self.xs)"
    And the derive uses the helper "Debug"

  Scenario: Scalar-only derives use no helper
    Given the struct:
      """
      struct Point { x: number, y: number }
      """
    When I derive Eq
    Then the derive uses no helper
