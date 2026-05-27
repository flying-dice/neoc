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

  Scenario: A dead private declaration is a warning
    Given the source:
      """
      fn helper() -> number { 1 }
      fn main() -> number { 2 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                   |
      | warning  | unused function `helper`  |

  Scenario: A needless return is a warning
    Given the source:
      """
      fn answer() -> number { return 42 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                                            |
      | warning  | unneeded `return`; a function body's last expression is its value |

  Scenario: A type error is reported
    Given the source:
      """
      fn main() -> number { nope() }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message             |
      | error    | unknown name `nope` |
