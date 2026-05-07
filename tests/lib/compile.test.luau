local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local compile = require("lib:compile")
local fs      = require("std:fs")

describe("compile", function()

    describe("bundle", function()
        test("bundles an entrypoint into a self-contained executable", function()
            local script = "/tmp/neoc-compile-test-entry.luau"
            local output = "/tmp/neoc-compile-test-out"
            fs.write(script, 'print("bundled-ok")')

            local ok, err = compile.bundle({
                entrypoint = script,
                output     = output,
            })
            expect(err).toBeNil()
            expect(ok).toBeTruthy()

            -- Output should be readable (exists) and larger than the source.
            local content, readErr = fs.read(output)
            expect(readErr).toBeNil()
            expect(#content > 1000).toBeTruthy()
        end)

        test("returns (nil, err) for missing entrypoint", function()
            local ok, err = compile.bundle({
                entrypoint = "/nonexistent-path.luau",
                output     = "/tmp/nope",
            })
            expect(ok).toBeNil()
            expect(err).toContain("compile.bundle:")
        end)
    end)

end)
