local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local fs = require("std:fs")
local random = require("lib:random")

fs.create_dir_all(".tmp")

describe("fs", function()

    -- create_dir ---------------------------------------------------------------

    describe("create_dir", function()
        test("creates a directory that does not exist", function()
            local name = ".tmp/fs_create_dir_" .. random.uuid_v4()
            local ok, err = fs.create_dir(name)
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)

        test("errors when directory already exists", function()
            local ok, err = fs.create_dir(".tmp")
            expect(ok).toBeNil()
            expect(err).toContain("fs.create_dir:")
        end)

        test("errors when parent does not exist", function()
            local ok, err = fs.create_dir(".tmp/fs_create_dir_no_parent/sub/deep")
            expect(ok).toBeNil()
            expect(err).toContain("fs.create_dir:")
        end)
    end)

    -- create_dir_all -----------------------------------------------------------

    describe("create_dir_all", function()
        test("creates nested directories", function()
            local ok, err = fs.create_dir_all(".tmp/fs_create_dir_all/sub/deep")
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)

        test("succeeds when directory already exists", function()
            fs.create_dir_all(".tmp/fs_create_dir_all_idem")
            local ok, err = fs.create_dir_all(".tmp/fs_create_dir_all_idem")
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)

        test("succeeds when partial path exists", function()
            fs.create_dir_all(".tmp/fs_create_dir_all_partial")
            local ok, err = fs.create_dir_all(".tmp/fs_create_dir_all_partial/child/grandchild")
            expect(err).toBeNil()
            expect(ok).toBe(true)
        end)
    end)

    -- open ---------------------------------------------------------------------

    describe("open", function()
        test("mode r returns a reader", function()
            fs.write(".tmp/fs_open_r.txt", "hello")
            local reader, err = fs.open({ path = ".tmp/fs_open_r.txt", mode = "r" })
            expect(err).toBeNil()
            local data, rerr = reader:read_all()
            expect(rerr).toBeNil()
            expect(data).toBe("hello")
        end)

        test("mode w returns a writer and truncates existing content", function()
            fs.write(".tmp/fs_open_w.txt", "old content")
            local writer, err = fs.open({ path = ".tmp/fs_open_w.txt", mode = "w" })
            expect(err).toBeNil()
            writer:write("new")
            writer:flush()
            local data, _ = fs.read(".tmp/fs_open_w.txt")
            expect(data).toBe("new")
        end)

        test("mode a appends to existing content", function()
            fs.write(".tmp/fs_open_a.txt", "hello")
            local writer, err = fs.open({ path = ".tmp/fs_open_a.txt", mode = "a" })
            expect(err).toBeNil()
            writer:write(" world")
            writer:flush()
            local data, _ = fs.read(".tmp/fs_open_a.txt")
            expect(data).toBe("hello world")
        end)

        test("mode a creates file if it does not exist", function()
            -- reset state from prior runs so the file starts empty
            fs.write(".tmp/fs_open_a_new.txt", "")
            local writer, err = fs.open({ path = ".tmp/fs_open_a_new.txt", mode = "a" })
            expect(err).toBeNil()
            writer:write("created")
            writer:flush()
            local data, _ = fs.read(".tmp/fs_open_a_new.txt")
            expect(data).toBe("created")
        end)

        test("unknown mode returns error", function()
            local handle, err = fs.open({ path = ".tmp/fs_unused.txt", mode = "x" })
            expect(handle).toBeNil()
            expect(err).toContain("fs.open:")
        end)

        test("missing file with mode r returns error", function()
            local handle, err = fs.open({ path = ".tmp/does_not_exist_ever.txt", mode = "r" })
            expect(handle).toBeNil()
            expect(err).toContain("fs.open:")
        end)
    end)

    -- read ---------------------------------------------------------------------

    describe("read", function()
        test("reads entire file as string", function()
            fs.write(".tmp/fs_read_test.txt", "hello world")
            local data, err = fs.read(".tmp/fs_read_test.txt")
            expect(err).toBeNil()
            expect(data).toBe("hello world")
        end)

        test("reads binary content", function()
            local writer, _ = fs.open({ path = ".tmp/fs_read_bin.bin", mode = "w" })
            writer:write("\0\1\2\254\255")
            writer:flush()
            local data, err = fs.read(".tmp/fs_read_bin.bin")
            expect(err).toBeNil()
            expect(data).toBe("\0\1\2\254\255")
        end)

        test("reads empty file as empty string", function()
            fs.write(".tmp/fs_read_empty.txt", "")
            local data, err = fs.read(".tmp/fs_read_empty.txt")
            expect(err).toBeNil()
            expect(data).toBe("")
        end)

        test("missing file returns error", function()
            local data, err = fs.read(".tmp/does_not_exist_ever.txt")
            expect(data).toBeNil()
            expect(err).toContain("fs.read:")
        end)
    end)

    -- write --------------------------------------------------------------------

    describe("write", function()
        test("writes string to file", function()
            local ok, err = fs.write(".tmp/fs_write_test.txt", "written")
            expect(err).toBeNil()
            expect(ok).toBe(true)
            local data, _ = fs.read(".tmp/fs_write_test.txt")
            expect(data).toBe("written")
        end)

        test("overwrites existing content", function()
            fs.write(".tmp/fs_write_overwrite.txt", "first")
            fs.write(".tmp/fs_write_overwrite.txt", "second")
            local data, _ = fs.read(".tmp/fs_write_overwrite.txt")
            expect(data).toBe("second")
        end)

        test("writes empty string", function()
            local ok, err = fs.write(".tmp/fs_write_empty.txt", "")
            expect(err).toBeNil()
            expect(ok).toBe(true)
            local data, _ = fs.read(".tmp/fs_write_empty.txt")
            expect(data).toBe("")
        end)
    end)

end)