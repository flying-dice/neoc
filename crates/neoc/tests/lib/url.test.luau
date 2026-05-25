local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local url = require("lib:url")


describe("url", function()

    -- ── Url UserData ─────────────────────────────────────────────────────────

    describe("Url", function()
        test("parse exposes every component", function()
            local u = url.Url.parse("https://ada:secret@example.com:8443/a/b?x=1&y=2#frag")
            expect(u.scheme).toBe("https")
            expect(u.username).toBe("ada")
            expect(u.password).toBe("secret")
            expect(u.host).toBe("example.com")
            expect(u.port).toBe(8443)
            expect(u.path).toBe("/a/b")
            expect(u.query).toBe("x=1&y=2")
            expect(u.fragment).toBe("frag")
        end)

        test("default port is nil", function()
            local u = url.parse("https://example.com/")
            expect(u.port).toBeNil()
        end)

        test("optional components are nil when absent", function()
            local u = url.parse("https://example.com/")
            expect(u.query).toBeNil()
            expect(u.fragment).toBeNil()
            expect(u.password).toBeNil()
            -- Username is the empty string, not nil — matches the underlying
            -- `url` crate where `username()` returns `&str`.
            expect(u.username).toBe("")
        end)

        test("parse rejects malformed input", function()
            local v, err = url.parse("not a url")
            expect(v).toBeNil()
            expect(err).toContain("url.parse:")
        end)

        test("tostring is canonical and matches href", function()
            local u = url.parse("https://example.com/a%20b?x=1#frag")
            expect(tostring(u)).toBe("https://example.com/a%20b?x=1#frag")
            expect(u.href).toBe(tostring(u))
        end)

        test("equality compares canonical form", function()
            local a = url.parse("https://example.com/a")
            local b = url.parse("https://example.com/a")
            local c = url.parse("https://example.com/b")
            expect(a == b).toBeTruthy()
            expect(a == c).toBeFalsy()
        end)

        test("origin returns scheme://host:port for special schemes", function()
            expect((url.parse("https://example.com:8443/a")).origin).toBe("https://example.com:8443")
            expect((url.parse("https://example.com/a")).origin).toBe("https://example.com")
        end)
    end)

    -- ── Mutation via with_* (immutable — returns new Url) ────────────────────

    describe("with_*", function()
        test("with_path / with_query / with_fragment chain", function()
            local u = url.parse("https://example.com/old")
            local v = u:with_path("/new"):with_query("a=1"):with_fragment("here")
            expect(tostring(u)).toBe("https://example.com/old")
            expect(tostring(v)).toBe("https://example.com/new?a=1#here")
        end)

        test("with_port clears when nil", function()
            local u = url.parse("https://example.com:9999/")
            expect(tostring(u:with_port(nil))).toBe("https://example.com/")
        end)

        test("with_port accepts integers in range", function()
            local u, err = (url.parse("https://example.com/")):with_port(8443)
            expect(err).toBeNil()
            expect(tostring(u)).toBe("https://example.com:8443/")
        end)

        test("with_port rejects out-of-range values", function()
            local u = url.parse("https://example.com/")
            local v_high, err_high = u:with_port(99999)
            expect(v_high).toBeNil()
            expect(err_high).toBe("url.Url.with_port: port 99999 out of range (expected 0..=65535)")
            local v_low, err_low = u:with_port(-1)
            expect(v_low).toBeNil()
            expect(err_low).toBe("url.Url.with_port: port -1 out of range (expected 0..=65535)")
        end)

        test("with_port rejects non-integer values", function()
            local u = url.parse("https://example.com/")
            local v, err = u:with_port(8080.5)
            expect(v).toBeNil()
            expect(err).toBe("url.Url.with_port: port 8080.5 is not an integer")
        end)

        test("with_query nil clears the query", function()
            local u = url.parse("https://example.com/?a=1")
            expect((u:with_query(nil)).query).toBeNil()
        end)

        test("with_fragment nil clears the fragment", function()
            local u = url.parse("https://example.com/#here")
            expect((u:with_fragment(nil)).fragment).toBeNil()
        end)

        test("with_host updates host", function()
            local u, err = (url.parse("https://example.com/")):with_host("other.example")
            expect(err).toBeNil()
            expect(u.host).toBe("other.example")
        end)

        test("with_scheme rejects invalid schemes", function()
            local u = url.parse("https://example.com/")
            local v, err = u:with_scheme("not a scheme")
            expect(v).toBeNil()
            expect(err).toContain("url.Url.with_scheme:")
        end)
    end)

    -- ── Resolve (relative against base) ──────────────────────────────────────

    describe("resolve", function()
        test("method form resolves relative reference", function()
            local base = url.parse("https://example.com/dir/page")
            expect(tostring(base:resolve("other"))).toBe("https://example.com/dir/other")
            expect(tostring(base:resolve("/abs"))).toBe("https://example.com/abs")
            expect(tostring(base:resolve("?q=1"))).toBe("https://example.com/dir/page?q=1")
            expect(tostring(base:resolve("#frag"))).toBe("https://example.com/dir/page#frag")
        end)

        test("flat form accepts Url or string base", function()
            local from_url = url.resolve(url.parse("https://example.com/dir/x"), "y")
            local from_str = url.resolve("https://example.com/dir/x", "y")
            expect(tostring(from_url)).toBe("https://example.com/dir/y")
            expect(tostring(from_str)).toBe("https://example.com/dir/y")
        end)

        test("flat form errors on bad base string", function()
            local v, err = url.resolve("not a url", "x")
            expect(v).toBeNil()
            expect(err).toContain("url.resolve:")
        end)
    end)

    -- ── Query inspection ─────────────────────────────────────────────────────

    describe("query_pairs / query_map", function()
        test("query_pairs preserves order and repeats", function()
            local u = url.parse("https://example.com/?a=1&b=2&a=3")
            local pairs_out = u:query_pairs()
            expect(#pairs_out).toBe(3)
            expect(pairs_out[1][1]).toBe("a")
            expect(pairs_out[1][2]).toBe("1")
            expect(pairs_out[2][1]).toBe("b")
            expect(pairs_out[2][2]).toBe("2")
            expect(pairs_out[3][1]).toBe("a")
            expect(pairs_out[3][2]).toBe("3")
        end)

        test("query_map collapses repeats — last wins", function()
            local m = (url.parse("https://example.com/?a=1&b=2&a=3")):query_map()
            expect(m.a).toBe("3")
            expect(m.b).toBe("2")
        end)

        test("empty query yields empty pairs and map", function()
            local u = url.parse("https://example.com/")
            expect(#u:query_pairs()).toBe(0)
            expect(next(u:query_map())).toBeNil()
        end)
    end)

    -- ── Percent encoding ─────────────────────────────────────────────────────

    describe("encode / decode", function()
        test("encode default = component (encodeURIComponent-equivalent)", function()
            expect(url.encode("hello world!/?")).toBe("hello%20world%21%2F%3F")
            -- RFC 3986 unreserved characters survive untouched.
            expect(url.encode("a-_.~b")).toBe("a-_.~b")
        end)

        test("encode set=query keeps `?` but encodes `\"` `#` ` `", function()
            expect(url.encode("a b\"c#d", "query")).toBe("a%20b%22c%23d")
        end)

        test("encode set=path additionally encodes `?` `{` `}`", function()
            expect(url.encode("a b?c{d}", "path")).toBe("a%20b%3Fc%7Bd%7D")
        end)

        test("encode set=fragment encodes `<` `>` ` `", function()
            expect(url.encode("a b<c>d", "fragment")).toBe("a%20b%3Cc%3Ed")
        end)

        test("encode unknown set errors with (nil, err)", function()
            local v, err = url.encode("x", "bogus")
            expect(v).toBeNil()
            expect(err).toContain("url.encode:")
        end)

        test("decode resolves percent-escapes", function()
            expect(url.decode("hello%20world%21")).toBe("hello world!")
        end)

        test("decode does NOT translate `+` to space (that's decode_query)", function()
            expect(url.decode("a+b")).toBe("a+b")
        end)

        test("decode rejects invalid UTF-8", function()
            local v, err = url.decode("%FF%FE")
            expect(v).toBeNil()
            expect(err).toContain("url.decode:")
        end)
    end)

    -- ── Query string round-trip ──────────────────────────────────────────────

    describe("encode_query / decode_query", function()
        test("encode_query form-encodes (space → +)", function()
            local s = url.encode_query({
                {"name", "ada lovelace"},
                {"role", "engineer/lead"},
            })
            expect(s).toBe("name=ada+lovelace&role=engineer%2Flead")
        end)

        test("decode_query yields ordered pairs with + → space", function()
            local pairs_out = url.decode_query("name=ada+lovelace&role=engineer%2Flead")
            expect(#pairs_out).toBe(2)
            expect(pairs_out[1][1]).toBe("name")
            expect(pairs_out[1][2]).toBe("ada lovelace")
            expect(pairs_out[2][1]).toBe("role")
            expect(pairs_out[2][2]).toBe("engineer/lead")
        end)

        test("decode_query preserves repeated keys", function()
            local pairs_out = url.decode_query("a=1&a=2&a=3")
            expect(#pairs_out).toBe(3)
            for i = 1, 3 do
                expect(pairs_out[i][1]).toBe("a")
                expect(pairs_out[i][2]).toBe(tostring(i))
            end
        end)

        test("round-trips arbitrary pairs", function()
            local input = { {"a", "1 2"}, {"b", "c&d"}, {"e", "f=g"}, {"h", "100% pure"} }
            local round = url.decode_query(url.encode_query(input))
            expect(#round).toBe(#input)
            for i, p in ipairs(input) do
                expect(round[i][1]).toBe(p[1])
                expect(round[i][2]).toBe(p[2])
            end
        end)
    end)

    -- ── Compose: build from pieces ───────────────────────────────────────────

    test("build URL from pieces using with_* + encode_query", function()
        local q = url.encode_query({ {"x", "1"}, {"y", "2"} })
        local u = url.parse("https://example.com/")
            :with_path("/api/v1/items")
            :with_query(q)
            :with_fragment("top")
        expect(tostring(u)).toBe("https://example.com/api/v1/items?x=1&y=2#top")
    end)
end)
