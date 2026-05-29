Feature: Formatting

  Formatting rewrites a document to neoc's canonical form via the shared
  formatter. A document with a syntax error is left untouched.

  Scenario: A messy function is rewritten to the canonical form
    Given the source:
      """
      fn  add(a:number,b:number)->number{a+b}
      """
    When I format the document
    Then the source becomes:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """

  # --- chaos: unhappy paths the happy-path suite misses ----------------------

  # The formatter refuses to rewrite source it cannot parse, so a half-typed
  # edit is never mangled — input is returned untouched.
  @chaos
  Scenario: A function with a missing closing brace is left untouched
    Given the source:
      """
      fn add(a: number) -> number { a + b
      """
    When I format the document
    Then the source becomes:
      """
      fn add(a: number) -> number { a + b
      """

  # An unterminated string literal is a syntax error; the document is preserved
  # verbatim rather than reflowed around the broken token.
  @chaos
  Scenario: An unterminated string literal leaves the document untouched
    Given the source:
      """
      fn greet() -> string { "hello
      """
    When I format the document
    Then the source becomes:
      """
      fn greet() -> string { "hello
      """

  # A binary expression cut off after its operator — the most common shape of an
  # in-progress edit — does not parse, so formatting is a no-op.
  @chaos
  Scenario: A dangling binary operator leaves the document untouched
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a +
      }
      """
    When I format the document
    Then the source becomes:
      """
      fn add(a: number, b: number) -> number {
          a +
      }
      """

  # `&` is not a type-position token in neoc (no reference types), so a `&number`
  # parameter is a syntax error and the source is returned unchanged.
  @chaos
  Scenario: A bare ampersand in a type leaves the document untouched
    Given the source:
      """
      fn id(x: &number) -> number { x }
      """
    When I format the document
    Then the source becomes:
      """
      fn id(x: &number) -> number { x }
      """

  # Formatting is idempotent: canonical source already in the house style yields
  # no edit and is returned verbatim.
  @chaos
  Scenario: Already-canonical source is left unchanged
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I format the document
    Then the source becomes:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """

  # A document holding only a comment carries no items; the comment survives the
  # round trip unchanged.
  @chaos
  Scenario: A comment-only document is preserved
    Given the source:
      """
      // a lonely note, no code yet
      """
    When I format the document
    Then the source becomes:
      """
      // a lonely note, no code yet
      """

  # Runs of blank lines between top-level items collapse to a single blank line.
  @chaos
  Scenario: Several blank lines between items collapse to one
    Given the source:
      """
      struct A;




      struct B;
      """
    When I format the document
    Then the source becomes:
      """
      struct A;

      struct B;
      """

  # Tab indentation is rewritten to four spaces per level — neoc's canonical
  # indent mirrors rustfmt.
  @chaos
  Scenario: Tab indentation is rewritten to four spaces
    Given the source:
      """
      fn main() {
      	let x = 1;
      }
      """
    When I format the document
    Then the source becomes:
      """
      fn main() {
          let x = 1;
      }
      """

  # Each nesting level adds exactly four spaces, however deep the braces go.
  @chaos
  Scenario: Deeply nested blocks indent four spaces per level
    Given the source:
      """
      fn f() -> number {if true{if true{1}else{2}}else{3}}
      """
    When I format the document
    Then the source becomes:
      """
      fn f() -> number {
          if true {
              if true {
                  1
              } else {
                  2
              }
          } else {
              3
          }
      }
      """

  # A blank line the author left inside a block is kept, but only one — extra
  # blanks collapse.
  @chaos
  Scenario: Blank lines inside a block collapse to a single blank
    Given the source:
      """
      fn f() -> number {
          let x = 1;




          let y = 2;
          x + y
      }
      """
    When I format the document
    Then the source becomes:
      """
      fn f() -> number {
          let x = 1;

          let y = 2;
          x + y
      }
      """

  # Spec (LEXICAL.md §4) admits UAX #31 identifiers, so a unicode name should
  # canonicalise like any other. The grammar does not yet accept it, so this is
  # the intended-but-unverified outcome — kept @pending until it lands.
  @chaos @pending
  Scenario: A unicode identifier is rewritten to the canonical form
    Given the source:
      """
      fn café(naïve:number)->number{naïve}
      """
    When I format the document
    Then the source becomes:
      """
      fn café(naïve: number) -> number {
          naïve
      }
      """
