Feature: Document highlight

  Highlighting a name marks every occurrence in the open file. The declaring
  occurrence is a write; the rest are reads. Locations are 1-based (line,
  column) — the editor's own coordinates.

  Scenario: The declaration is a write and its uses are reads
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      fn main() -> number { fib(3) }
      """
    When I highlight "fib"
    Then the highlights are:
      | line | column | kind  |
      | 1    | 4      | write |
      | 1    | 31     | read  |
      | 2    | 23     | read  |

  # The write/read split keys off a *declaration* — a top-level item or a unique
  # member. A parameter is neither: it is a local binding. So every occurrence of
  # a parameter, including the binding itself, is a read; the write tag never
  # appears. The handler's "declaring name is a write" holds for items, not locals.
  @chaos
  Scenario: A parameter has no write — even its binding is a read
    Given the source:
      """
      fn scale(factor: number) -> number { factor + factor + factor }
      """
    When I highlight "factor"
    Then the highlights are:
      | line | column | kind |
      | 1    | 10     | read |
      | 1    | 38     | read |
      | 1    | 47     | read |
      | 1    | 56     | read |

  # A `let` binding is local too — its def and both reads are all reads. No write.
  @chaos
  Scenario: A let binding and its reads are all reads
    Given the source:
      """
      fn run() -> number {
        let total = 1;
        total + total
      }
      """
    When I highlight "total"
    Then the highlights are:
      | line | column | kind |
      | 2    | 7      | read |
      | 3    | 3      | read |
      | 3    | 11     | read |

  # The same name binds a parameter in two separate functions. Highlight is
  # scope-aware: the cursor on the first function's `item` marks only that
  # function's binding and use — the second function's `item` is a different
  # binding and stays dark.
  @chaos
  Scenario: A name reused in two scopes highlights only the cursor's scope
    Given the source:
      """
      fn first(item: number) -> number { item }
      fn second(item: number) -> number { item }
      """
    When I highlight "item"
    Then the highlights are:
      | line | column | kind |
      | 1    | 10     | read |
      | 1    | 36     | read |

  # An inner `let value` shadows the outer parameter `value`. The cursor lands on
  # the parameter — its first occurrence — so only the parameter is highlighted;
  # the shadowing `let` and its read in the body belong to a different binding.
  @chaos
  Scenario: Shadowing keeps the highlight on the outer binding only
    Given the source:
      """
      fn pick(value: number) -> number {
        let value = 2;
        value
      }
      """
    When I highlight "value"
    Then the highlights are:
      | line | column | kind |
      | 1    | 9      | read |

  # A `let` bound but never read: only the binding occurrence, and it is a read,
  # never a write — locals carry no write tag regardless of use count.
  @chaos
  Scenario: An unread let binding is a single read, not a write
    Given the source:
      """
      fn run() -> number {
        let spare = 1;
        0
      }
      """
    When I highlight "spare"
    Then the highlights are:
      | line | column | kind |
      | 2    | 7      | read |

  # Two structs declare a field named `x`. The cursor on the *first* `x` lands on
  # a field declaration, not a `recv.member` use, so resolution falls back to a
  # name match and sweeps in both declarations and both accesses — none tagged a
  # write. Type-scoping only kicks in when the cursor is on an access (`b.x`).
  # @pending until cursor-on-field-declaration is type-scoped like cursor-on-access.
  @chaos @pending
  Scenario: A field name shared by two structs stays type-scoped from a declaration
    Given the source:
      """
      struct A { x: number }
      struct B { x: number }
      fn read_xs(a: A, b: B) -> number { a.x + b.x }
      """
    When I highlight "x"
    Then the highlights are:
      | line | column | kind  |
      | 1    | 12     | write |

  # Placing the cursor on an operator is not placing it on an identifier. The
  # highlight set is empty — no panic, no spurious read.
  @chaos
  Scenario: Highlighting from an operator yields nothing
    Given the source:
      """
      fn run() -> number { 1 + 1 }
      """
    When I highlight "+"
    Then the highlights are:
      | line | column | kind |
