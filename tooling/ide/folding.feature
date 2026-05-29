Feature: Folding ranges

  Brace-delimited bodies fold — functions, structs, enums, traits, impls,
  `match` blocks — as do multi-line block comments. A single-line body does not
  fold. Ranges are 1-based (start, end) lines.

  Scenario: Struct and function bodies each fold
    Given the source:
      """
      struct Point {
        x: number,
        y: number,
      }
      fn origin() -> Point {
        Point { x: 0, y: 0 }
      }
      """
    When I request folds
    Then the folds are:
      | start | end |
      | 1     | 4   |
      | 5     | 7   |

  @chaos
  Scenario: An empty single-line body does not fold
    Given the source:
      """
      struct Empty {}
      fn nothing() {}
      """
    When I request folds
    Then the folds are:
      | start | end |

  @chaos
  Scenario: An empty body still folds when its braces straddle two lines
    Given the source:
      """
      struct Empty {
      }
      """
    When I request folds
    Then the folds are:
      | start | end |
      | 1     | 2   |

  @chaos
  Scenario: A single-line function between multi-line bodies is skipped
    Given the source:
      """
      fn id(n: number) -> number { n }
      struct Pair {
        a: number,
        b: number,
      }
      """
    When I request folds
    Then the folds are:
      | start | end |
      | 2     | 5   |

  @chaos
  Scenario: An impl wrapping a method body yields nested folds
    Given the source:
      """
      struct Point {
        x: number,
      }
      impl Point {
        fn x(self) -> number {
          self.x
        }
      }
      """
    When I request folds
    Then the folds are:
      | start | end |
      | 1     | 3   |
      | 4     | 8   |
      | 5     | 7   |

  @chaos
  Scenario: A match nested inside a match folds at every level
    Given the source:
      """
      fn classify(n: number) -> number {
        match n {
          0 => match n {
            _ => 1,
          },
          _ => 2,
        }
      }
      """
    When I request folds
    Then the folds are:
      | start | end |
      | 1     | 8   |
      | 2     | 7   |
      | 3     | 5   |

  @chaos
  Scenario: An unterminated block comment swallows the rest and folds nothing
    Given the source:
      """
      /*
        a note that never closes
      fn main() {}
      """
    When I request folds
    Then the folds are:
      | start | end |

  @chaos
  Scenario: Consecutive line comments do not fold
    Given the source:
      """
      // note one
      // note two
      fn main() {}
      """
    When I request folds
    Then the folds are:
      | start | end |
