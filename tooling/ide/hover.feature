Feature: Hover

  The hover handler renders a neoc-flavoured signature for the item under the
  cursor. Every rendered string speaks neoc, never the host language.

  Scenario: Hover on a generic function declaration
    Given the source:
      """
      fn first<T>(a: T, b: T) -> T { a }
      """
    When I hover on "first" at its declaration
    Then the hover shows:
      """
      fn first<T>(a: T, b: T) -> T
      """

  Scenario: Hover on a local binding shows its inferred type
    Given the source:
      """
      fn main() -> number {
        let total = 1 + 2
        total
      }
      """
    When I hover on "total" at the call site
    Then the hover shows:
      """
      total: number
      """

  # Call-site instantiation — resolving T from the argument types — is not
  # implemented yet (see neoc#60 "Out of scope"). @pending keeps this scenario
  # out of the run so CI stays green; drop the tag when the instantiation query
  # lands and the scenario goes live.
  @pending
  Scenario: Hover at a call site instantiates type parameters
    Given the source:
      """
      fn first<T>(a: T, b: T) -> T { a }
      fn main() { println(first(7, 9)); }
      """
    When I hover on "first" at the call site
    Then the hover shows:
      """
      fn first<number>(a: number, b: number) -> number
      """
