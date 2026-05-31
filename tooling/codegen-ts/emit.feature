Feature: Emitting TypeScript

  The TypeScript backend lowers a neoc module to a self-typed `.ts`: the same
  ES2022 runtime the JavaScript backend emits, with TypeScript type annotations
  woven onto its declarations. A `pub fn` keeps its parameter and return types,
  a `pub struct` projects to an `export interface`, a `pub enum` to a
  discriminated-union `type`, and a `use` to a typed, extensionless sibling
  import. Unlike the JS target there is no sibling `.d.ts` — the types are
  inline. `Result`/`Option` import from the shared `./neoc-lib`. The shapes
  asserted here mirror the crate's goldens, `neoc-codegen-js`, and
  `lang/DYNAMIC.md`.

  Scenario: A typed pub fn returning Result carries its types
    Given the source:
      """
      pub fn find(id: number) -> Result<string, string> {
          Ok("x")
      }
      """
    When I emit TypeScript
    Then the TS contains "export function find(id: number): Result<string, string> {"
    And the TS contains "import type { Result, Option } from "./neoc-lib";"
    And the emit reports no problems

  Scenario: A pub struct projects to an exported interface
    Given the source:
      """
      pub struct Row { id: number, body: string }
      """
    When I emit TypeScript
    Then the TS contains "export interface Row { id: number; body: string; }"

  Scenario: A pub enum projects to a discriminated union matching the runtime tags
    Given the source:
      """
      pub enum Shape { Dot, Circle(number), Rect { w: number, h: number } }
      """
    When I emit TypeScript
    Then the TS contains "export type Shape = { __enum: "Shape"; __tag: "Dot" } | { __enum: "Shape"; __tag: "Circle"; _0: number } | { __enum: "Shape"; __tag: "Rect"; w: number; h: number };"

  Scenario: A pub const carries its declared type
    Given the source:
      """
      pub const MAX: number = 10;
      """
    When I emit TypeScript
    Then the TS contains "export const MAX: number = 10;"
