Feature: Workspace symbols

  Workspace symbol search matches declared names — top-level items and their
  members — by case-insensitive substring across the project's open documents.

  Scenario: A substring query finds the matching symbols
    Given the source:
      """
      struct Point { x: number }
      fn paint() {}
      fn main() {}
      """
    When I search workspace symbols for "p"
    Then the workspace symbols are:
      | symbol |
      | Point  |
      | paint  |

  Scenario: A project-wide search reaches declarations in every file
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I search workspace symbols for ""
    Then the workspace symbols are:
      | symbol |
      | add    |
      | run    |

  Scenario: The query filters declarations across files
    Given the file "shapes.neoc":
      """
      pub struct Circle { r: number }
      pub struct Square { s: number }
      """
    And the file "geometry.neoc":
      """
      pub fn circumference(r: number) -> number { r }
      """
    When I search workspace symbols for "circ"
    Then the workspace symbols are:
      | symbol        |
      | Circle        |
      | circumference |

  @chaos
  Scenario: A query matching no declaration yields no symbols
    Given the source:
      """
      struct Point { x: number }
      fn main() {}
      """
    When I search workspace symbols for "zzz"
    Then the workspace symbols are:
      | symbol |

  @chaos
  Scenario: An empty query lists every symbol in declaration order, members inline
    Given the source:
      """
      struct Pair { left: number, right: number }
      fn run() {}
      """
    When I search workspace symbols for ""
    Then the workspace symbols are:
      | symbol |
      | Pair   |
      | left   |
      | right  |
      | run    |

  @chaos
  Scenario Outline: A mixed-case query matches case-insensitively
    Given the source:
      """
      struct Point { x: number }
      fn main() {}
      """
    When I search workspace symbols for "<query>"
    Then the workspace symbols are:
      | symbol |
      | Point  |

    Examples:
      | query |
      | POINT |
      | PoInT |
      | point |
      | oIn   |

  @chaos
  Scenario: A field name is searchable when no top-level item matches
    Given the source:
      """
      struct Circle { radius: number }
      fn main() {}
      """
    When I search workspace symbols for "rad"
    Then the workspace symbols are:
      | symbol |
      | radius |

  @chaos
  Scenario: An enum variant is searchable as a member
    Given the source:
      """
      enum Shape { Triangle, Square }
      fn main() {}
      """
    When I search workspace symbols for "tri"
    Then the workspace symbols are:
      | symbol   |
      | Triangle |

  @chaos
  Scenario: An impl block surfaces as a symbol named after its target type
    Given the source:
      """
      struct Point { x: number }
      impl Point { fn origin() -> Point { Point { x: 0 } } }
      """
    When I search workspace symbols for "impl"
    Then the workspace symbols are:
      | symbol     |
      | impl Point |

  @chaos
  Scenario Outline: A regex-special query is matched literally, not as a pattern
    Given the source:
      """
      struct Point { x: number }
      fn main() {}
      """
    When I search workspace symbols for "<query>"
    Then the workspace symbols are:
      | symbol |

    Examples:
      | query |
      | .     |
      | .*    |
      | P.int |
      | ^P    |
      | x$    |
      | [xP]  |

  @chaos
  Scenario: A whitespace query is not treated as match-all
    Given the source:
      """
      struct Point { x: number }
      fn main() {}
      """
    When I search workspace symbols for " "
    Then the workspace symbols are:
      | symbol |

  @chaos
  Scenario: A project-wide query matching nothing yields no symbols
    Given the file "util.neoc":
      """
      pub fn add(a: number, b: number) -> number { a + b }
      """
    And the file "main.neoc":
      """
      use util::add;
      pub fn run() -> number { add(1, 2) }
      """
    When I search workspace symbols for "zzz"
    Then the workspace symbols are:
      | symbol |

  @chaos
  Scenario: The same name declared in two files surfaces once per file
    Given the file "alpha.neoc":
      """
      pub fn run() -> number { 1 }
      """
    And the file "beta.neoc":
      """
      pub fn run() -> number { 2 }
      """
    When I search workspace symbols for "run"
    Then the workspace symbols are:
      | symbol |
      | run    |
      | run    |

  @chaos
  Scenario: A project-wide search indexes only top-level declarations, not members
    Given the file "shapes.neoc":
      """
      pub struct Point { x: number, y: number }
      """
    When I search workspace symbols for "x"
    Then the workspace symbols are:
      | symbol |

  @chaos
  Scenario: A project-wide empty query lists every top-level declaration, sorted
    Given the file "shapes.neoc":
      """
      pub struct Circle { r: number }
      pub enum Shape { Round }
      """
    And the file "math.neoc":
      """
      pub fn area(r: number) -> number { r }
      """
    When I search workspace symbols for ""
    Then the workspace symbols are:
      | symbol |
      | Circle |
      | Shape  |
      | area   |

  @chaos
  Scenario: An impl block is absent from a project-wide search
    Given the file "point.neoc":
      """
      pub struct Point { x: number }
      impl Point { fn origin() -> Point { Point { x: 0 } } }
      """
    When I search workspace symbols for "impl"
    Then the workspace symbols are:
      | symbol |

  @chaos @pending
  Scenario: A project-wide search finds a method by its name
    Given the file "point.neoc":
      """
      pub struct Point { x: number }
      impl Point { fn origin() -> Point { Point { x: 0 } } }
      """
    When I search workspace symbols for "origin"
    Then the workspace symbols are:
      | symbol |
      | origin |

  @chaos @pending
  Scenario: A non-identifier byte query matches a unicode-named declaration
    Given the source:
      """
      fn café() {}
      fn main() {}
      """
    When I search workspace symbols for "café"
    Then the workspace symbols are:
      | symbol |
      | café   |
