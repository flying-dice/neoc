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

  Scenario: Remove an unused import deletes its declaration
    Given the source:
      """
      use std::fs;
      use std::io;
      fn main() -> number { io::run() }
      """
    When I remove the unused import "fs"
    Then the source becomes:
      """
      use std::io;
      fn main() -> number { io::run() }
      """
