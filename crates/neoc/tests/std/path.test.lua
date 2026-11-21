local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local path = require("std:path")

-- The expectations below are for `/` paths. On Windows the same calls use `\` and drive prefixes, which `separator`
-- tells apart; the cases that depend on it run on the platform they describe.
local unix = path.separator == "/"

describe("path", function()

    describe("separator", function()
        test("is one character", function()
            expect(#path.separator).toBe(1)
        end)
    end)

    describe("join", function()
        test("joins parts with the separator", function()
            local sep = path.separator
            expect(path.join("a", "b", "c.txt")).toBe("a" .. sep .. "b" .. sep .. "c.txt")
        end)

        test("one part is returned as it is", function()
            expect(path.join("a")).toBe("a")
        end)

        test("does not normalise", function()
            local sep = path.separator
            expect(path.join("a", "..")).toBe("a" .. sep .. "..")
        end)

        test("an absolute later part replaces what came before", function()
            if not unix then return end
            expect(path.join("a", "/b", "c")).toBe("/b/c")
        end)

        test("no argument is an error", function()
            expect(function() path.join() end).toThrow()
        end)
    end)

    describe("normalize", function()
        test("drops dots and repeated separators and folds parents", function()
            if not unix then return end
            expect(path.normalize("a/./b//c")).toBe("a/b/c")
            expect(path.normalize("a/b/..")).toBe("a")
            expect(path.normalize("./a")).toBe("a")
            expect(path.normalize("a/../..")).toBe("..")
            expect(path.normalize("../a")).toBe("../a")
        end)

        test("a parent of the root stays at the root", function()
            if not unix then return end
            expect(path.normalize("/..")).toBe("/")
            expect(path.normalize("/a/../..")).toBe("/")
        end)

        test("an empty result is a dot", function()
            expect(path.normalize("")).toBe(".")
            expect(path.normalize(".")).toBe(".")
            expect(path.normalize("a/..")).toBe(".")
        end)
    end)

    describe("parent", function()
        test("is the path without its last component", function()
            if not unix then return end
            expect(path.parent("/a/b")).toBe("/a")
            expect(path.parent("a/b/c")).toBe("a/b")
            expect(path.parent("a")).toBe("")
        end)

        test("a root has none", function()
            if not unix then return end
            expect(path.parent("/")).toBeNil()
            expect(path.parent("")).toBeNil()
        end)
    end)

    describe("file_name, file_stem and extension", function()
        test("take a path apart", function()
            expect(path.file_name("a/b.txt")).toBe("b.txt")
            expect(path.file_stem("a/b.tar.gz")).toBe("b.tar")
            expect(path.extension("a/b.txt")).toBe("txt")
            expect(path.extension("a/b.tar.gz")).toBe("gz")
        end)

        test("a dotfile has no extension", function()
            expect(path.file_name(".bashrc")).toBe(".bashrc")
            expect(path.file_stem(".bashrc")).toBe(".bashrc")
            expect(path.extension(".bashrc")).toBeNil()
        end)

        test("no file name, no extension", function()
            expect(path.file_name("a/..")).toBeNil()
            expect(path.file_stem("a/..")).toBeNil()
            expect(path.extension("a/b")).toBeNil()
            expect(path.file_name("")).toBeNil()
        end)
    end)

    describe("with_extension and with_file_name", function()
        test("replace one part", function()
            if not unix then return end
            expect(path.with_extension("a/b.txt", "md")).toBe("a/b.md")
            expect(path.with_extension("a/b", "md")).toBe("a/b.md")
            expect(path.with_extension("a/b.txt", "")).toBe("a/b")
            expect(path.with_file_name("a/b.txt", "c.md")).toBe("a/c.md")
        end)
    end)

    describe("is_absolute", function()
        test("tells absolute from relative", function()
            if not unix then return end
            expect(path.is_absolute("/a")).toBe(true)
            expect(path.is_absolute("a")).toBe(false)
            expect(path.is_absolute("")).toBe(false)
        end)
    end)

    describe("absolute", function()
        test("keeps an absolute path and joins a relative one to the current directory", function()
            if not unix then return end
            expect(path.absolute("/a/b")).toBe("/a/b")
            local resolved, err = path.absolute("a/b")
            expect(err).toBeNil()
            expect(path.is_absolute(resolved)).toBe(true)
            expect(path.parent(path.parent(resolved)) ~= nil).toBe(true)
            expect(path.file_name(resolved)).toBe("b")
        end)

        test("drops dots and keeps parents, without looking at the disk", function()
            if not unix then return end
            expect(path.absolute("/no/such/./dir//x")).toBe("/no/such/dir/x")
            expect(path.absolute("/a/../b")).toBe("/a/../b")
        end)

        test("an empty path is an error", function()
            local resolved, err = path.absolute("")
            expect(resolved).toBeNil()
            expect(err).toContain("path.absolute:")
        end)
    end)

    describe("components", function()
        test("lists a root and then each name", function()
            if not unix then return end
            expect(table.concat(path.components("/a/./b/../c"), "|")).toBe("/|a|b|..|c")
            expect(table.concat(path.components("a/b"), "|")).toBe("a|b")
            expect(#path.components("")).toBe(0)
        end)
    end)

    describe("strip_prefix", function()
        test("works by component, not by characters", function()
            if not unix then return end
            expect(path.strip_prefix("/a/b/c", "/a/b")).toBe("c")
            expect(path.strip_prefix("/a/bc", "/a/b")).toBeNil()
            expect(path.strip_prefix("/a", "/a")).toBe("")
            expect(path.strip_prefix("/a", "/b")).toBeNil()
        end)
    end)

    describe("hostile input", function()
        test("a NUL byte or invalid UTF-8 is just bytes, never an error or a crash", function()
            expect(type(path.join("a\0b", "c"))).toBe("string")
            expect(type(path.normalize("\255\254/.."))).toBe("string")
            expect(path.file_name("x/\255") ~= nil).toBe(true)
        end)

        test("a very long path is handled", function()
            local long = string.rep("a/", 50000)
            expect(type(path.normalize(long))).toBe("string")
            expect(#path.components(long)).toBe(50000)
        end)

        test("a wrong argument type is an error", function()
            expect(function() path.normalize({}) end).toThrow()
            expect(function() path.parent(nil) end).toThrow()
        end)
    end)
end)
