local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local env = require("std:env")

describe("env", function()

    -- Platform constants -------------------------------------------------------

    describe("platform constants", function()
        test("env.os is a non-empty string", function()
            expect(type(env.os)).toBe("string")
            expect(#env.os > 0).toBe(true)
        end)

        test("env.arch is a non-empty string", function()
            expect(type(env.arch)).toBe("string")
            expect(#env.arch > 0).toBe(true)
        end)

        test("env.family is unix or windows", function()
            expect(env.family == "unix" or env.family == "windows").toBe(true)
        end)

        test("env.exe_suffix is consistent with family", function()
            if env.family == "windows" then
                expect(env.exe_suffix).toBe(".exe")
            else
                expect(env.exe_suffix).toBe("")
            end
        end)

        test("env.dll_prefix is consistent with family", function()
            if env.family == "windows" then
                expect(env.dll_prefix).toBe("")
            else
                expect(env.dll_prefix).toBe("lib")
            end
        end)

        test("env.dll_suffix is a non-empty string", function()
            expect(type(env.dll_suffix)).toBe("string")
            expect(#env.dll_suffix > 0).toBe(true)
        end)
    end)

    -- Directory paths ----------------------------------------------------------

    describe("home_dir", function()
        test("returns a string path on typical systems", function()
            local home, err = env.home_dir()
            expect(err).toBeNil()
            expect(type(home)).toBe("string")
            expect(#home > 0).toBe(true)
        end)
    end)

    describe("temp_dir", function()
        test("returns a non-empty string", function()
            local tmp = env.temp_dir()
            expect(type(tmp)).toBe("string")
            expect(#tmp > 0).toBe(true)
        end)
    end)

    describe("current_dir", function()
        test("returns a string path", function()
            local cwd, err = env.current_dir()
            expect(err).toBeNil()
            expect(type(cwd)).toBe("string")
            expect(#cwd > 0).toBe(true)
        end)
    end)

    -- Environment variables ----------------------------------------------------

    describe("var", function()
        test("reads an existing variable", function()
            local val, err = env.var("HOME")
            if env.family == "windows" then
                val, err = env.var("USERPROFILE")
            end
            expect(err).toBeNil()
            expect(type(val)).toBe("string")
        end)

        test("returns error for missing variable", function()
            local val, err = env.var("_NEOC_DEFINITELY_MISSING_XYZ_42")
            expect(val).toBeNil()
            expect(type(err)).toBe("string")
            expect(string.find(err, "env.var: _NEOC_DEFINITELY_MISSING_XYZ_42:")).toBeTruthy()
        end)
    end)

    describe("vars", function()
        test("returns a table of strings", function()
            local all = env.vars()
            expect(type(all)).toBe("table")
            local count = 0
            for k, v in pairs(all) do
                expect(type(k)).toBe("string")
                expect(type(v)).toBe("string")
                count = count + 1
            end
            expect(count > 0).toBe(true)
        end)
    end)

    -- CLI arguments ------------------------------------------------------------

    describe("args", function()
        test("library mode — absent global returns empty table", function()
            local prev = _G._NEOC_ARGS
            _G._NEOC_ARGS = nil
            local a = env.args()
            expect(type(a)).toBe("table")
            expect(#a).toBe(0)
            _G._NEOC_ARGS = prev
        end)

        test("normal path — returns args from global", function()
            local prev = _G._NEOC_ARGS
            _G._NEOC_ARGS = {"hello", "world"}
            local a = env.args()
            expect(#a).toBe(2)
            expect(a[1]).toBe("hello")
            expect(a[2]).toBe("world")
            _G._NEOC_ARGS = prev
        end)

        test("error path — non-table raises", function()
            local prev = _G._NEOC_ARGS
            _G._NEOC_ARGS = 42
            local ok, err = pcall(env.args)
            expect(ok).toBe(false)
            expect(string.find(tostring(err), "env.args: _NEOC_ARGS: expected array of strings")).toBeTruthy()
            _G._NEOC_ARGS = prev
        end)
    end)

end)
