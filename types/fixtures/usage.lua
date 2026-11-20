-- Correct use of the `std:`, `lib:` and `vnd:` modules. The luals test (`cargo test -p neoc-vm --test luals`)
-- requires this file to check clean against the generated module definitions (types/luals/<ns>/<name>.lua, found by types/luals-plugin.lua); the
-- matching wrong uses are in negative/wrong_use.lua. If the definitions stop loading or stop matching a module's
-- real surface, this file (or that one) fails.
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

-- every module is a table of its own, so naming it is not an unused local
local _modules = {
  archive, args, cache, compile, compress, crypto, csv, diff, hex, hmac, html, http, ini, log, mime, pkey,
  random, template, text, unicode, vcr, env, io, net, thread, workers, hyper, jsonschema, quick_xml,
  serde_json, serde_yaml, sqlx_mysql, sqlx_postgres, testcontainers_modules, toml, fs,
}

-- a fallible function returns the value, then an error message
local encoded = base64.encode("hello")
local decoded, decode_err = base64.decode(encoded)
if decoded == nil then
  error(decode_err)
end
print(decoded:upper())

-- `assert` narrows the value away from nil
print(assert(hash.hash("sha256", "data")):len())
print(assert(fs.read("/tmp/x")):len())

-- a resource type has methods
local object = json.object()
object:set("name", "Ada")
local count = object:len()
print(count + 1, object:has("name"), object:keys())
print(json.array():len(), json.null)

-- a constructor on a module field, and the methods it returns
local map = collections.Map.new()
map:set("k", 1)
print(map:size() + 1)
local counter = collections.Counter.new(10)
print(counter:increment())

-- a type with static members and methods
local timeout = time.Duration.from_secs(5)
print(timeout:as_millis() + 1, timeout:is_zero(), time.Duration.ZERO:is_zero())
print(time.sleep(timeout):as_secs(), time.Instant.now():elapsed():as_millis())

-- a fallible constructor, and the read-only fields of what it returns
local date = assert(chrono.Date.new(2026, 5, 20))
print(date.year + 1, date.weekday, assert(date:format("%Y")):upper())
local parsed = assert(url.Url.parse("https://example.com:8443/a?b=1"))
print(parsed.scheme:upper(), parsed.href, parsed.port, parsed.host, parsed.path)
local version = assert(semver.Version.parse("1.2.3"))
print(version.major + version.minor + version.patch, version.pre)
print(chrono.Duration.of({ hours = 1 }), glob.Glob.new("*.lua"), regex.Pattern.new("a+", "i"))

-- the optional argument of a function
print(assert(tempfile.TempFile.new()), assert(tempfile.TempFile.new({ prefix = "x" })))

-- functions that take any number of trailing arguments: SQL parameters, a JSON path, a task's arguments
local db = assert(rusqlite.open(":memory:"))
db:execute("CREATE TABLE t (a, b)")
print(db:execute("INSERT INTO t VALUES (?, ?)", 1, 2))
print(db:query("SELECT * FROM t WHERE a = ? AND b = ?", 1, 2))
print(db:query_one("SELECT * FROM t"))
local statement = assert(db:prepare("SELECT * FROM t WHERE a = ? AND b = ?"))
print(statement:query(1, 2))
print(object:get("a", "b", 1))
local job = task.spawn(function(a, b)
  print(a, b)
end, 1, 2)
print(job:is_finished(), job:await())
task.defer(print, "deferred", 1)
task.delay(1, print, "later", 1, 2)

-- an HTTP handler reads the fields of its request
---@param req vnd_hyper.Request
local function handler(req)
  print(req.method, req.path, req.body, req.is_json, req.headers, req.json, req:header("accept"))
end
print(handler, hyper.serve)

-- the test runner: a matcher chain per assertion
test.describe("fixture", function()
  test.test("matchers", function()
    test.expect(1).toBe(1)
    test.expect(nil).toBeNil()
    test.expect(true).toBeTruthy()
    test.expect(false).toBeFalsy()
    test.expect("abc").toContain("b")
    test.expect(function() end).toThrow()
  end)
end)
print(cache, vcr)
