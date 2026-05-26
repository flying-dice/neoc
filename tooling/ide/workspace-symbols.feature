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

  Scenario: A project-wide search reaches declarations in every file
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I search workspace symbols for ""
    Then the workspace symbols are:
      | symbol |
      | add    |
      | run    |

  Scenario: The query filters declarations across files
    Given the file "shapes.neoc":
      """
      pub struct Circle { r: number }
      pub struct Square { s: number }
      """
    And the file "geometry.neoc":
      """
      pub fn circumference(r: number) -> number { r }
      """
    When I search workspace symbols for "circ"
    Then the workspace symbols are:
      | symbol        |
      | Circle        |
      | circumference |
