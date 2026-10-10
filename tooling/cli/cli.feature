Feature: CLI

  The `neoc` binary drives both halves of the toolchain. `check` parses and
  type-checks without emitting; `build` transpiles a `.neoc` entry to Lua (the
  default target) next to the source; `run --eval` transpiles and executes an
  inline snippet
  in-process. Diagnostics speak neoc — `<path>: <stage>: <message>`, where the
  stage is `parse`, `type`, or `emit` — never the host language. A clean program
  exits 0; anything that fails to compile, a path that matches nothing, or an
  unknown subcommand exits non-zero.

  Scenario: check accepts a well-formed program
    Given a file "ok.neoc" with:
      """
      extern { fn println(n: number); }
      fn main() {
        println(1 + 2);
      }
      """
    When I run "neoc check ok.neoc"
    Then it exits successfully

  Scenario: check writes no output next to the source
    Given a file "ok.neoc" with:
      """
      extern { fn println(n: number); }
      fn main() {
        println(1 + 2);
      }
      """
    When I run "neoc check ok.neoc"
    Then it exits successfully
    And no file "ok.lua" exists

  Scenario: build transpiles a clean program to Lua next to the source
    Given a file "foo.neoc" with:
      """
      extern { fn println(n: number); }
      fn main() {
        println(1 + 2);
      }
      """
    When I run "neoc build foo.neoc"
    Then it exits successfully
    And a non-empty file "foo.lua" exists

  Scenario: run --eval executes an inline snippet
    When I run "neoc run -e fn main() -> number { 1 + 2 }"
    Then it exits successfully

  @chaos
  Scenario: check rejects a type error with a neoc diagnostic
    Given a file "bad.neoc" with:
      """
      fn main() {
        let x: string = 42;
      }
      """
    When I run "neoc check bad.neoc"
    Then it exits with an error
    And stderr contains "bad.neoc"
    And stderr contains "type:"

  @chaos
  Scenario: check rejects a parse error and names the file
    Given a file "broken.neoc" with:
      """
      fn main( {
      """
    When I run "neoc check broken.neoc"
    Then it exits with an error
    And stderr contains "broken.neoc"

  @chaos
  Scenario: build skips emitting Lua when the program has a diagnostic
    Given a file "bad.neoc" with:
      """
      fn main( {
      """
    When I run "neoc build bad.neoc"
    Then it exits with an error
    And no file "bad.lua" exists

  @chaos
  Scenario: a glob that matches nothing is reported, not silently ignored
    When I run "neoc check no-such-*.neoc"
    Then it exits with an error
    And stderr contains "no files matched"

  @chaos
  Scenario: an unknown subcommand is refused
    When I run "neoc frobnicate"
    Then it exits with an error

  @chaos
  Scenario: run --eval surfaces a type error against the eval chunk
    When I run "neoc run --eval fn main() { let x: string = 42; }"
    Then it exits with an error
    And stderr contains "<eval>: type:"
