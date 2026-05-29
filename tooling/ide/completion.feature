Feature: Completion details

  A completion item carries only its label and kind until the editor focuses it.
  Resolve then fills in the signature (detail) and the `///` documentation —
  neoc-flavoured, the same rendering hover shows. Keywords and members name no
  top-level item, so resolve leaves them untouched.

  Scenario: Resolving a function fills its signature and documentation
    Given the source:
      """
      /// Adds one.
      fn inc(n: number) -> number { n }
      fn main() -> number { inc(1) }
      """
    When I resolve the completion "inc"
    Then the completion detail is "fn inc(number) -> number"
    And the completion documentation is "Adds one."

  Scenario: Resolving a struct fills its signature
    Given the source:
      """
      struct Point { x: number, y: number }
      """
    When I resolve the completion "Point"
    Then the completion detail is "struct Point { x: number, y: number }"

  Scenario: Resolving a keyword adds no detail
    Given the source:
      """
      fn main() {}
      """
    When I resolve the completion "fn"
    Then the completion has no detail

  # --- chaos: unhappy paths the happy suite misses ---

  @chaos
  Scenario: Resolving a doc-comment-less function still fills its signature
    A function with no `///` above it has no documentation to show, but its
    signature is still resolved — the detail must not go missing just because
    the author wrote no doc.
    Given the source:
      """
      fn solo(n: number) -> number { n }
      """
    When I resolve the completion "solo"
    Then the completion detail is "fn solo(number) -> number"

  @chaos
  Scenario: Resolving an enum names its variants
    Given the source:
      """
      enum Color { Red, Green }
      """
    When I resolve the completion "Color"
    Then the completion detail is "enum Color { Red, Green }"

  @chaos
  Scenario: Resolving a type alias shows its right-hand side
    Given the source:
      """
      type Count = number;
      """
    When I resolve the completion "Count"
    Then the completion detail is "type Count = number"

  @chaos
  Scenario: Resolving a generic function keeps its type parameters abstract
    Completion resolve has no call site, so the type parameters stay abstract —
    `fn id(T) -> T`, never instantiated to a concrete type the way hover at a
    call site shows.
    Given the source:
      """
      fn id<T>(x: T) -> T { x }
      """
    When I resolve the completion "id"
    Then the completion detail is "fn id(T) -> T"

  @chaos
  Scenario: Resolving a const shows its type, not its value
    A const resolves to its declared type, never the literal it was assigned —
    `MAX: number`, not `10`.
    Given the source:
      """
      const MAX: number = 10;
      """
    When I resolve the completion "MAX"
    Then the completion detail is "MAX: number"

  @chaos
  Scenario Outline: Resolving a const of any type shows that type
    Given the source:
      """
      <decl>
      """
    When I resolve the completion "<name>"
    Then the completion detail is "<detail>"

    Examples:
      | decl                                  | name | detail        |
      | const FLAG: bool = true;              | FLAG | FLAG: bool    |
      | const XS: [number] = [];              | XS   | XS: [number]  |
      | struct Point { x: number }\nconst ORIGIN: Point = Point { x: 0 }; | ORIGIN | ORIGIN: Point |

  @chaos
  Scenario Outline: Resolving any keyword adds no detail
    Members and keywords name no top-level item, so resolve leaves every
    keyword untouched — not just `fn`.
    Given the source:
      """
      fn main() {}
      """
    When I resolve the completion "<keyword>"
    Then the completion has no detail

    Examples:
      | keyword  |
      | let      |
      | match    |
      | self     |
      | true     |
      | false    |
      | return   |
      | struct   |

  @chaos
  Scenario: Resolving a keyword in an empty document adds no detail
    Even with no items declared, the keywords are still offered, and resolving
    one adds nothing.
    Given the source:
      """
      fn x() {}
      """
    When I resolve the completion "loop"
    Then the completion has no detail

  @chaos
  Scenario: Resolving across a syntactically broken region still fills the signature
    A stray line of garbage above a good function must not stop resolve from
    rendering that function's signature.
    Given the source:
      """
      @@@ &&&
      fn good() -> number { 1 }
      """
    When I resolve the completion "good"
    Then the completion detail is "fn good() -> number"

  @chaos
  Scenario: Resolving an unterminated function declaration still fills its signature
    An editor sees half-typed source constantly — a function whose body brace is
    never closed must still resolve to its signature.
    Given the source:
      """
      fn cut(n: number) -> number {
      """
    When I resolve the completion "cut"
    Then the completion detail is "fn cut(number) -> number"

  @chaos
  Scenario: Resolving an unterminated declaration still carries its doc comment
    The `///` above a function that has not been closed yet is still its
    documentation; resolve fills both detail and documentation from the partial
    source.
    Given the source:
      """
      /// Halves a number.
      fn half(n: number) -> number {
      """
    When I resolve the completion "half"
    Then the completion detail is "fn half(number) -> number"
    And the completion documentation is "Halves a number."

  @chaos
  Scenario: Resolving a duplicated name resolves the first declaration
    Two functions share a name — invalid source the author is mid-edit on.
    Resolve picks the first declaration rather than failing.
    Given the source:
      """
      fn dup() -> number { 1 }
      fn dup() -> number { 2 }
      """
    When I resolve the completion "dup"
    Then the completion detail is "fn dup() -> number"

  @chaos
  Scenario: Resolving a function whose return is a Result renders the neoc type
    Given the source:
      """
      fn parse(s: string) -> Result<number, string> { todo }
      """
    When I resolve the completion "parse"
    Then the completion detail is "fn parse(string) -> Result<number, string>"

  @chaos @pending
  Scenario: An ordinary // note above a function is not its documentation
    A non-doc `//` comment must never leak into the resolved documentation. The
    detail is asserted live; the absence of documentation needs a negative
    documentation step the catalog does not yet provide.
    Given the source:
      """
      // just a note, not a doc
      fn noted(n: number) -> number { n }
      """
    When I resolve the completion "noted"
    Then the completion detail is "fn noted(number) -> number"

  @chaos @pending
  Scenario: A doc comment detached by a blank line is not documentation
    A `///` separated from the declaration by a blank line is detached; the
    resolved item should carry detail but no documentation. Pending a negative
    documentation step.
    Given the source:
      """
      /// detached

      fn gap(n: number) -> number { n }
      """
    When I resolve the completion "gap"
    Then the completion detail is "fn gap(number) -> number"

  @chaos @pending
  Scenario: Resolving an enum variant adds no detail
    A variant is a member, not a top-level item, so resolve should leave it
    untouched. Pending: a member is not offered as a top-level completion, so
    the catalog's `resolve the completion` step cannot reach it without panicking
    — needs a member-aware completion fixture.
    Given the source:
      """
      enum Color { Red, Green }
      """
    When I resolve the completion "Red"
    Then the completion has no detail
