Feature: Diagnostics

  The server reports hard correctness errors from the compiler and warning-level
  lints for valid-but-suspect code. Lints are neoc's clippy-style layer; an
  unused import is the first.

  Scenario: An unused import is a warning
    Given the source:
      """
      use std::fs;
      use std::io;
      fn main() -> number { io::run() }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message            |
      | warning  | unused import `fs` |

  Scenario: A type error is reported
    Given the source:
      """
      fn main() -> number { nope() }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                 |
      | error    | unknown name `nope`     |
