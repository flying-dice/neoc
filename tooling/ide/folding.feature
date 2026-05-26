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
