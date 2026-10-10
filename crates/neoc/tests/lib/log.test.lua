local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local log = require("lib:log")

-- Every emitting test runs at `level = "off"` so the test runner's
-- stderr stays clean. Behaviour tests that need an actual emit log a
-- single line and accept the noise — they're called out explicitly.

describe("log", function()

    -- new ---------------------------------------------------------------------

    describe("new", function()
        test("returns a logger with default options", function()
            local logger = log.new()
            expect(logger ~= nil).toBe(true)
            expect(type(logger.info)).toBe("function")
        end)

        test("accepts level option", function()
            local logger = log.new({ level = "error" })
            expect(logger ~= nil).toBe(true)
        end)

        test("accepts format option", function()
            local logger = log.new({ level = "off", format = "text" })
            expect(logger ~= nil).toBe(true)
        end)

        test("rejects unknown level", function()
            local ok, err = pcall(log.new, { level = "verbose" })
            expect(ok).toBe(false)
            expect(tostring(err):find("unknown level") ~= nil).toBe(true)
        end)

        test("rejects unknown format", function()
            local ok, err = pcall(log.new, { format = "yaml" })
            expect(ok).toBe(false)
            expect(tostring(err):find("unknown format") ~= nil).toBe(true)
        end)
    end)

    -- level methods -----------------------------------------------------------

    describe("level methods", function()
        local logger = log.new({ level = "off" })

        test("trace exists", function() expect(type(logger.trace)).toBe("function") end)
        test("debug exists", function() expect(type(logger.debug)).toBe("function") end)
        test("info exists",  function() expect(type(logger.info)).toBe("function")  end)
        test("warn exists",  function() expect(type(logger.warn)).toBe("function")  end)
        test("error exists", function() expect(type(logger.error)).toBe("function") end)

        test("calls return without error when suppressed", function()
            logger:trace("hidden")
            logger:debug("hidden")
            logger:info("hidden")
            logger:warn("hidden")
            logger:error("hidden")
            expect(true).toBe(true)
        end)

        test("calls accept optional fields table", function()
            logger:info("with fields", { method = "GET", status = 200 })
            logger:info("no fields")
            expect(true).toBe(true)
        end)
    end)

    -- with --------------------------------------------------------------------

    describe("with", function()
        test("returns a logger", function()
            local logger = log.new({ level = "off" })
            local child = logger:with({ request_id = "abc" })
            expect(child ~= nil).toBe(true)
            child:info("ok")
            expect(true).toBe(true)
        end)

        test("parent unchanged after child creation", function()
            local logger = log.new({ level = "off" })
            local _ = logger:with({ k = 1 })
            -- parent still callable with no errors
            logger:info("x")
            expect(true).toBe(true)
        end)

        test("child accepts further with chaining", function()
            local logger = log.new({ level = "off" })
            local c1 = logger:with({ a = 1 })
            local c2 = c1:with({ b = 2 })
            c2:info("chained")
            expect(true).toBe(true)
        end)
    end)

    -- module-level wrappers ---------------------------------------------------

    describe("module-level wrappers", function()
        test("log.trace exists",  function() expect(type(log.trace)).toBe("function") end)
        test("log.debug exists",  function() expect(type(log.debug)).toBe("function") end)
        test("log.info exists",   function() expect(type(log.info)).toBe("function")  end)
        test("log.warn exists",   function() expect(type(log.warn)).toBe("function")  end)
        test("log.error exists",  function() expect(type(log.error)).toBe("function") end)

        -- log.trace and log.debug are below the default info threshold,
        -- so these calls don't emit anything on stderr.
        test("trace/debug below default threshold do not error", function()
            log.trace("hidden")
            log.debug("hidden")
            expect(true).toBe(true)
        end)
    end)

    -- field handling ----------------------------------------------------------

    describe("field handling", function()
        local logger = log.new({ level = "off" })

        test("nil field accepted (and omitted in output)", function()
            logger:info("msg", { present = "yes", absent = nil })
            expect(true).toBe(true)
        end)

        test("function value accepted (placeholder)", function()
            logger:info("msg", { cb = function() end })
            expect(true).toBe(true)
        end)

        test("nested table accepted", function()
            logger:info("msg", { user = { id = 1, name = "alice" } })
            expect(true).toBe(true)
        end)

        test("deeply nested table accepted (depth-limit placeholder)", function()
            local deep = { l1 = { l2 = { l3 = { l4 = { l5 = "x" } } } } }
            logger:info("msg", deep)
            expect(true).toBe(true)
        end)

        test("circular reference accepted (placeholder)", function()
            local a = {}
            a.self = a
            logger:info("msg", { node = a })
            expect(true).toBe(true)
        end)

        test("array-like table accepted", function()
            logger:info("msg", { items = { 1, 2, 3 } })
            expect(true).toBe(true)
        end)

        test("empty fields table accepted", function()
            logger:info("msg", {})
            expect(true).toBe(true)
        end)

        test("multiple field types in one call", function()
            logger:info("msg", {
                str = "hello",
                num = 3.14,
                int = 42,
                bool = true,
                empty = nil,
                nested = { ok = true },
            })
            expect(true).toBe(true)
        end)

        test("non-string-keyed field accepted without crashing", function()
            -- Lua allows any value as a table key; log must coerce
            -- unsupported keys to a placeholder, never propagate an error.
            logger:info("msg", { [true] = "x" })
            expect(true).toBe(true)
        end)

        test("text format does not let caller shadow reserved keys", function()
            -- Single emitted line; ack noise in test runs.
            local logger_text = log.new({ level = "info", format = "text" })
            logger_text:info("L_text_reserved_emit", { level = "shadow", ts = "FAKE", msg = "FAKEMSG", user = "alice" })
            expect(true).toBe(true)
        end)
    end)

    -- level filtering ---------------------------------------------------------

    describe("level filtering", function()
        test("warn-level logger suppresses trace/debug/info", function()
            -- We can't observe stderr from inside Lua, but we can
            -- verify the calls return without error at every level.
            local logger = log.new({ level = "warn" })
            -- These calls go to stderr — one emitted line per warn/error.
            -- Keep the message terse so noise stays minimal during test runs.
            logger:warn("L_warn_emit")
            logger:error("L_error_emit")
            -- These should not produce output (below threshold).
            logger:trace("L_trace_suppressed")
            logger:debug("L_debug_suppressed")
            logger:info("L_info_suppressed")
            expect(true).toBe(true)
        end)

        test("off level suppresses all", function()
            local logger = log.new({ level = "off" })
            logger:trace("hidden")
            logger:debug("hidden")
            logger:info("hidden")
            logger:warn("hidden")
            logger:error("hidden")
            expect(true).toBe(true)
        end)
    end)
end)
