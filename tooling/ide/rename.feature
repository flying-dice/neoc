Feature: Rename

  Rename rewrites every occurrence of the name under the cursor — declaration
  and uses alike — to the new name. Resolution is name-based across the file,
  the same set find-all-references reports.

  Scenario: Renaming a function updates its declaration and every call
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I rename "fib" to "fibonacci"
    Then the source becomes:
      """
      fn fibonacci(n: number) -> number { fibonacci(n) }
      fn main() -> number { fibonacci(3) }
      """

  Scenario: Renaming a type updates its declaration and every use
    Given the source:
      """
      struct Point { x: number }
      fn make() -> Point { Point { x: 1 } }
      """
    When I rename "Point" to "Vec2"
    Then the source becomes:
      """
      struct Vec2 { x: number }
      fn make() -> Vec2 { Vec2 { x: 1 } }
      """

  # --- chaos: unhappy paths ---------------------------------------------------
  # Rename is a blind text rewrite of the resolved occurrence set (the same set
  # find-all-references reports). It performs NO collision, keyword, or validity
  # check on the new name — so a careless rename will cheerfully produce
  # shadowed, duplicated, or syntactically invalid neoc. These scenarios pin
  # that current behaviour; the `@pending` ones describe the guard we'd want.

  @chaos
  Scenario: Renaming to the same name leaves the source untouched
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I rename "fib" to "fib"
    Then the source becomes:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """

  @chaos
  Scenario: Renaming a function onto a name already taken collides silently
    Given the source:
      """
      fn helper() -> number { 1 }
      fn main() -> number { helper() }
      """
    When I rename "helper" to "main"
    Then the source becomes:
      """
      fn main() -> number { 1 }
      fn main() -> number { main() }
      """

  @chaos
  Scenario: Renaming a type onto an existing type collides silently
    Given the source:
      """
      struct Point { x: number }
      struct Vec2 { y: number }
      fn make() -> Point { Point { x: 1 } }
      """
    When I rename "Point" to "Vec2"
    Then the source becomes:
      """
      struct Vec2 { x: number }
      struct Vec2 { y: number }
      fn make() -> Vec2 { Vec2 { x: 1 } }
      """

  @chaos
  Scenario: Renaming a field onto a sibling field duplicates the field name
    Given the source:
      """
      struct Point { x: number, y: number }
      fn make() -> Point { Point { x: 1, y: 2 } }
      """
    When I rename "x" to "y"
    Then the source becomes:
      """
      struct Point { y: number, y: number }
      fn make() -> Point { Point { y: 1, y: 2 } }
      """

  @chaos
  Scenario: Renaming a top-level item leaves a shadowing local untouched
    Given the source:
      """
      fn helper() -> number { 1 }
      fn main() -> number {
        let helper = 2;
        helper
      }
      """
    When I rename "helper" to "aide"
    Then the source becomes:
      """
      fn aide() -> number { 1 }
      fn main() -> number {
        let helper = 2;
        helper
      }
      """

  @chaos
  Scenario: Renaming onto a keyword produces invalid neoc with no warning
    Given the source:
      """
      fn helper() -> number { 1 }
      fn main() -> number { helper() }
      """
    When I rename "helper" to "fn"
    Then the source becomes:
      """
      fn fn() -> number { 1 }
      fn main() -> number { fn() }
      """

  @chaos
  Scenario: Renaming to a non-ASCII identifier rewrites every occurrence
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I rename "fib" to "café"
    Then the source becomes:
      """
      fn café(n: number) -> number { café(n) }
      fn main() -> number { café(3) }
      """

  # @pending: the desired guard — renaming onto a reserved word should be
  # rejected so the buffer never goes invalid. Today the rewrite is blind (see
  # the `fn` scenario above), and there is no "rename is rejected" outcome step
  # to assert against, so this family stays pending until both land.
  @chaos @pending
  Scenario Outline: Renaming onto a reserved word is rejected
    Given the source:
      """
      fn helper() -> number { 1 }
      fn main() -> number { helper() }
      """
    When I rename "helper" to "<keyword>"
    Then the source becomes:
      """
      fn helper() -> number { 1 }
      fn main() -> number { helper() }
      """

    Examples:
      | keyword |
      | struct  |
      | let     |
      | match   |
      | ext     |

  # @pending: renaming onto a name already bound in the same scope should warn
  # about the collision before rewriting. Today it rewrites silently (see the
  # `helper` -> `main` scenario); this asserts the collision is instead refused
  # and the source left intact.
  @chaos @pending
  Scenario: Renaming onto a colliding sibling field is refused
    Given the source:
      """
      struct Point { x: number, y: number }
      fn make() -> Point { Point { x: 1, y: 2 } }
      """
    When I rename "x" to "y"
    Then the source becomes:
      """
      struct Point { x: number, y: number }
      fn make() -> Point { Point { x: 1, y: 2 } }
      """
