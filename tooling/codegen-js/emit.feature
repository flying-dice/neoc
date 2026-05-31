Feature: Emitting JavaScript

  The JavaScript backend lowers a neoc module to an ES2022 module. A `pub` item
  becomes an `export`; a `use` becomes a relative, `.js`-suffixed `import`. neoc
  is expression-oriented and JavaScript statement-oriented, so block-like values
  (an `if`/`else`, a `match`) lower to immediately-invoked arrow functions. The
  shapes asserted here mirror the crate's goldens and `lang/DYNAMIC.md`.

  Scenario: A pub fn becomes an exported function
    Given the source:
      """
      pub fn add(a: number, b: number) -> number {
          a + b
      }
      """
    When I emit JavaScript
    Then the JS contains "export function add(a, b) {"
    And the JS contains "return (a + b);"
    And the emit reports no problems

  Scenario: A module-private fn is not exported
    Given the source:
      """
      fn helper() -> number {
          1
      }
      """
    When I emit JavaScript
    Then the JS contains "function helper() {"
    And the JS does not contain "export"

  Scenario: A use becomes a relative ES-module import
    Given the source:
      """
      use math::add;

      fn main() {
          add(1, 2)
      }
      """
    When I emit JavaScript
    Then the JS contains "function main() {"
    And the JS contains "return add(1, 2);"

  Scenario: A pub const becomes an exported binding
    Given the source:
      """
      pub const PI: number = 3;
      """
    When I emit JavaScript
    Then the JS contains "export const PI = 3;"

  Scenario: An impl method lands on the type's prototype with self as this
    Given the source:
      """
      struct Point {
          x: number,
          y: number,
      }

      impl Point {
          fn sum(self) -> number {
              self.x + self.y
          }
      }
      """
    When I emit JavaScript
    Then the JS contains "const Point$ = {};"
    And the JS contains "Point$.sum = function() {"
    And the JS contains "return (this.x + this.y);"

  Scenario: A named struct literal becomes a tagged object
    Given the source:
      """
      struct Point {
          x: number,
          y: number,
      }

      fn make() -> Point {
          Point { x: 1, y: 2 }
      }
      """
    When I emit JavaScript
    Then the JS contains "{ __struct: "Point", x: 1, y: 2 }"

  Scenario: Ok is imported from the shared neoc-lib runtime
    Given the source:
      """
      fn ok_one() -> Result<number, string> {
          Ok(1)
      }
      """
    When I emit JavaScript
    Then the JS contains "import { Ok, Err, Some, None, Option$, Result$ } from "./neoc-lib.js";"
    And the JS contains "return Ok(1);"
    And the JS does not contain "__enum: "Result", __tag: "Ok""
    And the emit reports no problems

  Scenario: The try operator hoists an early-return guard
    Given the source:
      """
      fn run(r: Result<number, string>) -> Result<number, string> {
          let x = r?;
          Ok(x)
      }
      """
    When I emit JavaScript
    Then the JS contains "__tag === "Err" || "
    And the JS contains "__tag === "None""

  Scenario: An if-else expression lowers to an IIFE
    Given the source:
      """
      fn pick(c: bool) -> number {
          if c { 1 } else { 2 }
      }
      """
    When I emit JavaScript
    Then the JS contains "return (() => {"
    And the JS contains "if (c) {"

  Scenario: A match on an enum dispatches on the tag
    Given the source:
      """
      enum Color {
          Red,
          Green,
      }

      fn pick(c: Color) -> number {
          match c {
              Color::Red => 1,
              Color::Green => 2,
          }
      }
      """
    When I emit JavaScript
    Then the JS contains "__tag === "Red""
    And the JS contains "__tag === "Green""

  Scenario: A derived Debug emits the golden display on the prototype
    Given the source:
      """
      #[derive(Debug)]
      struct Point {
          x: number,
          y: number,
      }
      """
    When I emit JavaScript
    Then the JS contains "Point$.debug = function() {"
    And the JS contains "return "Point { " + "x = " + String(this.x)"

  Scenario: An extern mod becomes a namespace ES-module import
    Given the source:
      """
      extern "js" {
          mod fs {
              fn read(path: string) -> string;
          }
      }

      fn main() {
          fs::read("config")
      }
      """
    When I emit JavaScript
    Then the JS contains "import * as fs from "fs";"
    And the JS contains "fs.read("config")"

  Scenario: A pub struct projects to an exported interface in the .d.ts
    Given the source:
      """
      pub struct Row {
          id: number,
          body: string,
      }
      """
    When I emit JavaScript
    Then the d.ts contains "export interface Row { id: number; body: string; }"

  Scenario: A pub enum projects to a discriminated union matching the runtime tags
    Given the source:
      """
      pub enum Shape {
          Dot,
          Circle(number),
          Rect { w: number, h: number },
      }
      """
    When I emit JavaScript
    Then the d.ts contains "export type Shape = { __enum: "Shape"; __tag: "Dot" } | { __enum: "Shape"; __tag: "Circle"; _0: number } | { __enum: "Shape"; __tag: "Rect"; w: number; h: number };"

  Scenario: A pub fn returning Result projects an export declaration and imports Result
    Given the source:
      """
      pub fn find(id: number) -> Result<string, string> {
          Err("missing")
      }
      """
    When I emit JavaScript
    Then the d.ts contains "import type { Result, Option } from "./neoc-lib";"
    And the d.ts contains "export declare function find(id: number): Result<string, string>;"

  Scenario: A pub const projects an exported declare const with its type
    Given the source:
      """
      pub const MAX: number = 99;
      """
    When I emit JavaScript
    Then the d.ts contains "export declare const MAX: number;"
