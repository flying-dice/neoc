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

  # ---------------------------------------------------------------------------
  # @chaos — unhappy paths. Every asserted message is traced verbatim to
  # neoc-types (infer/) or neoc-lsp/src/lints.rs; types named are neoc types.
  # ---------------------------------------------------------------------------

  @chaos
  Scenario Outline: A call with the wrong number of arguments is rejected
    Given the source:
      """
      <source>
      """
    When I check the document
    Then the diagnostics include:
      | severity | message    |
      | error    | <message>  |

    Examples:
      | source                                                                        | message                                              |
      | fn id(a: number) -> number { a }\nfn main() -> number { id() }                 | this call passes 0 argument(s) but the function takes 1 |
      | fn add(a: number, b: number) -> number { a }\nfn main() -> number { add(1) }   | this call passes 1 argument(s) but the function takes 2 |
      | fn id(a: number) -> number { a }\nfn main() -> number { id(1, 2) }             | this call passes 2 argument(s) but the function takes 1 |

  @chaos
  Scenario: An argument of the wrong type names the expected neoc type
    Given the source:
      """
      fn need(a: number) -> number { a }
      fn main() -> number { need(true) }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                              |
      | error    | argument 1 expects `number`, found `bool` |

  @chaos
  Scenario Outline: `+` rejects operands that are neither two numbers nor two strings
    Given the source:
      """
      fn main() -> number { <expr> }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message    |
      | error    | <message>  |

    Examples:
      | expr        | message                                                          |
      | 1 + true    | `+` expects two `number`s or two `string`s, found `number` and `bool` |
      | "a" + 1     | `+` expects two `number`s or two `string`s, found `string` and `number` |

  @chaos
  Scenario Outline: An arithmetic operator demands `number` operands
    Given the source:
      """
      fn main() -> number { <expr> }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message    |
      | error    | <message>  |

    Examples:
      | expr      | message                                       |
      | true - 1  | arithmetic operand expects `number`, found `bool` |
      | 5 * "a"   | arithmetic operand expects `number`, found `string` |

  @chaos
  Scenario: A logical operator demands `bool` operands
    Given the source:
      """
      fn main() -> bool { 1 && true }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                  |
      | error    | logical operand expects `bool`, found `number` |

  @chaos
  Scenario Outline: Calling a non-function value is rejected
    Given the source:
      """
      <source>
      """
    When I check the document
    Then the diagnostics include:
      | severity | message    |
      | error    | <message>  |

    Examples:
      | source                                          | message                  |
      | fn main() -> number { let x = 1; x() }          | `number` is not callable |
      | fn main() -> number { 5() }                     | `number` is not callable |

  @chaos
  Scenario: A `match` on `bool` that misses an arm is non-exhaustive
    Given the source:
      """
      fn main() -> number { match true { true => 1 } }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                                                              |
      | error    | this match on `bool` is not exhaustive; cover both `true` and `false` or add a `_` arm |

  @chaos
  Scenario: A `match` on an enum lists the missing variants
    Given the source:
      """
      enum Color { Red, Green, Blue }
      fn pick(c: Color) -> number { match c { Color::Red => 1 } }
      fn main() -> number { pick(Color::Red) }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                                              |
      | error    | this match on `Color` is not exhaustive; missing variant(s): Green, Blue |

  @chaos
  Scenario: A `match` arm after a catch-all is unreachable
    Given the source:
      """
      fn main() -> number { match 1 { _ => 0, 1 => 2 } }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                                                             |
      | error    | this match arm is unreachable; an earlier arm already matches every value           |

  @chaos
  Scenario: Comparing two different neoc types is rejected
    Given the source:
      """
      fn main() -> bool { 1 == "a" }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                            |
      | error    | cannot compare `number` with `string` |

  @chaos
  Scenario: A struct literal for an undeclared struct is unknown
    Given the source:
      """
      fn main() -> number { let p = Point { x: 1 }; 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                |
      | error    | unknown struct `Point` |

  @chaos
  Scenario: A struct literal missing a field is reported
    Given the source:
      """
      struct P { x: number, y: number }
      fn main() -> number { let p = P { x: 1 }; 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message               |
      | error    | missing field `y` in `P` |

  @chaos
  Scenario: Accessing a field a struct does not have is rejected
    Given the source:
      """
      struct P { x: number }
      fn main() -> number { let p = P { x: 1 }; p.y }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                  |
      | error    | struct `P` has no field `y` |

  @chaos
  Scenario: Calling a method a struct does not have is rejected
    Given the source:
      """
      struct P {}
      fn main() -> number { let p = P {}; p.nope() }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                  |
      | error    | type `P` has no method `nope` |

  @chaos
  Scenario: Referring to an enum variant that does not exist is rejected
    Given the source:
      """
      enum Color { Red }
      fn main() -> Color { Color::Blue }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                          |
      | error    | enum `Color` has no variant `Blue` |

  @chaos
  Scenario: A function body whose value is the wrong neoc type is rejected
    Given the source:
      """
      fn main() -> number { "hello" }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                  |
      | error    | function body expects `number`, found `string` |

  @chaos
  Scenario: A list with disagreeing element types is rejected
    Given the source:
      """
      fn main() -> number { let xs = [1, true]; 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                               |
      | error    | list elements disagree: `number` vs `bool` |

  @chaos
  Scenario: Iterating something that is not a list is rejected
    Given the source:
      """
      fn main() -> number { for x in 5 { } 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                       |
      | error    | `for` expects a list to iterate, found `number` |

  @chaos
  Scenario: Indexing into a non-list value is rejected
    Given the source:
      """
      fn main() -> number { let x = 5; x[0] }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                  |
      | error    | cannot index into `number` |

  @chaos
  Scenario Outline: Every unused import is its own warning
    Given the source:
      """
      use std::fs;
      use std::io;
      fn main() -> number { 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message    |
      | warning  | <message>  |

    Examples:
      | message            |
      | unused import `fs` |
      | unused import `io` |

  # --- @pending: provoked but unverifiable / unimplemented in single-file diagnose ---

  # The single-file checker reports no diagnostic for a second `fn dup`: name
  # collection keeps the last definition silently. The intended behaviour is a
  # "duplicate definition" error; no such message exists yet, so this stays
  # pending until duplicate-definition checking lands.
  @chaos @pending
  Scenario: A duplicate top-level definition is an error
    Given the source:
      """
      fn dup() -> number { 1 }
      fn dup() -> number { 2 }
      fn main() -> number { dup() }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                       |
      | error    | duplicate definition of `dup` |

  # An unknown type *name* in an annotation is not flagged as such — it is
  # treated as an opaque type and only surfaces as a downstream mismatch
  # (`function body expects `Widget`…`). The intended behaviour is to name the
  # unknown type directly. Pending an unknown-type-name check.
  @chaos @pending
  Scenario: An unknown type name in a return annotation is reported
    Given the source:
      """
      fn main() -> Widget { 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message               |
      | error    | unknown type `Widget` |

  # A refutable pattern in `let` is rejected, but today's message leaks a
  # host-language debug form of the pattern. The intended neoc-shaped message
  # names the construct, not its internal representation. Pending the message
  # rewrite.
  @chaos @pending
  Scenario: A refutable pattern in `let` is rejected in neoc terms
    Given the source:
      """
      enum E { A, B }
      fn main() -> number { let E::A = E::A; 0 }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                                       |
      | error    | a `let` binding needs an irrefutable pattern  |

  # Reassigning an immutable binding should be an error; there is no `mut`
  # binding form or reassignment check in single-file diagnose yet, so the
  # outcome cannot be asserted. Pending the mutability model.
  @chaos @pending
  Scenario: Reassigning an immutable binding is an error
    Given the source:
      """
      fn main() -> number { let x = 1; x = 2; x }
      """
    When I check the document
    Then the diagnostics include:
      | severity | message                              |
      | error    | cannot assign to immutable binding `x` |
