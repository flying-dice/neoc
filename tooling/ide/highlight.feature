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
