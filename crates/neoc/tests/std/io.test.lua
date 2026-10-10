local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local io = require("std:io")

fs.create_dir_all(".tmp")

describe("io", function()

    -- from_string --------------------------------------------------------------

    describe("from_string", function()
        test("creates a reader from a string", function()
            local reader = io.from_string("hello")
            local data, err = reader:read_all()
            expect(err).toBeNil()
            expect(data).toBe("hello")
        end)

        test("empty string produces an empty reader", function()
            local reader = io.from_string("")
            local data, err = reader:read_all()
            expect(err).toBeNil()
            expect(data).toBe("")
        end)
    end)

    -- read ---------------------------------------------------------------------

    describe("read", function()
        test("reads exactly n bytes", function()
            local reader = io.from_string("hello world")
            local data, err = reader:read(5)
            expect(err).toBeNil()
            expect(data).toBe("hello")
        end)

        test("reads remaining bytes when fewer than n available", function()
            local reader = io.from_string("hi")
            local data, err = reader:read(100)
            expect(err).toBeNil()
            expect(data).toBe("hi")
        end)

        test("advances the cursor across calls", function()
            local reader = io.from_string("hello")
            local first, _  = reader:read(3)
            local second, _ = reader:read(3)
            expect(first).toBe("hel")
            expect(second).toBe("lo")
        end)

        test("returns nil at EOF", function()
            local reader = io.from_string("x")
            reader:read(1)
            local data, err = reader:read(1)
            expect(err).toBeNil()
            expect(data).toBeNil()
        end)
    end)

    -- read_line ----------------------------------------------------------------

    describe("read_line", function()
        test("reads a line stripping the newline", function()
            local reader = io.from_string("hello\nworld\n")
            local line, err = reader:read_line()
            expect(err).toBeNil()
            expect(line).toBe("hello")
        end)

        test("reads successive lines", function()
            local reader = io.from_string("foo\nbar\n")
            local a, _ = reader:read_line()
            local b, _ = reader:read_line()
            expect(a).toBe("foo")
            expect(b).toBe("bar")
        end)

        test("strips CRLF line endings", function()
            local reader = io.from_string("hello\r\n")
            local line, err = reader:read_line()
            expect(err).toBeNil()
            expect(line).toBe("hello")
        end)

        test("reads a line with no trailing newline", function()
            local reader = io.from_string("hello")
            local line, err = reader:read_line()
            expect(err).toBeNil()
            expect(line).toBe("hello")
        end)

        test("returns nil at EOF", function()
            local reader = io.from_string("x\n")
            reader:read_line()
            local line, err = reader:read_line()
            expect(err).toBeNil()
            expect(line).toBeNil()
        end)
    end)

    -- read_all -----------------------------------------------------------------

    describe("read_all", function()
        test("reads all bytes", function()
            local reader = io.from_string("hello world")
            local data, err = reader:read_all()
            expect(err).toBeNil()
            expect(data).toBe("hello world")
        end)

        test("reads only remaining bytes after a prior read", function()
            local reader = io.from_string("hello world")
            reader:read(6)
            local data, err = reader:read_all()
            expect(err).toBeNil()
            expect(data).toBe("world")
        end)

        test("returns empty string on empty input", function()
            local reader = io.from_string("")
            local data, err = reader:read_all()
            expect(err).toBeNil()
            expect(data).toBe("")
        end)
    end)

    -- write + flush ------------------------------------------------------------

    describe("write", function()
        test("writes data to a file", function()
            local writer, err = fs.open({ path = ".tmp/io_write_test.txt", mode = "w" })
            expect(err).toBeNil()
            local ok, werr = writer:write("hello")
            expect(werr).toBeNil()
            expect(ok).toBe(true)
        end)

        test("binary data round-trips through a file", function()
            local writer, _ = fs.open({ path = ".tmp/io_binary_test.bin", mode = "w" })
            writer:write("\0\1\2\254\255")
            writer:flush()

            local data, err = fs.read(".tmp/io_binary_test.bin")
            expect(err).toBeNil()
            expect(data).toBe("\0\1\2\254\255")
        end)
    end)

    describe("flush", function()
        test("flush on a file writer succeeds", function()
            local writer, _ = fs.open({ path = ".tmp/io_flush_test.txt", mode = "w" })
            writer:write("data")
            local ok, err = writer:flush()
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)
    end)

    -- copy ---------------------------------------------------------------------

    describe("copy", function()
        test("copies all bytes from reader to writer", function()
            local reader = io.from_string("copy me")
            local writer, _ = fs.open({ path = ".tmp/io_copy_test.txt", mode = "w" })
            local n, err = io.copy(reader, writer)
            expect(err).toBeNil()
            expect(n).toBe(7)
        end)

        test("copied bytes are readable from the destination", function()
            local reader = io.from_string("hello copy")
            local writer, _ = fs.open({ path = ".tmp/io_copy_verify.txt", mode = "w" })
            io.copy(reader, writer)
            writer:flush()

            local data, err = fs.read(".tmp/io_copy_verify.txt")
            expect(err).toBeNil()
            expect(data).toBe("hello copy")
        end)

        test("wrong first argument returns error", function()
            local writer, _ = fs.open({ path = ".tmp/io_copy_err.txt", mode = "w" })
            local n, err = io.copy(writer, writer)
            expect(n).toBeNil()
            expect(err).toContain("io.copy:")
        end)

        test("wrong second argument returns error", function()
            local reader = io.from_string("x")
            local n, err = io.copy(reader, reader)
            expect(n).toBeNil()
            expect(err).toContain("io.copy:")
        end)
    end)

end)