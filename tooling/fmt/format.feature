Feature: Formatting

  The formatter rewrites valid neoc source to its canonical form — two-space
  indent, the opening brace on the same line, spaces around binary operators and
  after `,` / `:`, and exactly one trailing newline. Formatting is idempotent. A
  source with a syntax error is refused, never mangled.

  Scenario: A messy function is rewritten to the canonical form
    Given the source:
      """
      fn  add(a:number,b:number)->number{a+b}
      """
    When I format it
    Then it becomes:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """

  Scenario: Already-canonical source is left unchanged
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I format it
    Then it is left unchanged

  @chaos
  Scenario: A source with a syntax error is refused, not mangled
    Given the source:
      """
      fn add(a: number) -> number { a +
      """
    When I format it
    Then formatting is refused
