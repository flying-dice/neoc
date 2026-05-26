Feature: Workspace symbols

  Workspace symbol search matches declared names — top-level items and their
  members — by case-insensitive substring across the project's open documents.

  Scenario: A substring query finds the matching symbols
    Given the source:
      """
      struct Point { x: number }
      fn paint() {}
      fn main() {}
      """
    When I search workspace symbols for "p"
    Then the workspace symbols are:
      | symbol |
      | Point  |
      | paint  |
