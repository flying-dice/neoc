local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local archive  = require("lib:archive")
local fs       = require("std:fs")
local tempfile = require("lib:tempfile")

-- A scratch directory with two source files, used by the file-oriented and
-- directory-walking cases. Returns the TempDir handle (keep it alive) and its
-- absolute path.
local function scratch()
    local dir = tempfile.tempdir()
    local root = dir:path()
    fs.write(root .. "/main.lua", "print('hi')")
    fs.write(root .. "/Cargo.toml", "[package]")
    return dir, root
end


describe("archive", function()

    -- tar: in-memory round-trip ------------------------------------------------

    describe("tar bytes round-trip", function()
        for _, comp in ipairs({ "none", "gzip", "zstd" }) do
            test(comp .. " round-trips name, size, and content", function()
                local dir, root = scratch()
                local blob, err = archive.tar_create_bytes({
                    { path = root .. "/main.lua", archive_path = "app/main.lua" },
                }, { compression = comp })
                expect(err).toBeNil()
                expect(#blob > 0).toBeTruthy()

                local entries, list_err = archive.tar_list_bytes(blob)
                expect(list_err).toBeNil()
                expect(#entries).toBe(1)
                expect(entries[1].name).toBe("app/main.lua")
                expect(entries[1].size).toBe(11)
                -- tar stores entries inside one solid stream: no per-entry ratio.
                expect(entries[1].compression_ratio).toBeNil()

                local content, ex_err = archive.tar_extract_bytes(blob, "app/main.lua")
                expect(ex_err).toBeNil()
                expect(content).toBe("print('hi')")
                dir:close()
            end)
        end

        test("binary content survives byte-for-byte", function()
            local dir, root = scratch()
            local raw = "\0\1\2\254\255\127\128"
            fs.write(root .. "/blob.bin", raw)
            local archived = archive.tar_create_bytes({
                { path = root .. "/blob.bin", archive_path = "blob.bin" },
            }, { compression = "gzip" })
            local out = archive.tar_extract_bytes(archived, "blob.bin")
            expect(out).toBe(raw)
            dir:close()
        end)

        test("read auto-detects compression regardless of how it was made", function()
            local dir, root = scratch()
            for _, comp in ipairs({ "none", "gzip", "zstd" }) do
                local blob = archive.tar_create_bytes({
                    { path = root .. "/main.lua" },
                }, { compression = comp })
                -- No hint passed on read — magic-byte detection must cope.
                local entries = archive.tar_list_bytes(blob)
                expect(entries[1].name).toBe(root:sub(2) .. "/main.lua")
            end
            dir:close()
        end)

        test("archive_path defaults to path", function()
            local dir, root = scratch()
            local blob = archive.tar_create_bytes({ { path = root .. "/main.lua" } })
            local entries = archive.tar_list_bytes(blob)
            -- tar drops the leading '/', so the stored name is root-without-slash.
            expect(entries[1].name).toBe(root:sub(2) .. "/main.lua")
            dir:close()
        end)
    end)

    -- tar: strip_prefix --------------------------------------------------------

    test("strip_prefix removes a leading path prefix", function()
        local dir, root = scratch()
        local blob = archive.tar_create_bytes({
            { path = root .. "/main.lua", archive_path = "src/main.lua" },
        }, { strip_prefix = "src/" })
        local entries = archive.tar_list_bytes(blob)
        expect(entries[1].name).toBe("main.lua")
        dir:close()
    end)

    -- tar: file <-> bytes parity + extension inference -------------------------

    describe("tar file IO", function()
        test("create to file, extract from path", function()
            local dir, root = scratch()
            local out = root .. "/bundle.tar.gz"
            local okw, err = archive.tar_create(out, {
                { path = root .. "/main.lua", archive_path = "main.lua" },
                { path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
            })
            expect(err).toBeNil()
            expect(okw).toBeTruthy()

            local destdir = tempfile.tempdir()
            local count, ex_err = archive.tar_extract(out, destdir:path())
            expect(ex_err).toBeNil()
            expect(count).toBe(2)
            expect(fs.read(destdir:path() .. "/main.lua")).toBe("print('hi')")
            destdir:close()
            dir:close()
        end)

        test("compression option clashing with extension is an error", function()
            local dir, root = scratch()
            local val, err = archive.tar_create(root .. "/x.tar.gz", {
                { path = root .. "/main.lua" },
            }, { compression = "zstd" })
            expect(val).toBeNil()
            expect(err).toContain("conflicts with the extension")
            dir:close()
        end)
    end)

    -- include filter -----------------------------------------------------------

    test("include glob extracts only matching entries", function()
        local dir, root = scratch()
        local blob = archive.tar_create_bytes({
            { path = root .. "/main.lua", archive_path = "main.lua" },
            { path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
        })
        local destdir = tempfile.tempdir()
        local count = archive.tar_extract(blob, destdir:path(), { include = { "*.lua" } })
        expect(count).toBe(1)
        expect(fs.read(destdir:path() .. "/main.lua")).toBe("print('hi')")
        local missing = fs.read(destdir:path() .. "/Cargo.toml")
        expect(missing).toBeNil()
        destdir:close()
        dir:close()
    end)

    -- path-traversal protection (extraction is the untrusted boundary) ---------
    --
    -- Extraction runs every entry name through one shared `sanitize_entry_name`
    -- (tar and zip alike); the zip cases below exercise that code path
    -- end-to-end, and its full reject/accept matrix — `..`, absolute, drive,
    -- null, over-long, backslash, and symlink-target escape — is covered
    -- host-independently by the Rust unit tests in `archive.rs`. The tar writer
    -- additionally refuses to *emit* an unsafe name in the first place (defence
    -- in depth on the create side):

    describe("path traversal", function()
        test("tar writer refuses a traversal archive_path", function()
            local dir, root = scratch()
            local blob, err = archive.tar_create_bytes({
                { path = root .. "/main.lua", archive_path = "../escape.lua" },
            })
            expect(blob).toBeNil()
            expect(err).toBeTruthy()
            dir:close()
        end)

        test("zip entry escaping dest is rejected", function()
            local blob = archive.zip_create_bytes({ ["../../evil.txt"] = "pwned" })
            local destdir = tempfile.tempdir()
            local count, err = archive.zip_extract(blob, destdir:path())
            expect(count).toBeNil()
            expect(err).toContain("escapes the destination")
            destdir:close()
        end)

        test("a name that normalises within dest is accepted", function()
            local blob = archive.zip_create_bytes({ ["safe/../safe/ok.txt"] = "fine" })
            local destdir = tempfile.tempdir()
            local count, err = archive.zip_extract(blob, destdir:path())
            expect(err).toBeNil()
            expect(count).toBe(1)
            expect(fs.read(destdir:path() .. "/safe/ok.txt")).toBe("fine")
            destdir:close()
        end)
    end)

    -- decompression-bomb cap ---------------------------------------------------

    test("max_output_size caps extraction", function()
        local big = string.rep("a", 200000)
        local blob = archive.zip_create_bytes({ ["big.txt"] = big })
        local destdir = tempfile.tempdir()
        local count, err = archive.zip_extract(blob, destdir:path(), { max_output_size = 1024 })
        expect(count).toBeNil()
        expect(err).toContain("max_output_size")
        destdir:close()
    end)

    -- zip ----------------------------------------------------------------------

    describe("zip", function()
        test("bytes round-trip with per-entry compression ratio", function()
            local blob, err = archive.zip_create_bytes({
                ["a.txt"] = string.rep("x", 1000),
                ["b.txt"] = "small",
            })
            expect(err).toBeNil()

            local entries = archive.zip_list_bytes(blob)
            expect(#entries).toBe(2)
            -- Highly compressible entry: ratio is a real number well under 1.
            for _, e in ipairs(entries) do
                if e.name == "a.txt" then
                    expect(e.size).toBe(1000)
                    expect(e.compression_ratio < 1).toBeTruthy()
                end
            end

            local content = archive.zip_extract_bytes(blob, "a.txt")
            expect(content).toBe(string.rep("x", 1000))
        end)

        test("create from filesystem paths, extract, read back", function()
            local dir, root = scratch()
            local out = root .. "/out.zip"
            local okw, err = archive.zip_create(out, {
                root .. "/main.lua",
                root .. "/Cargo.toml",
            })
            expect(err).toBeNil()
            expect(okw).toBeTruthy()

            local destdir = tempfile.tempdir()
            local count = archive.zip_extract(out, destdir:path())
            expect(count).toBe(2)
            destdir:close()
            dir:close()
        end)

        test("extract_bytes of a missing entry returns (nil, err)", function()
            local blob = archive.zip_create_bytes({ ["a.txt"] = "x" })
            local val, err = archive.zip_extract_bytes(blob, "nope.txt")
            expect(val).toBeNil()
            expect(err).toContain("not found")
        end)
    end)

end)
