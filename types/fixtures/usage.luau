--!strict
-- Correct use of the `std:`, `lib:` and `vnd:` modules, in Luau. The luau-lsp test
-- (`NEOC_REQUIRE_LUAU_LSP=1 cargo test -p neoc-vm --test luau_decls`) requires this file to check clean, with
-- no diagnostic and no lint warning, in strict mode against types/neoc.d.luau; the matching wrong uses are in
-- negative/. If the declarations stop loading or stop matching a module's real surface, this file (or those)
-- fails.
--
-- The test also requires every module key to be `require`d here, so a new module needs a line in this file.
local archive = require("lib:archive")
local args = require("lib:args")
local base64 = require("lib:base64")
local cache = require("lib:cache")
local chrono = require("lib:chrono")
local compile = require("lib:compile")
local compress = require("lib:compress")
local crypto = require("lib:crypto")
local csv = require("lib:csv")
local diff = require("lib:diff")
local glob = require("lib:glob")
local hash = require("lib:hash")
local hex = require("lib:hex")
local hmac = require("lib:hmac")
local html = require("lib:html")
local http = require("lib:http")
local ini = require("lib:ini")
local json = require("lib:json")
local log = require("lib:log")
local mime = require("lib:mime")
local pkey = require("lib:pkey")
local random = require("lib:random")
local regex = require("lib:regex")
local semver = require("lib:semver")
local template = require("lib:template")
local tempfile = require("lib:tempfile")
local test = require("lib:test")
local text = require("lib:text")
local unicode = require("lib:unicode")
local url = require("lib:url")
local vcr = require("lib:vcr")
local collections = require("std:collections")
local env = require("std:env")
local fs = require("std:fs")
local io = require("std:io")
local net = require("std:net")
local task = require("std:task")
local thread = require("std:thread")
local time = require("std:time")
local workers = require("std:workers")
local hyper = require("vnd:hyper")
local jsonschema = require("vnd:jsonschema")
local quick_xml = require("vnd:quick_xml")
local rusqlite = require("vnd:rusqlite")
local serde_json = require("vnd:serde_json")
local serde_yaml = require("vnd:serde_yaml")
local sqlx_mysql = require("vnd:sqlx_mysql")
local sqlx_postgres = require("vnd:sqlx_postgres")
local testcontainers_modules = require("vnd:testcontainers_modules")
local toml = require("vnd:toml")

-- every module is a table of its own, so naming it is not an unused import
local _modules: { any } = {
	archive, args, cache, compile, compress, crypto, csv, diff, glob, hex, hmac, html, http, ini, log, mime,
	pkey, random, template, tempfile, text, unicode, vcr, collections, env, io, net, thread, workers, hyper,
	jsonschema, quick_xml, serde_json, serde_yaml, sqlx_mysql, sqlx_postgres, testcontainers_modules, toml,
}

-- a fallible function returns the value, then an error message; checking the value narrows it
local encoded = base64.encode("hello")
local decoded, decode_err = base64.decode(encoded)
if decoded == nil then
	error(decode_err)
end
print(decoded:upper())

-- `assert` narrows the value away from nil
print(assert(hash.hash("sha256", "data")):len())
print(assert(fs.read("/tmp/x")):len())

-- the error can be handled and the value used on the other branch
local written, write_err = fs.write("/tmp/x", "data")
if not written then
	print("write failed:", write_err)
end

-- a resource type has methods, called with a colon
local object = json.object()
object:set("name", "Ada")
print(object:len(), object:has("name"))
local list = json.array()
list:push(1)
list:set(1, "one")

-- constructors live on a sub-table of the module, fields read as plain values
local parsed = assert(url.Url.parse("https://example.com/a?b=1"))
print(parsed.host, parsed.scheme:upper(), parsed.href)
local version = assert(semver.Version.parse("1.2.3"))
print(version.major + 1, version:increment("minor").minor)
local date = assert(chrono.Date.new(2026, 5, 20))
print(date.year, assert(date:format("%Y-%m-%d")))
local timeout = time.Duration.from_secs(5)
print(timeout:as_millis(), time.Duration.ZERO:is_zero())
local counter = collections.Counter.new()
print(counter:increment(), collections.Counter.new(10):get())
local map = collections.Map.new()
map:set("k", { 1, 2, 3 })

-- optional parameters may be left out, and a variadic takes any number of arguments
local pattern = assert(regex.Pattern.new("a+", "i"))
print(pattern:is_match("aaa"), pattern:split("a b", 1))
local db = assert(rusqlite.open(":memory:"))
db:execute("create table t (a integer)")
db:execute("insert into t (a) values (?, ?)", 1, 2)
print(db:query("select * from t"))
db:close()

-- a callback is a function; a function that returns several values
local running = task.spawn(function(...)
	return select("#", ...)
end, 1, 2, 3)
print(running:await())
print(task.wait(0))

-- a handler returns a body, a table or nothing
local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
	local accept = req:header("accept")
	if accept == nil then
		return "no accept header"
	end
	return { status = 200, body = accept:upper() }
end))
print(server:addr())
server:close()
local response, request_err = hyper.get("http://127.0.0.1:1/")
print(response, request_err)

-- the test framework: a plain table of matchers, called with a dot
test.describe("fixture", function()
	test.test("matches", function()
		test.expect(1 + 1).toBe(2)
		test.expect(nil).toBeNil()
	end)
end)

-- a logger and an environment
log.info("hello", { user = "ada" })
print(env.os:upper())
