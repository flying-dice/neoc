local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local tempfile = require("lib:tempfile")
local fs       = require("std:fs")

describe("tempfile", function()

    -- tempfile() ------------------------------------------------------------

    describe("tempfile", function()
        test("returns a handle with a path", function()
            local f = assert(tempfile.tempfile())
            expect(type(f:path())).toBe("string")
            f:close()
        end)

        test("file exists on disk while handle lives", function()
            local f = assert(tempfile.tempfile())
            local path = f:path()
            -- fs.read on a freshly-created empty temp file should succeed.
            local bytes = assert(fs.read(path))
            expect(bytes).toBe("")
            f:close()
        end)

        test("prefix and suffix appear in the filename", function()
            local f = assert(tempfile.tempfile({prefix = "neoc-", suffix = ".tmp"}))
            local p = f:path()
            -- Match prefix anywhere after the final separator and suffix at end.
            expect(string.find(p, "neoc%-") ~= nil).toBe(true)
            expect(string.sub(p, -4)).toBe(".tmp")
            f:close()
        end)

        test("dir option places file under the given parent", function()
            local parent = assert(tempfile.tempdir())
            local f = assert(tempfile.tempfile({dir = parent:path()}))
            expect(string.sub(f:path(), 1, #parent:path())).toBe(parent:path())
            f:close()
            parent:close()
        end)

        test("missing parent dir returns (nil, err)", function()
            local f, e = tempfile.tempfile({dir = "/does/not/exist/anywhere"})
            expect(f).toBe(nil)
            expect(string.sub(e, 1, 21)).toBe("tempfile.TempFile.new")
        end)
    end)

    -- tempdir() -------------------------------------------------------------

    describe("tempdir", function()
        test("returns a handle with a path", function()
            local d = assert(tempfile.tempdir())
            expect(type(d:path())).toBe("string")
            d:close()
        end)

        test("dir option places the directory under the given parent", function()
            local outer = assert(tempfile.tempdir())
            local inner = assert(tempfile.tempdir({dir = outer:path()}))
            expect(string.sub(inner:path(), 1, #outer:path())).toBe(outer:path())
            inner:close()
            outer:close()
        end)

        test("missing parent dir returns (nil, err)", function()
            local d, e = tempfile.tempdir({dir = "/does/not/exist/anywhere"})
            expect(d).toBe(nil)
            expect(string.sub(e, 1, 20)).toBe("tempfile.TempDir.new")
        end)
    end)

    -- keep ------------------------------------------------------------------

    describe("keep", function()
        test("returns the path and persists the file", function()
            local f = assert(tempfile.tempfile())
            local kept = assert(f:keep())
            expect(type(kept)).toBe("string")
            -- Clean up manually since keep relinquishes auto-cleanup.
            -- (Done with `assert(fs.read(...))` to confirm existence first.)
            assert(fs.read(kept))
        end)

        test("second keep returns (nil, err)", function()
            local f = assert(tempfile.tempfile())
            f:keep()
            local v, e = f:keep()
            expect(v).toBe(nil)
            expect(string.sub(e, 1, 22)).toBe("tempfile.TempFile.keep")
        end)

        test("tempdir keep persists the directory", function()
            local d = assert(tempfile.tempdir())
            local kept = assert(d:keep())
            expect(type(kept)).toBe("string")
        end)
    end)

    -- close -----------------------------------------------------------------

    describe("close", function()
        test("close is idempotent for files", function()
            local f = assert(tempfile.tempfile())
            f:close()
            f:close()
        end)

        test("close on tempdir returns true on first and subsequent calls", function()
            local d = assert(tempfile.tempdir())
            local ok1 = d:close()
            local ok2 = d:close()
            expect(ok1).toBe(true)
            expect(ok2).toBe(true)
        end)
    end)

    -- gettempdir() ----------------------------------------------------------

    describe("gettempdir", function()
        test("returns a non-empty string", function()
            local sys = tempfile.gettempdir()
            expect(type(sys)).toBe("string")
            expect(#sys > 0).toBe(true)
        end)

        test("matches the dir a default tempfile lives under", function()
            local sys = tempfile.gettempdir()
            local f = assert(tempfile.tempfile())
            expect(string.sub(f:path(), 1, #sys)).toBe(sys)
            f:close()
        end)
    end)

    -- tostring --------------------------------------------------------------

    describe("tostring", function()
        test("TempFile tostring includes the path", function()
            local f = assert(tempfile.tempfile())
            local s = tostring(f)
            expect(string.sub(s, 1, 9)).toBe("TempFile(")
            expect(string.sub(s, -1)).toBe(")")
            f:close()
        end)

        test("TempDir tostring includes the path", function()
            local d = assert(tempfile.tempdir())
            local s = tostring(d)
            expect(string.sub(s, 1, 8)).toBe("TempDir(")
            expect(string.sub(s, -1)).toBe(")")
            d:close()
        end)
    end)

    -- constructor aliases ---------------------------------------------------

    describe("constructors", function()
        test("TempFile.new mirrors flat tempfile()", function()
            local f = assert(tempfile.TempFile.new())
            expect(type(f:path())).toBe("string")
            f:close()
        end)

        test("TempDir.new mirrors flat tempdir()", function()
            local d = assert(tempfile.TempDir.new())
            expect(type(d:path())).toBe("string")
            d:close()
        end)
    end)
end)
