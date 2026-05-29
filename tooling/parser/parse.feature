Feature: Parsing

  The parser turns neoc source into a typed AST. Well-formed source parses
  cleanly — no diagnostics. Comments and whitespace carry no meaning of their
  own, and an empty file is a (trivially) well-formed module. Malformed source
  is reported, never silently accepted (see chaos.feature).

  Scenario: A function definition parses cleanly
    Given the source:
      """
      fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I parse it
    Then it parses cleanly

  Scenario: An extern block declaring a host function parses cleanly
    Given the source:
      """
      extern {
          fn println(s: string);
      }
      """
    When I parse it
    Then it parses cleanly

  Scenario: A struct definition parses cleanly
    Given the source:
      """
      struct Point {
          x: number,
          y: number,
      }
      """
    When I parse it
    Then it parses cleanly

  Scenario: A let binding over a match parses cleanly
    Given the source:
      """
      fn main() {
          let s = Some(42);
          let v = match s {
              Some(n) => n,
              None => 0,
          };
      }
      """
    When I parse it
    Then it parses cleanly

  Scenario: Deeply nested blocks parse cleanly
    Given the source:
      """
      fn f() {
          if true {
              if true {
                  if true {
                      if true {
                          let x = 1;
                      }
                  }
              }
          }
      }
      """
    When I parse it
    Then it parses cleanly

  Scenario: A source that is only a comment parses cleanly
    Given the source:
      """
      // a lone comment is a well-formed, empty module
      """
    When I parse it
    Then it parses cleanly

  Scenario: An empty source is a well-formed empty module
    Given an empty source
    When I parse it
    Then it parses cleanly
