Feature: CLI

  The `neoc` binary runs Lua. `run` executes one or more `.lua` scripts (globs
  expand) or an inline `--eval` snippet in the runtime (unsafe Lua globals
  stripped, only registered modules load; not a security boundary). A clean
  script exits 0; a script that faults, a path that matches nothing, a `.neoc`
  source file or an unknown subcommand exits non-zero.

  Scenario: run executes a Lua script
    Given a file "hello.lua" with:
      """
      print("hello from lua")
      """
    When I run "neoc run hello.lua"
    Then it exits successfully
    And stdout contains "hello from lua"

  Scenario: run --eval executes an inline snippet
    When I run "neoc run -e print(1 + 2)"
    Then it exits successfully
    And stdout contains "3"

  Scenario: run reports a passing test file
    Given a file "ok.test.lua" with:
      """
      local t = require("lib:test")
      t.test("adds", function()
        t.expect(1 + 2).toBe(3)
      end)
      """
    When I run "neoc run ok.test.lua"
    Then it exits successfully
    And stdout contains "1 passed"

  @chaos
  Scenario: run reports a failing test file and exits non-zero
    Given a file "bad.test.lua" with:
      """
      local t = require("lib:test")
      t.test("fails", function()
        t.expect(1).toBe(2)
      end)
      """
    When I run "neoc run bad.test.lua"
    Then it exits with an error
    And stdout contains "1 failed"

  @chaos
  Scenario: a script fault names the script
    Given a file "boom.lua" with:
      """
      local n = explode()
      """
    When I run "neoc run boom.lua"
    Then it exits with an error
    And stderr contains "boom.lua"

  @chaos
  Scenario: a .neoc path is refused with a plain message
    Given a file "old.neoc" with:
      """
      fn main() {}
      """
    When I run "neoc run old.neoc"
    Then it exits with an error
    And stderr contains "runs Lua (`.lua`) scripts only"

  @chaos
  Scenario: a glob that matches nothing is reported, not silently ignored
    When I run "neoc run no-such-*.lua"
    Then it exits with an error
    And stderr contains "no matches for pattern"

  @chaos
  Scenario: a removed compiler subcommand is refused
    When I run "neoc build main.neoc"
    Then it exits with an error

  @chaos
  Scenario: an unknown subcommand is refused
    When I run "neoc frobnicate"
    Then it exits with an error
