Feature: Extern constructors

  An extern `impl` method marked `#[extern(constructor)]` is a target
  constructor: a call `T::new(args)` lowers to the target's constructor form
  rather than a static call. `extern_constructors` returns each such pair as
  `(type, method)` — the impl's target type and the constructor `fn`'s name.
  Methods without the attribute are ordinary associated calls and are excluded.

  Scenario: A constructor-tagged method is collected as a pair
    Given the source:
      """
      extern {
          struct Database;
          impl Database {
              #[extern(constructor)] fn new(filename: string);
              fn query(self, sql: string) -> Unknown;
          }
      }
      """
    When I collect extern constructors
    Then the constructors include "Database::new"

  Scenario: An untagged method is not a constructor
    Given the source:
      """
      extern {
          struct Database;
          impl Database {
              #[extern(constructor)] fn new(filename: string);
              fn query(self, sql: string) -> Unknown;
          }
      }
      """
    When I collect extern constructors
    Then the constructors include "Database::new"
    And the constructors exclude "Database::query"

  Scenario: A constructor inside an extern mod is collected
    Given the source:
      """
      extern {
          #[extern(path = "bun:sqlite")]
          mod sqlite {
              struct Database;
              impl Database {
                  #[extern(constructor)] fn open(filename: string);
              }
          }
      }
      """
    When I collect extern constructors
    Then the constructors include "Database::open"

  @chaos
  Scenario: A module with no extern impls has no constructors
    Given the source:
      """
      struct Point { x: number }
      impl Point {
          fn new(x: number) -> Point { Point { x: x } }
      }
      """
    When I collect extern constructors
    Then the constructors exclude "Point::new"

  @chaos
  Scenario: An extern impl with no tagged method yields no constructor
    Given the source:
      """
      extern {
          struct Changes { changes: number }
          impl Changes {
              fn total(self) -> number;
          }
      }
      """
    When I collect extern constructors
    Then the constructors exclude "Changes::total"

  @chaos
  Scenario: A malformed extern item makes the source refuse to parse cleanly
    Given the source:
      """
      extern {
          struct Database
          impl Database {
              #[extern(constructor)] fn new(filename: string);
          }
      }
      """
    Then the source did not parse cleanly
