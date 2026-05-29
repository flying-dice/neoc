Feature: Extern struct names

  Backends consult `extern_struct_names` to tell an extern handle apart from a
  native neoc `struct`: a struct-literal of an extern name lowers to a bare
  target object, and methods on it lower directly. The set collects every
  extern `struct` name across an `extern` block, recursively — including those
  nested inside an extern `mod`.

  Scenario: A bare extern struct is collected
    Given the source:
      """
      extern {
          struct Response;
      }
      """
    When I collect extern struct names
    Then the struct names include "Response"

  Scenario: An extern struct with fields is collected
    Given the source:
      """
      extern {
          struct Changes { changes: number }
      }
      """
    When I collect extern struct names
    Then the struct names include "Changes"

  Scenario: A struct nested inside an extern mod is collected
    Given the source:
      """
      extern {
          #[extern(path = "bun:sqlite")]
          mod sqlite {
              struct Database;
              struct Statement;
          }
      }
      """
    When I collect extern struct names
    Then the struct names include "Database"
    And the struct names include "Statement"

  @chaos
  Scenario: A module with no extern block has no extern structs
    Given the source:
      """
      struct Point { x: number, y: number }
      """
    When I collect extern struct names
    Then the struct names exclude "Point"

  @chaos
  Scenario: An extern fn is not an extern struct
    Given the source:
      """
      extern {
          fn now() -> number;
      }
      """
    When I collect extern struct names
    Then the struct names exclude "now"

  @chaos
  Scenario: A native struct alongside an extern block is not collected
    Given the source:
      """
      struct Local { value: number }
      extern {
          struct Foreign;
      }
      """
    When I collect extern struct names
    Then the struct names include "Foreign"
    And the struct names exclude "Local"
