local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local csv = require("lib:csv")
local io  = require("std:io")
local fs  = require("std:fs")

describe("csv", function()

    -- parse -------------------------------------------------------------------

    describe("parse", function()
        test("rows without headers", function()
            local rows, err = csv.parse("a,b,c\n1,2,3")
            expect(err).toBeNil()
            expect(#rows).toBe(2)
            expect(rows[1][1]).toBe("a")
            expect(rows[2][3]).toBe("3")
        end)

        test("records with headers", function()
            local records = csv.parse("name,age\nAlice,30\nBob,25", { headers = true })
            expect(#records).toBe(2)
            expect(records[1].name).toBe("Alice")
            expect(records[2].age).toBe("25")
        end)

        test("type coercion: number", function()
            local r = csv.parse("duration\n1.23\n4.56", {
                headers = true,
                types   = { duration = "number" },
            })
            expect(r[1].duration).toBe(1.23)
            expect(type(r[1].duration)).toBe("number")
            expect(r[2].duration).toBe(4.56)
        end)

        test("type coercion: boolean", function()
            local r = csv.parse("on\ntrue\nfalse\n1\n0\nTRUE", {
                headers = true,
                types   = { on = "boolean" },
            })
            expect(r[1].on).toBe(true)
            expect(r[2].on).toBe(false)
            expect(r[3].on).toBe(true)
            expect(r[4].on).toBe(false)
            expect(r[5].on).toBe(true)
        end)

        test("types without headers errors", function()
            local v, e = csv.parse("a,b\n1,2", { types = { a = "number" } })
            expect(v).toBeNil()
            expect(string.find(e, "'types' requires 'headers = true'") ~= nil).toBeTruthy()
        end)

        test("duplicate headers errors with line and column", function()
            local v, e = csv.parse("name,age,name\nAlice,30,x", { headers = true })
            expect(v).toBeNil()
            expect(string.find(e, "duplicate header") ~= nil).toBeTruthy()
            expect(string.find(e, "\"name\"") ~= nil).toBeTruthy()
        end)

        test("unequal field count errors", function()
            local v, e = csv.parse("a,b\n1,2\n3", { headers = true })
            expect(v).toBeNil()
            expect(string.find(e, "expected 2") ~= nil).toBeTruthy()
        end)

        test("error snippet preserves source quoting", function()
            -- single-char quoted field must come back quoted, not unquoted
            local _, e = csv.parse('a,b,c\n"x",y,z,extra\n')
            expect(string.find(e, '"x",y,z,extra', 1, true) ~= nil).toBeTruthy()
        end)

        test("error snippet preserves escaped quotes", function()
            local _, e = csv.parse('a,b\n"he said ""hi""",1,extra')
            expect(string.find(e, '"he said ""hi""",1,extra', 1, true) ~= nil).toBeTruthy()
        end)

        test("error snippet truncates long lines with ellipsis", function()
            local long = string.rep("x", 120)
            local _, e = csv.parse('a,b\n' .. long .. ',y,extra')
            expect(string.find(e, '…"$') ~= nil).toBeTruthy()
        end)

        test("empty fields untyped are empty strings", function()
            local r = csv.parse("a,,c")
            expect(r[1][1]).toBe("a")
            expect(r[1][2]).toBe("")
            expect(r[1][3]).toBe("c")
        end)

        test("empty fields typed are nil", function()
            local r = csv.parse("name,age\nAlice,\nBob,30", {
                headers = true,
                types   = { age = "number" },
            })
            expect(r[1].age).toBeNil()
            expect(r[2].age).toBe(30)
        end)

        test("type coercion failure errors", function()
            local v, e = csv.parse("dur\nabc", {
                headers = true,
                types   = { dur = "number" },
            })
            expect(v).toBeNil()
            expect(string.find(e, "cannot coerce") ~= nil).toBeTruthy()
            expect(string.find(e, "abc") ~= nil).toBeTruthy()
        end)

        test("tsv via delimiter option", function()
            local r = csv.parse("a\tb\nx\ty", { delimiter = "\t", headers = true })
            expect(r[1].b).toBe("y")
        end)

        test("multibyte delimiter rejected", function()
            local v, e = csv.parse("a,b", { delimiter = ",," })
            expect(v).toBeNil()
            expect(string.find(e, "single byte") ~= nil).toBeTruthy()
        end)

        test("quoted fields with embedded newlines", function()
            local r = csv.parse("a,b\n\"line\n1\",x")
            expect(r[2][1]).toBe("line\n1")
            expect(r[2][2]).toBe("x")
        end)

        test("escaped quotes inside quoted field", function()
            local r = csv.parse("a\n\"she said \"\"hi\"\"\"")
            expect(r[2][1]).toBe('she said "hi"')
        end)

        test("comment lines are skipped", function()
            local r = csv.parse("# comment\na,b\n1,2", { comment = "#" })
            expect(#r).toBe(2)
            expect(r[1][1]).toBe("a")
        end)
    end)

    -- rows iterator -----------------------------------------------------------

    describe("rows", function()
        test("iterates all records", function()
            local count = 0
            for row in csv.rows("a,b\n1,2\n3,4\n5,6", { headers = true }) do
                expect(row.a ~= nil).toBeTruthy()
                count = count + 1
            end
            expect(count).toBe(3)
        end)

        test("returns nil at EOF on subsequent calls", function()
            local iter = csv.rows("a\n1")
            expect(iter() ~= nil).toBeTruthy()
            expect(iter() ~= nil).toBeTruthy()
            expect(iter()).toBeNil()
            expect(iter()).toBeNil()
        end)

        test("raises on parse error", function()
            local ok = pcall(function()
                for _ in csv.rows("a,b\n1,2\n3", { headers = true }) do end
            end)
            expect(ok).toBe(false)
        end)

        test("error snippet preserves source quoting", function()
            local ok, err = pcall(function()
                for _ in csv.rows('a,b\n"x",y,extra\n') do end
            end)
            expect(ok).toBe(false)
            expect(string.find(tostring(err), '"x",y,extra', 1, true) ~= nil).toBeTruthy()
        end)
    end)

    -- reader (handle-based streaming) -----------------------------------------

    describe("reader", function()
        test("streams records from an io handle", function()
            local h = io.from_string("a,b\n1,2\n3,4")
            local r, err = csv.reader(h, { headers = true })
            expect(err).toBeNil()
            local n = 0
            for _ in r:rows() do n = n + 1 end
            expect(n).toBe(2)
        end)

        test("headers() returns the cached header row", function()
            local h = io.from_string("name,age\nAlice,30")
            local r = csv.reader(h, { headers = true })
            local first = r:headers()
            local second = r:headers()
            expect(first[1]).toBe("name")
            expect(second[1]).toBe("name")
        end)

        test("headers() before rows() does not consume data rows", function()
            local h = io.from_string("name,age\nAlice,30\nBob,25")
            local r = csv.reader(h, { headers = true })
            r:headers()
            local rows = {}
            for row in r:rows() do
                table.insert(rows, row.name)
            end
            expect(#rows).toBe(2)
            expect(rows[1]).toBe("Alice")
        end)

        test("headers() errors without headers = true", function()
            local h = io.from_string("a,b")
            local r = csv.reader(h)
            local v, e = r:headers()
            expect(v).toBeNil()
            expect(string.find(e, "'headers' option must be true") ~= nil).toBeTruthy()
        end)

        test("exclusive borrow rejects second reader on same handle", function()
            local h = io.from_string("a\n1")
            local _r1 = csv.reader(h)
            local r2, e = csv.reader(h)
            expect(r2).toBeNil()
            expect(string.find(e, "already in use") ~= nil).toBeTruthy()
        end)

        test("borrow released when reader is dropped", function()
            local h = io.from_string("a\n1")
            do
                local _r = csv.reader(h)
            end
            collectgarbage("collect")
            collectgarbage("collect")
            local r2, e = csv.reader(h)
            expect(e).toBeNil()
            expect(r2 ~= nil).toBeTruthy()
        end)

        test("argument must be a ReaderHandle", function()
            local r, e = csv.reader("not a handle")
            expect(r).toBeNil()
            expect(string.find(e, "ReaderHandle") ~= nil).toBeTruthy()
        end)

        test("error snippet preserves source quoting from stream", function()
            local h = io.from_string('a,b\n"alpha",beta,extra\n')
            local r = csv.reader(h)
            local ok, err = pcall(function()
                for _ in r:rows() do end
            end)
            expect(ok).toBe(false)
            expect(string.find(tostring(err), '"alpha",beta,extra', 1, true) ~= nil).toBeTruthy()
        end)

        test("streams from a real file", function()
            local path = "/tmp/neoc-csv-test.csv"
            fs.write(path, "x,y\n10,20\n30,40\n")
            local h = fs.open({ path = path, mode = "r" })
            local r = csv.reader(h, { headers = true })
            local sum = 0
            for row in r:rows() do
                sum = sum + tonumber(row.x) + tonumber(row.y)
            end
            expect(sum).toBe(100)
        end)
    end)

    -- stringify ---------------------------------------------------------------

    describe("stringify", function()
        test("writes headers + records in column order", function()
            local out = csv.stringify(
                {
                    { name = "Alice", age = "30" },
                    { name = "Bob",   age = "25" },
                },
                { headers = { "name", "age" } }
            )
            expect(out).toBe("name,age\nAlice,30\nBob,25\n")
        end)

        test("missing keys emit empty string", function()
            local out = csv.stringify(
                { { name = "Alice" } },
                { headers = { "name", "age" } }
            )
            expect(out).toBe("name,age\nAlice,\n")
        end)

        test("custom delimiter for TSV output", function()
            local out = csv.stringify(
                { { a = "x", b = "y" } },
                { headers = { "a", "b" }, delimiter = "\t" }
            )
            expect(out).toBe("a\tb\nx\ty\n")
        end)

        test("fields with delimiter are quoted", function()
            local out = csv.stringify(
                { { a = "x,y", b = "z" } },
                { headers = { "a", "b" } }
            )
            expect(out).toBe("a,b\n\"x,y\",z\n")
        end)

        test("required headers", function()
            local v, e = csv.stringify({}, {})
            expect(v).toBeNil()
            expect(string.find(e, "'headers' is required") ~= nil).toBeTruthy()
        end)
    end)

    -- writer ------------------------------------------------------------------

    describe("writer", function()
        test("array shape rows", function()
            local w = csv.writer({ headers = { "test", "status" } })
            w:row({ "login", "PASS" })
            w:row({ "signup", "FAIL" })
            local out = w:finish()
            expect(out).toBe("test,status\nlogin,PASS\nsignup,FAIL\n")
        end)

        test("keyed shape rows", function()
            local w = csv.writer({ headers = { "test", "status" } })
            w:row({ test = "login", status = "PASS" })
            local out = w:finish()
            expect(out).toBe("test,status\nlogin,PASS\n")
        end)

        test("array length mismatch errors", function()
            local w = csv.writer({ headers = { "a", "b", "c" } })
            local v, e = w:row({ "x", "y" })
            expect(v).toBeNil()
            expect(string.find(e, "expected 3") ~= nil).toBeTruthy()
        end)

        test("mixed array+keyed rejected", function()
            local w = csv.writer({ headers = { "a", "b" } })
            local v, e = w:row({ "x", b = "y" })
            expect(v).toBeNil()
            expect(string.find(e, "array or keyed table") ~= nil).toBeTruthy()
        end)

        test("keyed row missing key emits empty field", function()
            local w = csv.writer({ headers = { "name", "age" } })
            w:row({ name = "Alice" })
            local out = w:finish()
            expect(out).toBe("name,age\nAlice,\n")
        end)

        test("finish twice errors", function()
            local w = csv.writer({ headers = { "a" } })
            w:finish()
            local v, e = w:finish()
            expect(v).toBeNil()
            expect(string.find(e, "already finished") ~= nil).toBeTruthy()
        end)

        test("row after finish errors", function()
            local w = csv.writer({ headers = { "a" } })
            w:finish()
            local v, e = w:row({ "x" })
            expect(v).toBeNil()
            expect(string.find(e, "already finished") ~= nil).toBeTruthy()
        end)

        test("number and boolean values stringify", function()
            local w = csv.writer({ headers = { "n", "b" } })
            w:row({ 42, true })
            w:row({ 3.14, false })
            local out = w:finish()
            expect(out).toBe("n,b\n42,true\n3.14,false\n")
        end)
    end)

    -- round-trip --------------------------------------------------------------

    describe("round-trip", function()
        test("parse then stringify preserves rows", function()
            local input = "name,city\nAlice,Sheffield\nBob,Leeds\n"
            local records = csv.parse(input, { headers = true })
            local out = csv.stringify(records, { headers = { "name", "city" } })
            expect(out).toBe(input)
        end)
    end)
end)
