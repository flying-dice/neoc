local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local base64 = require("lib:base64")
local fs = require("std:fs")
local vcr = require("lib:vcr")

fs.create_dir_all(".tmp")

describe("vcr", function()

    -- record -------------------------------------------------------------------

    describe("record", function()
        test("wrapped infallible function returns correct result", function()
            local vcr = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            expect(vcr:wrap("base64.encode.hello", base64.encode)("hello")).toBe("aGVsbG8=")
        end)

        test("wrapped fallible function returns correct result", function()
            local vcr = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            local result, err = vcr:wrap("base64.decode.hello", base64.decode)("aGVsbG8=")
            expect(err).toBeNil()
            expect(result).toBe("hello")
        end)

        test("wrapped fallible function preserves error tuple", function()
            local vcr = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            local result, err = vcr:wrap("base64.decode.invalid", base64.decode)("not*valid!")
            expect(result).toBeNil()
            expect(err).toContain("base64.decode:")
        end)

        test("duplicate key raises an error", function()
            local vcr = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            vcr:wrap("base64.encode.dupe", base64.encode)
            expect(function()
                vcr:wrap("base64.encode.dupe", base64.encode)
            end).toThrow()
        end)

        test("same key in different vcr instances is allowed", function()
            local vcr1 = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            local vcr2 = vcr.cassette({ path = ".tmp/vcr_unused.json", mode = "record" })
            vcr1:wrap("base64.encode.key", base64.encode)
            vcr2:wrap("base64.encode.key", base64.encode)
        end)

        test("writes to disk on each call", function()
            local vcr = vcr.cassette({ path = ".tmp/vcr_record_test.json", mode = "record" })
            vcr:wrap("base64.encode.hello", base64.encode)("hello")
        end)

        test("each instance has its own tape", function()
            local vcr1 = vcr.cassette({ path = ".tmp/vcr_tape1.json", mode = "record" })
            local vcr2 = vcr.cassette({ path = ".tmp/vcr_tape2.json", mode = "record" })
            vcr1:wrap("base64.encode.key", base64.encode)("only in vcr1")
            vcr2:wrap("base64.encode.key", base64.encode)("only in vcr2")
        end)
    end)

    -- replay -------------------------------------------------------------------

    describe("replay", function()
        test("returns recorded result without calling real function", function()
            local rec = vcr.cassette({ path = ".tmp/vcr_replay_test.json", mode = "record" })
            rec:wrap("base64.encode.hello", base64.encode)("hello")

            local rep = vcr.cassette({ path = ".tmp/vcr_replay_test.json", mode = "replay" })
            expect(rep:wrap("base64.encode.hello", base64.encode)("ignored")).toBe("aGVsbG8=")
        end)

        test("preserves multi-value result", function()
            local rec = vcr.cassette({ path = ".tmp/vcr_replay_mv_test.json", mode = "record" })
            rec:wrap("base64.decode.hello", base64.decode)("aGVsbG8=")

            local rep = vcr.cassette({ path = ".tmp/vcr_replay_mv_test.json", mode = "replay" })
            local result, err = rep:wrap("base64.decode.hello", base64.decode)("ignored")
            expect(err).toBeNil()
            expect(result).toBe("hello")
        end)

        test("duplicate key raises an error", function()
            local rec = vcr.cassette({ path = ".tmp/vcr_replay_dupe_test.json", mode = "record" })
            rec:wrap("base64.encode.key", base64.encode)("x")

            local rep = vcr.cassette({ path = ".tmp/vcr_replay_dupe_test.json", mode = "replay" })
            rep:wrap("base64.encode.key", base64.encode)
            expect(function()
                rep:wrap("base64.encode.key", base64.encode)
            end).toThrow()
        end)

        test("missing key raises an error", function()
            local rec = vcr.cassette({ path = ".tmp/vcr_replay_missing_test.json", mode = "record" })

            local rep = vcr.cassette({ path = ".tmp/vcr_replay_missing_test.json", mode = "replay" })
            expect(function()
                rep:wrap("base64.encode.missing", base64.encode)
            end).toThrow()
        end)
    end)

    -- off ----------------------------------------------------------------------

    describe("off", function()
        test("wrap returns the original function unchanged", function()
            local vcr = vcr.cassette({ mode = "off" })
            expect(vcr:wrap("base64.encode.hello", base64.encode)("hello")).toBe("aGVsbG8=")
        end)

        test("no duplicate key check", function()
            local vcr = vcr.cassette({ mode = "off" })
            vcr:wrap("base64.encode.dupe", base64.encode)
            vcr:wrap("base64.encode.dupe", base64.encode)
        end)

        test("dump is a no-op", function()
            local vcr = vcr.cassette({ mode = "off" })
            vcr:dump()
        end)
    end)

    -- a buffer (refused, never recorded as an array of numbers) ----------------

    describe("a buffer", function()
        local refused = "a buffer cannot be serialized; convert it explicitly first"

        -- a buffer 100 tables down (the guard walks as deep as a serializer, 128 tables)
        local function deep()
            local t = { buffer.create(2) }
            for _ = 2, 100 do
                t = { inner = t }
            end
            return t
        end

        test("refuses one 100 tables down", function()
            local tape = vcr.cassette({ path = ".tmp/vcr_buffer_deep.json", mode = "record" })
            local echo = tape:wrap("echo", function(x)
                return x
            end)
            local ok, err = pcall(echo, deep())
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("vcr.record: " .. refused)
        end)

        test("record refuses one in the arguments before the function runs", function()
            local tape = vcr.cassette({ path = ".tmp/vcr_buffer_args.json", mode = "record" })
            local called = false
            local echo = tape:wrap("echo", function(x)
                called = true
                return x
            end)
            local ok, err = pcall(echo, buffer.create(2))
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("vcr.record: " .. refused)
            expect(called).toBe(false)
        end)

        test("record refuses one in the result", function()
            local tape = vcr.cassette({ path = ".tmp/vcr_buffer_result.json", mode = "record" })
            local give = tape:wrap("give", function()
                return { data = buffer.create(2) }
            end)
            local ok, err = pcall(give)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("vcr.record: " .. refused)
        end)
    end)
end)
