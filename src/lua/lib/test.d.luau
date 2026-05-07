--!strict
--
-- Type definitions for `lib:test` — paired with `src/lua/lib/test.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Usage
--
-- ```lua
-- local test = require("lib:test")
--
-- test.describe("my suite", function()
--     test.it("adds numbers", function()
--         test.expect(1 + 1).toBe(2)
--     end)
-- end)
-- ```
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Matcher returned by `expect(value)`. Holds the subject value and exposes
    assertion methods using dot syntax (not `:` — no `self` parameter).

    ```lua
    test.expect(result).toBe("aGk=")
    test.expect(err).toBeNil()
    ```
]=]
export type Matcher = {
    --[=[
        Passes if `actual == expected` (strict Lua `==`).
        Errors with a descriptive message on mismatch.
    ]=]
    toBe: (expected: any) -> (),

    --[=[
        Passes if the subject is `nil`. Errors otherwise.
    ]=]
    toBeNil: () -> (),

    --[=[
        Passes if the subject is truthy (anything other than `nil` or `false`).
    ]=]
    toBeTruthy: () -> (),

    --[=[
        Passes if the subject is falsy (`nil` or `false`).
    ]=]
    toBeFalsy: () -> (),

    --[=[
        For strings: passes if `item` is a substring of the subject.
        For tables: passes if `item` appears as a value anywhere in the table.
        Errors for other subject types.
    ]=]
    toContain: (item: any) -> (),

    --[=[
        Passes if the subject is a function that raises an error when called
        with no arguments. Errors if the function returns normally.
    ]=]
    toThrow: () -> (),
}

--[=[
    The module table returned by `require("lib:test")`.
]=]
export type TestModule = {
    --[=[
        Declare a named test suite. The callback may be async (it can `await`
        inside). Suites nest — `describe` inside `describe` prefixes the test
        name.

        ```lua
        test.describe("math", function()
            test.it("adds", function() ... end)
        end)
        ```
    ]=]
    describe: (name: string, fn: () -> ()) -> (),

    --[=[
        Declare a named test case. The callback may be async. Alias: `it`.

        ```lua
        test.test("it returns 42", function()
            test.expect(compute()).toBe(42)
        end)
        ```
    ]=]
    test: (name: string, fn: () -> ()) -> (),

    --[=[
        Alias for `test`. Prefer this inside a `describe` block for
        BDD-style readability.

        ```lua
        test.describe("adder", function()
            test.it("returns the sum", function() ... end)
        end)
        ```
    ]=]
    it: (name: string, fn: () -> ()) -> (),

    --[=[
        Wrap `value` in a `Matcher` so you can chain assertion methods.

        ```lua
        test.expect(result).toBe("expected")
        test.expect(err).toBeNil()
        test.expect(fn).toThrow()
        ```
    ]=]
    expect: (value: any) -> Matcher,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local test = require("lib:test")` resolves `test` to `TestModule`.
declare function require(name: "lib:test"): TestModule
