Feature: Code actions

  Organize-imports sorts the document's `use` declarations and drops exact
  duplicates, rewriting the contiguous import run in place.

  Scenario: Organize imports sorts and de-duplicates the use declarations
    Given the source:
      """
      use std::io;
      use lib::base64;
      use std::io;
      use std::fs;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use lib::base64;
      use std::fs;
      use std::io;
      fn main() {}
      """
