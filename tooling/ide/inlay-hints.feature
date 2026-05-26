Feature: Inlay hints

  Each argument of a free-function call gets a `name:` hint naming the parameter
  it fills, read from the callee's declaration. A hint is omitted when the
  argument is exactly the parameter name.

  Scenario: Call arguments are labelled with their parameter names
    Given the source:
      """
      fn add(a: number, b: number) -> number { a }
      fn main() -> number { add(1, 2) }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |
      | a:   |
      | b:   |

  Scenario: A pass-through argument that already names the parameter is not labelled
    Given the source:
      """
      fn id(x: number) -> number { x }
      fn wrap(x: number) -> number { id(x) }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |
