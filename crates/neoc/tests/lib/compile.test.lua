local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local compile = require("lib:compile")
local env     = require("std:env")
local fs      = require("std:fs")

-- Per-run unique paths under the workspace `.tmp/` (gitignored), not the
-- system `/tmp`. Bundle output is the entire neoc binary (~230 MB); on
-- CI runners `/tmp` is a small tmpfs and back-to-back runs
-- surface as `No space left on device`. The workspace volume is per-job
-- and sized for build artefacts.
local function unique_path(suffix)
    local dir = env.current_dir() .. "/.tmp"
    fs.create_dir_all(dir)
    local nonce = string.format("%x", math.random(0, 2 ^ 30))
    return dir .. "/compile-test-" .. nonce .. "-" .. suffix
end

describe("compile", function()

    describe("bundle", function()
        test("bundles an entrypoint into a self-contained executable", function()
            local script = unique_path("entry.lua")
            local output = unique_path("out")
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
                entrypoint = "/nonexistent-path.lua",
                output     = unique_path("nope"),
            })
            expect(ok).toBeNil()
            expect(err).toContain("compile.bundle:")
        end)
    end)

end)
