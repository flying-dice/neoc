Feature: Semantic tokens

  Semantic tokens classify the document for type-aware highlighting: keywords,
  function and type names, parameters, fields, enum variants, literals, and
  comments. Locations are 1-based (line, column).

  Scenario: A function declaration is classified by role
    Given the source:
      """
      fn inc(n: number) -> number { n }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type      |
      | 1    | 1      | keyword   |
      | 1    | 4      | function  |
      | 1    | 8      | parameter |
      | 1    | 11     | type      |

  Scenario: Literals and comments are classified
    Given the source:
      """
      // a note
      const N: number = 42
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type    |
      | 1    | 1      | comment |
      | 2    | 19     | number  |
