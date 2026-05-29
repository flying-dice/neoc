Feature: Call hierarchy

  Call hierarchy answers two questions about a function: which functions call it
  (incoming) and which functions it calls (outgoing). Both resolve by name over
  the file's top-level functions.

  Scenario: Incoming calls list the callers
    Given the source:
      """
      fn target() -> number { 1 }
      fn a() -> number { target() }
      fn b() -> number { target() + target() }
      fn c() -> number { 0 }
      """
    When I find the callers of "target"
    Then the call hierarchy lists:
      | function |
      | a        |
      | b        |

  Scenario: Outgoing calls list the callees, once each
    Given the source:
      """
      fn leaf() -> number { 1 }
      fn helper() -> number { 2 }
      fn root() -> number { leaf() + helper() + leaf() }
      """
    When I find the callees of "root"
    Then the call hierarchy lists:
      | function |
      | leaf     |
      | helper   |

  @chaos
  Scenario: Incoming calls of a function nobody calls are empty
    Given the source:
      """
      fn lonely() -> number { 1 }
      fn other() -> number { 2 }
      """
    When I find the callers of "lonely"
    Then the call hierarchy lists:
      | function |

  @chaos
  Scenario: Outgoing calls of a leaf function are empty
    Given the source:
      """
      fn leaf() -> number { 1 }
      fn caller() -> number { leaf() }
      """
    When I find the callees of "leaf"
    Then the call hierarchy lists:
      | function |

  @chaos
  Scenario: A directly-recursive function is its own incoming caller
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      """
    When I find the callers of "fib"
    Then the call hierarchy lists:
      | function |
      | fib      |

  @chaos
  Scenario: A directly-recursive function is its own outgoing callee
    Given the source:
      """
      fn fib(n: number) -> number { fib(n) }
      """
    When I find the callees of "fib"
    Then the call hierarchy lists:
      | function |
      | fib      |

  @chaos
  Scenario: A caller with two call sites is listed once
    Given the source:
      """
      fn target() -> number { 1 }
      fn twice() -> number { target() + target() }
      """
    When I find the callers of "target"
    Then the call hierarchy lists:
      | function |
      | twice    |

  @chaos
  Scenario: Mutually recursive functions each see the other both ways
    Given the source:
      """
      fn ping() -> number { pong() }
      fn pong() -> number { ping() }
      """
    When I find the callers of "ping"
    Then the call hierarchy lists:
      | function |
      | pong     |

  @chaos
  Scenario: Outgoing calls reach a callee nested inside an if
    Given the source:
      """
      fn leaf() -> number { 1 }
      fn root() -> number { if true { leaf() } else { 0 } }
      """
    When I find the callees of "root"
    Then the call hierarchy lists:
      | function |
      | leaf     |

  @chaos
  Scenario: Outgoing calls drop callees that are not top-level functions
    Given the source:
      """
      struct Point { x: number }
      fn helper() -> number { 1 }
      fn root() -> number { Point(1) + mystery() + helper() }
      """
    When I find the callees of "root"
    Then the call hierarchy lists:
      | function |
      | helper   |

  @chaos
  Scenario: Incoming callers are listed in declaration order, not call order
    Given the source:
      """
      fn t() -> number { 1 }
      fn z() -> number { t() }
      fn a() -> number { t() }
      """
    When I find the callers of "t"
    Then the call hierarchy lists:
      | function |
      | z        |
      | a        |

  @chaos
  Scenario: Callees of a struct are empty — only functions have a call hierarchy
    Given the source:
      """
      struct Point { x: number }
      fn helper() -> number { 1 }
      """
    When I find the callees of "Point"
    Then the call hierarchy lists:
      | function |

  @chaos
  Scenario Outline: A method on an impl has no file-local call hierarchy
    Given the source:
      """
      struct S { x: number }
      impl S { fn m(self) -> number { 1 } }
      fn free() -> number { 2 }
      """
    When I find the <direction> of "m"
    Then the call hierarchy lists:
      | function |

    Examples:
      | direction |
      | callers   |
      | callees   |

  @chaos @pending
  Scenario: A constructor-style call to a struct should not list its caller
    Given the source:
      """
      struct Point { x: number }
      fn make() -> number { Point(1) }
      """
    When I find the callers of "Point"
    Then the call hierarchy lists:
      | function |
