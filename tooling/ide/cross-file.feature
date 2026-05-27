Feature: Cross-file navigation and edits
  Goto-definition, find-all-references, and rename reach across the project,
  scoped by visibility through the cross-file index
  (tasks/cross-file-resolution.md). A `pub` declaration is reachable from the
  modules that import it; the edit set never spreads to a module that does not.

  Scenario: Goto-definition follows a `use` into the defining module
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I goto the definition of "add" from "main.neoc"
    Then the definition is in "util.neoc"

  Scenario: References to a pub declaration span the defining file and its importers
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(add(1, 2), 3) }
      """
    When I find references to "add" from "main.neoc"
    Then the references span the files:
      | file       |
      | main.neoc  |
      | util.neoc  |

  Scenario: Renaming a pub declaration rewrites the defining file and its importers
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I rename "add" to "plus" from "main.neoc"
    Then the rename edits the files:
      | file       |
      | main.neoc  |
      | util.neoc  |

  Scenario: A private name stays in its own file
    Given the file "secret.neoc":
      """
      fn helper() -> number { 1 }
      pub fn run() -> number { helper() }
      """
    And the file "other.neoc":
      """
      fn helper() -> number { 2 }
      pub fn go() -> number { helper() }
      """
    When I find references to "helper" from "secret.neoc"
    Then the references span the files:
      | file        |
      | secret.neoc |
