Feature: Extern imports

  An `extern` block describes a target's surface (`lang/EXTERN.md`). Each
  top-level `mod` inside it lowers to one import a backend must wire:
  `extern_imports` returns them in declaration order. A `mod`'s neoc-side
  binding is its name; `#[extern(path = "...")]` retargets the import specifier,
  and `#[extern(ambient)]` makes it a global-root chain with no import emitted.
  Bare extern `struct`s and `fn`s are reached through their `mod`, never as an
  import of their own.

  Scenario: A plain extern mod imports under its own name
    Given the source:
      """
      extern {
          mod console {
              fn log(message: string);
          }
      }
      """
    When I collect extern imports for target lua
    Then the imports include "console"
    And the import "console" has path "console"

  Scenario: A path attribute retargets the import specifier
    Given the source:
      """
      extern {
          #[extern(path = "vnd:rusqlite")]
          mod rusqlite {
              fn open(path: string) -> Unknown;
          }
      }
      """
    When I collect extern imports for target lua
    Then the imports include "rusqlite"
    And the import "rusqlite" has path "vnd:rusqlite"

  Scenario: An ambient mod carries its global-root path
    Given the source:
      """
      extern {
          #[extern(ambient, path = "globalThis.crypto")]
          mod crypto {
              fn randomUUID() -> string;
          }
      }
      """
    When I collect extern imports for target js
    Then the imports include "crypto"
    And the import "crypto" is ambient
    And the import "crypto" has path "globalThis.crypto"

  Scenario: Several top-level mods import in declaration order
    Given the source:
      """
      extern {
          #[extern(path = "vnd:rusqlite")]
          mod rusqlite {
              fn open(path: string) -> Unknown;
          }
          #[extern(path = "fs")]
          mod files {
              fn read(path: string) -> string;
          }
      }
      """
    When I collect extern imports for target lua
    Then the imports include "rusqlite"
    And the imports include "files"
    And the import "files" has path "fs"

  @chaos
  Scenario: An empty module yields no imports
    Given the source:
      """
      fn main() {}
      """
    When I collect extern imports for target lua
    Then there are no imports

  @chaos
  Scenario: A bare extern struct is not an import of its own
    Given the source:
      """
      extern {
          struct Response;
      }
      """
    When I collect extern imports for target lua
    Then there are no imports
    And the imports exclude "Response"

  @chaos
  Scenario: A bare extern fn is not an import of its own
    Given the source:
      """
      extern {
          fn now() -> number;
      }
      """
    When I collect extern imports for target js
    Then there are no imports
    And the imports exclude "now"

  @chaos
  Scenario: A nested mod is reached through its top-level binding, not surfaced
    Given the source:
      """
      extern {
          #[extern(path = "vnd:rusqlite")]
          mod rusqlite {
              mod statement {
                  fn step() -> Unknown;
              }
          }
      }
      """
    When I collect extern imports for target lua
    Then the imports include "rusqlite"
    And the imports exclude "statement"

  @chaos @pending
  Scenario: Imports vary by target
    # `extern_imports` accepts a `Target` for parity with the other backend
    # queries, but the computed imports are target-agnostic — the same `mod`
    # surfaces identically for lua and js. There is no lua-only import that an
    # equivalent js compilation omits. Pending until per-target import
    # specialisation exists (`lang/EXTERN.md` §Lowering notes the parameter is
    # accepted but does not yet vary the result).
    Given the source:
      """
      extern {
          #[extern(path = "vnd:rusqlite")]
          mod rusqlite {
              fn open(path: string) -> Unknown;
          }
      }
      """
    When I collect extern imports for target js
    Then there are no imports
