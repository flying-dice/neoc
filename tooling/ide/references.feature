Feature: References

  Find-all-references returns every occurrence of the name under the cursor,
  resolved by name across the document. Locations are 1-based (line, column) —
  the editor's own coordinates. The declaration is part of the result unless the
  request excludes it.

  Scenario: All references to a function include its declaration and call sites
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn main() -> number { id(id(1)) }
      """
    When I find all references to "id"
    Then the references are at:
      | line | column |
      | 1    | 4      |
      | 2    | 23     |
      | 2    | 26     |

  Scenario: Excluding the declaration drops the defining name
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn main() -> number { id(id(1)) }
      """
    When I find references to "id" excluding the declaration
    Then the references are at:
      | line | column |
      | 2    | 23     |
      | 2    | 26     |

  Scenario: References to a type span its declaration and every use
    Given the source:
      """
      struct Point { x: number }
      fn make() -> Point { Point { x: 1 } }
      """
    When I find all references to "Point"
    Then the references are at:
      | line | column |
      | 1    | 8      |
      | 2    | 14     |
      | 2    | 22     |

  # --- chaos: unhappy paths the happy-path suite misses ----------------------

  # Resolution is scope-aware: a `let acc` inside the body opens a fresh binding
  # that shadows the `acc` parameter. References to the parameter stay on the
  # parameter — they never bleed across the shadow boundary into the inner `acc`
  # or its read. The parameter is unused here, so it is its own only occurrence.
  @chaos
  Scenario: A parameter's references stop at a shadowing inner let
    Given the source:
      """
      fn f(acc: number) -> number { let acc = 1; acc }
      """
    When I find all references to "acc"
    Then the references are at:
      | line | column |
      | 1    | 6      |

  # The same boundary holds for a top-level item: a `let calc` in `main` shadows
  # the `calc` function. References to the function find only its declaration —
  # renaming the function must never rewrite the unrelated local of the same name.
  @chaos
  Scenario: A top-level function's references do not reach a local that shadows it
    Given the source:
      """
      fn calc(a: number) -> number { a }
      fn main() -> number { let calc = 1; calc }
      """
    When I find all references to "calc"
    Then the references are at:
      | line | column |
      | 1    | 4      |

  # Excluding the declaration from that same shadowed item leaves nothing: the
  # only occurrence is the declaration, and the local `calc` is a different
  # binding the result already excludes.
  @chaos
  Scenario: Excluding the declaration of a shadowed item yields no references
    Given the source:
      """
      fn calc(a: number) -> number { a }
      fn main() -> number { let calc = 1; calc }
      """
    When I find references to "calc" excluding the declaration
    Then the references are at:
      | line | column |

  # Two functions each take a parameter named `acc`. They live in disjoint
  # scopes, so they are distinct bindings — references to the first function's
  # `acc` cover its declaration and its body read, never the second function's.
  @chaos
  Scenario: Identically named parameters in sibling scopes are not conflated
    Given the source:
      """
      fn f(acc: number) -> number { acc }
      fn g(acc: number) -> number { acc }
      """
    When I find all references to "acc"
    Then the references are at:
      | line | column |
      | 1    | 6      |
      | 1    | 31     |

  # A type and a value can share a spelling: the `Tag` struct and a `let Tag`
  # value. References from the type's declaration resolve to the type only — the
  # value binding that shadows the name is left out, even though it is read.
  @chaos
  Scenario: A type's references exclude a value binding of the same name
    Given the source:
      """
      struct Tag { id: number }
      fn main() -> number { let Tag = 7; Tag }
      """
    When I find all references to "Tag"
    Then the references are at:
      | line | column |
      | 1    | 8      |

  # Excluding the declaration of that struct, with no type-position use anywhere,
  # leaves an empty result — the value `Tag` is never swept in.
  @chaos
  Scenario: Excluding the declaration of an unused type yields no references
    Given the source:
      """
      struct Tag { id: number }
      fn main() -> number { let Tag = 7; Tag }
      """
    When I find references to "Tag" excluding the declaration
    Then the references are at:
      | line | column |

  # A binder introduced by a `for` pattern shadows an outer `let row`; the outer
  # binding, unused after the loop opens, is its own only reference. The loop
  # variable and its body read belong to the inner binding and stay out.
  @chaos
  Scenario: A for-loop binder shadows an outer let of the same name
    Given the source:
      """
      fn main() { let row = 0; for row in 1 { row } }
      """
    When I find all references to "row"
    Then the references are at:
      | line | column |
      | 1    | 17     |

  # A `match` arm pattern binds a fresh `val` that shadows the outer `let val`.
  # References to the outer binding cover its declaration and the scrutinee read
  # only — the arm's binding occurrence and the arm body's read are excluded.
  @chaos
  Scenario: A match-arm binder shadows the outer scrutinee binding
    Given the source:
      """
      fn main() -> number { let val = 1; match val { val => val } }
      """
    When I find all references to "val"
    Then the references are at:
      | line | column |
      | 1    | 27     |
      | 1    | 42     |

  # A closure parameter `acc` shadows the surrounding `let acc`. The outer
  # binding's references are its declaration and the trailing read after the
  # closure — never the closure's parameter or the body that reads it.
  @chaos
  Scenario: A closure parameter shadows an enclosing let
    Given the source:
      """
      fn main() -> number { let acc = 1; let g = |acc: number| -> number { acc }; acc }
      """
    When I find all references to "acc"
    Then the references are at:
      | line | column |
      | 1    | 27     |
      | 1    | 77     |

  # Excluding the declaration when the binding has no reads at all gives an empty
  # result — there is nothing left once the defining name is dropped. The same
  # outcome holds whether the lone declaration is an item or a local parameter.
  @chaos
  Scenario Outline: Excluding the declaration of an unreferenced name is empty
    Given the source:
      """
      <source>
      """
    When I find references to "<name>" excluding the declaration
    Then the references are at:
      | line | column |

    Examples:
      | source                           | name   |
      | struct Widget { id: number }     | Widget |
      | fn f(acc: number) -> number { 0 } | acc    |

  # A `const` shadowed by a `let` of the same name: references to the constant
  # resolve to its declaration alone. The local `LIMIT` is a separate binding the
  # constant's references never reach.
  @chaos
  Scenario: A constant's references stop at a shadowing local
    Given the source:
      """
      const LIMIT: number = 9;
      fn main() -> number { let LIMIT = 1; LIMIT }
      """
    When I find all references to "LIMIT"
    Then the references are at:
      | line | column |
      | 1    | 7      |

  # Intended-but-unverified: with the cursor inside the `b.x` access, find-refs
  # should be type-scoped — `B`'s field declaration plus the `b.x` read only,
  # never `A`'s `x` or `a.x`. The reference step anchors the cursor on the first
  # `x` (here `A`'s field declaration), which falls back to a name-only match and
  # sweeps every `x`; the type-scoped result is unreachable through this step, so
  # this stays @pending until a member-anchored reference step exists.
  @chaos @pending
  Scenario: Field references are scoped to the owning struct
    Given the source:
      """
      struct A { x: number }
      struct B { x: number }
      fn f(a: A, b: B) -> number { a.x + b.x }
      """
    When I find all references to "x"
    Then the references are at:
      | line | column |
      | 2    | 12     |
      | 3    | 38     |
