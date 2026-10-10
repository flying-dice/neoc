local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local mime = require("lib:mime")


describe("mime", function()

    -- from_extension -----------------------------------------------------------

    describe("from_extension", function()
        test("json with dot", function()
            local result, err = mime.from_extension(".json")
            expect(err).toBeNil()
            expect(result).toBe("application/json")
        end)

        test("html without dot", function()
            local result, err = mime.from_extension("html")
            expect(err).toBeNil()
            expect(result).toBe("text/html")
        end)

        test("css", function()
            local result, err = mime.from_extension(".css")
            expect(err).toBeNil()
            expect(result).toBe("text/css")
        end)

        test("png", function()
            local result, err = mime.from_extension(".png")
            expect(err).toBeNil()
            expect(result).toBe("image/png")
        end)

        test("unknown extension returns (nil, err)", function()
            local val, err = mime.from_extension(".zzzzz")
            expect(val).toBeNil()
            expect(err).toContain("mime.from_extension:")
        end)

        test("empty string returns (nil, err)", function()
            local val, err = mime.from_extension("")
            expect(val).toBeNil()
            expect(err).toContain("mime.from_extension:")
        end)

        test("dot-only returns (nil, err)", function()
            local val, err = mime.from_extension(".")
            expect(val).toBeNil()
            expect(err).toContain("mime.from_extension:")
        end)
    end)

    -- to_extension -------------------------------------------------------------

    describe("to_extension", function()
        test("text/html returns canonical .html via runtime prefs", function()
            local result, err = mime.to_extension("text/html")
            expect(err).toBeNil()
            expect(result).toBe(".html")
        end)

        test("image/jpeg returns canonical .jpg via runtime prefs", function()
            local result, err = mime.to_extension("image/jpeg")
            expect(err).toBeNil()
            expect(result).toBe(".jpg")
        end)

        test("application/json", function()
            local result, err = mime.to_extension("application/json")
            expect(err).toBeNil()
            expect(result).toBe(".json")
        end)

        test("user prefs override runtime prefs", function()
            local prefs = { ["text/html"] = ".xhtml" }
            local result, err = mime.to_extension("text/html", prefs)
            expect(err).toBeNil()
            expect(result).toBe(".xhtml")
        end)

        test("user prefs fall through to runtime prefs", function()
            local prefs = { ["text/html"] = ".xhtml" }
            local result, err = mime.to_extension("image/jpeg", prefs)
            expect(err).toBeNil()
            expect(result).toBe(".jpg")
        end)

        test("user prefs fall through to mime_guess", function()
            local prefs = { ["text/html"] = ".xhtml" }
            local result, err = mime.to_extension("text/css", prefs)
            expect(err).toBeNil()
            expect(result).toBe(".css")
        end)

        test("invalid MIME returns (nil, err)", function()
            local val, err = mime.to_extension("not-a-mime")
            expect(val).toBeNil()
            expect(err).toContain("mime.to_extension:")
        end)

        test("empty string returns (nil, err)", function()
            local val, err = mime.to_extension("")
            expect(val).toBeNil()
            expect(err).toContain("mime.to_extension:")
        end)

        test("dot-only returns (nil, err)", function()
            local val, err = mime.to_extension(".")
            expect(val).toBeNil()
            expect(err).toContain("mime.to_extension:")
        end)
    end)

    -- detect -------------------------------------------------------------------

    describe("detect", function()
        test("simple filename", function()
            local result, err = mime.detect("style.css")
            expect(err).toBeNil()
            expect(result).toBe("text/css")
        end)

        test("compound extension", function()
            local result, err = mime.detect("archive.tar.gz")
            expect(err).toBeNil()
            expect(result).toBe("application/gzip")
        end)

        test("path with directories", function()
            local result, err = mime.detect("assets/images/logo.png")
            expect(err).toBeNil()
            expect(result).toBe("image/png")
        end)

        test("no extension returns (nil, err)", function()
            local val, err = mime.detect("Makefile")
            expect(val).toBeNil()
            expect(err).toContain("mime.detect:")
        end)

        test("empty string returns (nil, err)", function()
            local val, err = mime.detect("")
            expect(val).toBeNil()
            expect(err).toContain("mime.detect:")
        end)

        test("dot-only returns (nil, err)", function()
            local val, err = mime.detect(".")
            expect(val).toBeNil()
            expect(err).toContain("mime.detect:")
        end)
    end)
end)
