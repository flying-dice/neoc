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
