Feature: Deriving on an enum

  On an enum, a derive branches on the runtime tag (`__tag`) and handles each
  variant by its payload shape — unit, tuple, or named (`lang/DYNAMIC.md` §7).
  Clone rebuilds the tagged table; Eq short-circuits on a tag mismatch then
  compares the matched variant's payload; Debug and Display render per variant.
  A unit variant carries no payload, so Eq emits no comparison branch for it.

  Scenario: Clone branches on the tag and rebuilds each variant
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Clone
    Then the emitted Lua is:
      """
      function Shape.clone(self)
        if self.__tag == "Dot" then
          return { __enum = "Shape", __tag = "Dot" }
        end
        if self.__tag == "Pair" then
          return { __enum = "Shape", __tag = "Pair", self[1], self[2] }
        end
        if self.__tag == "Named" then
          return { __enum = "Shape", __tag = "Named", label = self.label }
        end
      end
      """

  Scenario: Eq short-circuits on a tag mismatch then compares the payload
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Eq
    Then the emitted Lua is:
      """
      function Shape.eq(a, b)
        if a.__enum ~= b.__enum or a.__tag ~= b.__tag then
          return false
        end
        if a.__tag == "Pair" then
          return a[1] == b[1] and a[2] == b[2]
        end
        if a.__tag == "Named" then
          return a.label == b.label
        end
        return true
      end
      """

  Scenario: Eq skips a comparison branch for the unit variant
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Eq
    Then the derive contains "if a.__tag == "Pair" then"
    But the derive does not contain "if a.__tag == "Dot" then"

  Scenario: Debug renders each variant by its payload shape
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Debug
    Then it emits the line "    return "Dot""
    And it emits the line "    return "Pair(" .. tostring(self[1]) .. ", " .. tostring(self[2]) .. ")""
    And it emits the line "    return "Named { " .. "label = " .. tostring(self.label) .. " }""

  Scenario: Display drops the type-name decoration
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Display
    Then it emits the line "    return "Dot""
    And it emits the line "    return tostring(self[1]) .. ", " .. tostring(self[2])"
    And it emits the line "    return tostring(self.label)"

  @chaos
  Scenario: Deriving an unknown trait on an enum is rejected
    Given the enum:
      """
      enum Shape { Dot, Pair(number, number), Named { label: string } }
      """
    When I derive Hash
    Then deriving is rejected
    And the rejection message is "unknown derive `Hash`; the derivable traits are Clone, Eq, Debug, Display"
