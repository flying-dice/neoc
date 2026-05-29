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

  @chaos
  Scenario: Tokens still classify before a syntax error later in the file
    Given the source:
      """
      fn inc(n: number) -> number { n }
      fn broken( {
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type      |
      | 1    | 1      | keyword   |
      | 1    | 4      | function  |
      | 1    | 8      | parameter |
      | 1    | 11     | type      |

  @chaos
  Scenario: A bare keyword survives even when the rest of the item is malformed
    Given the source:
      """
      fn (
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type    |
      | 1    | 1      | keyword |

  @chaos
  Scenario: A boolean literal is coloured as a keyword, not a value
    Given the source:
      """
      const ON: bool = true
      const OFF: bool = false
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type    |
      | 1    | 18     | keyword |
      | 2    | 19     | keyword |

  @chaos
  Scenario: A string literal and a number literal get distinct token types
    Given the source:
      """
      const S: string = "hi"
      const N: number = 42
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type   |
      | 1    | 19     | string |
      | 2    | 19     | number |

  @chaos
  Scenario Outline: Every number literal form is one number token
    Given the source:
      """
      const N: number = <literal>
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type   |
      | 1    | 19     | number |

    Examples:
      | literal |
      | 42      |
      | 0       |
      | 3.14    |
      | 1_000   |

  @chaos
  Scenario Outline: Every string literal form is one string token
    Given the source:
      """
      const S: string = <literal>
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type   |
      | 1    | 19     | string |

    Examples:
      | literal          |
      | ""               |
      | "a\tb"           |
      | "fn struct enum" |

  @chaos
  Scenario: Keywords inside a string literal stay one string, not keyword tokens
    Given the source:
      """
      const S: string = "fn struct enum if else"
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type   |
      | 1    | 19     | string |

  @chaos
  Scenario: Keywords inside a comment stay one comment, and real code after it still classifies
    Given the source:
      """
      // fn struct enum number
      fn f() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 1    | 1      | comment  |
      | 2    | 1      | keyword  |
      | 2    | 4      | function |
      | 2    | 11     | type     |

  @chaos
  Scenario: An identifier whose spelling starts with a keyword is still an identifier
    Given the source:
      """
      fn type_of() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 1    | 1      | keyword  |
      | 1    | 4      | function |

  @chaos
  Scenario: A multi-line block comment is skipped, and the next item classifies on its own line
    Given the source:
      """
      /* line one
         line two */
      fn f() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 3    | 1      | keyword  |
      | 3    | 4      | function |
      | 3    | 11     | type     |

  @chaos
  Scenario: A doc comment is classified as a comment
    Given the source:
      """
      /// docs
      fn f() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 1    | 1      | comment  |
      | 2    | 1      | keyword  |
      | 2    | 4      | function |

  @chaos
  Scenario: A struct field is a property and an enum variant is an enum member
    Given the source:
      """
      struct Point { x: number }
      enum Color { Red }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type       |
      | 1    | 16     | property   |
      | 1    | 19     | type       |
      | 2    | 14     | enumMember |

  @chaos
  Scenario: A unicode identifier shifts following columns by its code-unit width
    Given the source:
      """
      fn café() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 1    | 1      | keyword  |
      | 1    | 4      | function |
      | 1    | 14     | type     |

  @chaos @pending
  Scenario: An astral-plane identifier counts as two columns per character
    Given the source:
      """
      fn 𝔣oo() -> number { 1 }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type     |
      | 1    | 1      | keyword  |
      | 1    | 4      | function |
      | 1    | 15     | type     |

  @chaos @pending
  Scenario: A cast operator is coloured as a keyword
    Given the source:
      """
      fn f() -> number { 1 as number }
      """
    When I request semantic tokens
    Then the semantic tokens include:
      | line | column | type    |
      | 1    | 22     | keyword |
