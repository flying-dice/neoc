Feature: Cross-file navigation and edits
  Goto-definition, find-all-references, and rename reach across the project,
  scoped by visibility through the cross-file index. A `pub` declaration is
  reachable from the modules that import it; the edit set never spreads to a
  module that does not.

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

  # --- @chaos: unhappy cross-file paths ---

  @chaos
  Scenario: Renaming a private name stays in its own file
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
    When I rename "helper" to "aux" from "secret.neoc"
    Then the rename edits the files:
      | file        |
      | secret.neoc |

  @chaos
  Scenario: References to a `pub` declaration with no importer touch only its file
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    When I find references to "add" from "util.neoc"
    Then the references span the files:
      | file      |
      | util.neoc |

  @chaos
  Scenario: A `use` of a missing module does not pull an unimported `pub` name across
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use missing::thing;
      pub fn run() -> number { add(1, 2) }
      """
    When I find references to "add" from "main.neoc"
    Then the references span the files:
      | file      |
      | main.neoc |

  @chaos
  Scenario: A single-segment `use` carries no module qualifier, so it forms no cross-file edge
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use add;
      pub fn run() -> number { add(1, 2) }
      """
    When I find references to "add" from "main.neoc"
    Then the references span the files:
      | file      |
      | main.neoc |

  @chaos
  Scenario: Goto disambiguates two `pub` declarations of one name by the importer's `use`
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "other.neoc":
      """
      pub fn add(a: number, b: number) -> number { a - b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I goto the definition of "add" from "main.neoc"
    Then the definition is in "util.neoc"

  @chaos
  Scenario: References to one of two same-named `pub` declarations stay with the imported module
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "other.neoc":
      """
      pub fn add(a: number, b: number) -> number { a - b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I find references to "add" from "main.neoc"
    Then the references span the files:
      | file      |
      | main.neoc |
      | util.neoc |

  @chaos
  Scenario: A `pub` rename reaches every importer of the module, not just the one renamed from
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "one.neoc":
      """
      use util::add;
      pub fn f() -> number { add(1, 2) }
      """
    And the file "two.neoc":
      """
      use util::add;
      pub fn g() -> number { add(3, 4) }
      """
    When I rename "add" to "plus" from "one.neoc"
    Then the rename edits the files:
      | file      |
      | one.neoc  |
      | two.neoc  |
      | util.neoc |

  @chaos
  Scenario: A name unknown to the project resolves to no references
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I find references to "nope" from "main.neoc"
    Then the references span the files:
      | file |

  @chaos
  Scenario: Renaming a name the project never declares edits nothing
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I rename "nope" to "x" from "main.neoc"
    Then the rename edits the files:
      | file |

  @chaos
  Scenario: A private name shared by two files, queried where it is absent, stays empty
    Given the file "util.neoc":
      """
      fn helper() -> number { 1 }
      pub fn add() -> number { helper() }
      """
    And the file "other.neoc":
      """
      fn helper() -> number { 2 }
      pub fn z() -> number { helper() }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add() }
      """
    When I find references to "helper" from "main.neoc"
    Then the references span the files:
      | file |

  @chaos
  Scenario: A `use`d module's private name does not cross into the importer that imports its `pub` sibling
    Given the file "util.neoc":
      """
      fn secret() -> number { 1 }
      pub fn add() -> number { secret() }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add() + secret() }
      """
    When I find references to "secret" from "main.neoc"
    Then the references span the files:
      | file      |
      | main.neoc |

  @chaos
  Scenario: A `pub` rename still rewrites an importer that already declares the new name
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn plus() -> number { 0 }
      pub fn run() -> number { add(1, 2) }
      """
    When I rename "add" to "plus" from "main.neoc"
    Then the rename edits the files:
      | file      |
      | main.neoc |
      | util.neoc |

  # --- @chaos @pending: no step asserts a cross-file goto that resolves to nothing ---
  # `the definition is in "<path>"` only asserts a positive landing; there is no
  # negative form for cross-file goto. These trace through the handler
  # (`imported_definition` returns nothing) but cannot be asserted with the
  # current vocabulary, so they stay filtered until such a step exists.

  @chaos @pending
  Scenario: Goto-definition does not cross a `use` into a private declaration
    Given the file "util.neoc":
      """
      fn secret() -> number { 1 }
      """
    And the file "main.neoc":
      """
      use util::secret;
      pub fn run() -> number { secret() }
      """
    When I goto the definition of "secret" from "main.neoc"
    Then the definition is in ""

  @chaos @pending
  Scenario: Goto-definition does not resolve a name whose module is never imported
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use missing::thing;
      pub fn run() -> number { add(1, 2) }
      """
    When I goto the definition of "add" from "main.neoc"
    Then the definition is in ""

  @chaos @pending
  Scenario: A `pub` rename across files does not flag a collision with the new name
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn plus() -> number { 0 }
      pub fn run() -> number { add(1, 2) }
      """
    When I rename "add" to "plus" from "main.neoc"
    Then the diagnostics include:
      | severity | message                                  |
      | error    | a declaration named `plus` already exists |
