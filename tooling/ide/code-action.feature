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

  @chaos
  Scenario: Organize imports leaves a file with no imports untouched
    Given the source:
      """
      fn main() -> number { 1 }
      """
    When I organize imports
    Then the source becomes:
      """
      fn main() -> number { 1 }
      """

  @chaos
  Scenario: Organize imports is a no-op on a single import
    Given the source:
      """
      use std::io;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use std::io;
      fn main() {}
      """

  @chaos
  Scenario: Organize imports is idempotent when already sorted and unique
    Given the source:
      """
      use lib::base64;
      use std::fs;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use lib::base64;
      use std::fs;
      fn main() {}
      """

  @chaos
  Scenario: Organize imports leaves an interleaved import run alone
    Given the source:
      """
      use std::io;
      fn a() {}
      use lib::base64;
      """
    When I organize imports
    Then the source becomes:
      """
      use std::io;
      fn a() {}
      use lib::base64;
      """

  @chaos
  Scenario: Organize imports collapses three identical imports to one
    Given the source:
      """
      use std::io;
      use std::io;
      use std::io;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use std::io;
      fn main() {}
      """

  @chaos
  Scenario: Organize imports drops a duplicate while the rest stays sorted
    Given the source:
      """
      use std::fs;
      use std::io;
      use std::io;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use std::fs;
      use std::io;
      fn main() {}
      """

  @chaos
  Scenario: Organize imports swallows a blank line inside the import run
    Given the source:
      """
      use std::io;

      use std::fs;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use std::fs;
      use std::io;
      fn main() {}
      """

  @chaos
  Scenario: Organize imports sorts aliased imports by their full declaration text
    Given the source:
      """
      use std::io as z;
      use std::fs as a;
      fn main() {}
      """
    When I organize imports
    Then the source becomes:
      """
      use std::fs as a;
      use std::io as z;
      fn main() {}
      """

  @chaos
  Scenario: Removing a used import refuses and leaves the source unchanged
    Given the source:
      """
      use std::io;
      fn main() -> number { io::run() }
      """
    When I remove the unused import "io"
    Then the source becomes:
      """
      use std::io;
      fn main() -> number { io::run() }
      """

  @chaos
  Scenario: Removing an import by a name that is not imported changes nothing
    Given the source:
      """
      use std::io;
      fn main() -> number { io::run() }
      """
    When I remove the unused import "collections"
    Then the source becomes:
      """
      use std::io;
      fn main() -> number { io::run() }
      """

  @chaos
  Scenario: Removing one unused import by name leaves another unused import in place
    Given the source:
      """
      use std::fs;
      use std::net;
      fn main() {}
      """
    When I remove the unused import "net"
    Then the source becomes:
      """
      use std::fs;
      fn main() {}
      """
