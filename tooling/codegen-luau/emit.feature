Feature: Emitting Luau

  The Luau backend lowers a neoc module to a self-typed `.luau`: the same Lua 5.1
  runtime the Lua backend emits, with native inline Luau type annotations woven
  onto its declarations. A `pub fn` keeps its parameter and return types, a
  `struct` projects to a `type` over a tagged table, an `enum` to a union `type`,
  and a `use` to a `require` binding. Unlike the `.d.lua` sidecar the Lua backend
  emits, the types here are first-class inline Luau — the file is self-typed, no
  sidecar. `Result`/`Option` are declared as inline `type` aliases whose union
  members mirror the runtime tagged tables. The shapes asserted here mirror the
  crate's goldens, `neoc-codegen-lua`, and `lang/DYNAMIC.md`.

  Scenario: A typed pub fn returning Result carries its types
    Given the source:
      """
      pub fn find(id: number) -> Result<string, string> {
          Ok("x")
      }
      """
    When I emit Luau
    Then the Luau contains "function find(id: number): Result<string, string>"
    And the Luau contains "type Result<T, E> = { __enum: "Result", __tag: "Ok", [1]: T } | { __enum: "Result", __tag: "Err", [1]: E }"
    And the emit reports no problems

  Scenario: A struct projects to a type over a tagged table
    Given the source:
      """
      pub struct Row { x: number }
      """
    When I emit Luau
    Then the Luau contains "type Row = { __struct: "Row", x: number }"

  Scenario: An enum projects to a union matching the runtime tags
    Given the source:
      """
      pub enum Shape { Circle(number), Square { side: number } }
      """
    When I emit Luau
    Then the Luau contains "type Shape = { __enum: "Shape", __tag: "Circle", [1]: number } | { __enum: "Shape", __tag: "Square", side: number }"

  Scenario: A use lowers to a require binding
    Given the source:
      """
      use math::add;
      pub fn f() -> number { add(1, 2) }
      """
    When I emit Luau
    Then the Luau contains "local add = require("math").add"
    And the Luau contains "function f(): number"
