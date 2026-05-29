Feature: Inlay hints

  Each argument of a free-function call gets a `name:` hint naming the parameter
  it fills, read from the callee's declaration. A hint is omitted when the
  argument is exactly the parameter name. Each `let` binding without a written
  type gets a `: type` hint after the name, from inference.

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

  Scenario: An unannotated let binding shows its inferred type
    Given the source:
      """
      fn main() -> number {
        let total = 1 + 2
        total
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint     |
      | : number |

  @chaos
  Scenario: A let with a written type gets no type hint, but its inferred neighbour still does
    Given the source:
      """
      fn main() -> number {
        let typed: number = 1
        let inferred = 2
        typed + inferred
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint     |
      | : number |

  @chaos
  Scenario: A string binding is hinted with the neoc type, not the host type
    Given the source:
      """
      fn main() -> string {
        let greeting = "hi"
        greeting
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint     |
      | : string |

  @chaos
  Scenario: A bool binding is hinted as bool
    Given the source:
      """
      fn main() -> bool {
        let ready = true
        ready
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint   |
      | : bool |

  @chaos
  Scenario: A binding of a user struct is hinted with the struct name
    Given the source:
      """
      struct Point { x: number, y: number }
      fn main() -> number {
        let here = Point { x: 1, y: 2 }
        0
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint    |
      | : Point |

  @chaos
  Scenario: A chain of inferred lets each show their type, in document order
    Given the source:
      """
      fn main() -> number {
        let a = 1
        let b = a + 1
        let c = b + 1
        c
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint     |
      | : number |
      | : number |
      | : number |

  @chaos
  Scenario: A let whose value fails to type-check gets no hint rather than a bogus one
    Given the source:
      """
      fn main() -> number {
        let bad = 1 + true
        0
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |

  @chaos
  Scenario: A let bound to a call of an undeclared function gets no hint
    Given the source:
      """
      fn main() -> number {
        let x = nope()
        0
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |

  @chaos
  Scenario: A call to an undeclared function labels none of its arguments
    Given the source:
      """
      fn main() -> number { nope(1, 2) }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |

  @chaos
  Scenario: Passing more arguments than parameters labels only the declared ones
    Given the source:
      """
      fn one(a: number) -> number { a }
      fn main() -> number { one(1, 2) }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |
      | a:   |

  @chaos
  Scenario: A method-call argument is not labelled, but the receiver let still gets its type
    Given the source:
      """
      struct C {}
      impl C { fn go(self, n: number) -> number { n } }
      fn main() -> number {
        let c = C {}
        c.go(5)
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |
      | : C  |

  @chaos
  Scenario: A nested call labels the arguments of both the inner and outer call
    Given the source:
      """
      fn inc(n: number) -> number { n }
      fn main() -> number { inc(inc(1)) }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint |
      | n:   |
      | n:   |

  @chaos
  Scenario: A list binding is hinted with the inferred list type
    Given the source:
      """
      fn main() -> number {
        let xs = [1, 2, 3]
        0
      }
      """
    When I request inlay hints
    Then the inlay hints are:
      | hint      |
      | : [number] |
