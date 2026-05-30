Feature: Emitting TypeScript — module graph and imports

  A `use` lowers to a typed sibling import. The relative specifier is
  extensionless (TypeScript resolves the sibling `.ts`), unlike the JS target's
  `.js`-suffixed form. A `use` that resolves to no binding emits no dangling
  import. The single-file harness resolves no module graph, so the *resolved*
  import shape is verified in the crate's goldens and pinned here as `@pending`.

  Scenario: A use that resolves to no binding emits no dangling import
    Given the source:
      """
      use math::add;

      pub fn main() -> number {
          add(1, 2)
      }
      """
    When I emit TypeScript
    Then the TS does not contain "import { add }"
    And the TS contains "return add(1, 2);"

  @pending
  Scenario: A resolved use imports a sibling by its extensionless specifier
    Given the source:
      """
      use a::b::helper;

      pub fn f() -> number {
          helper()
      }
      """
    When I emit TypeScript
    Then the TS contains "import { helper } from "./a/b";"
    And the TS does not contain ".js""

  Scenario: An extern mod keeps its host package specifier
    Given the source:
      """
      extern {
          mod fs {
              fn read(path: string) -> string;
          }
      }

      pub fn cfg() -> string {
          fs::read("c")
      }
      """
    When I emit TypeScript
    Then the TS contains "from "fs";"
