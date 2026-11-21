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
local path = require("std:path")
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

-- directory listings, metadata and tree operations: results are optional, the entries are typed
local entries = assert(fs.walk("/tmp/tree", { max_depth = 2 }))
for _, entry in entries do
	print(entry:file_name(), entry:kind(), entry:depth(), entry:is_dir(), path.extension(entry:path()))
end
local listing = assert(fs.read_dir("/tmp/tree"))
print(#listing)
local meta = assert(fs.metadata("/tmp/x"))
print(meta:len(), meta:is_file(), meta:modified(), meta:mode(), meta:readonly())
print(listing[1]:is_file(), listing[1]:is_symlink())
local link_meta = assert(fs.symlink_metadata("/tmp/link"))
print(link_meta:kind(), link_meta:is_dir(), link_meta:is_symlink(), link_meta:created(), link_meta:accessed())
local present: boolean? = fs.exists("/tmp/x")
local linked = fs.symlink("/tmp/x", "/tmp/link")
local target: string? = fs.read_link("/tmp/link")
local resolved: string? = fs.canonicalize("/tmp/link")
local unlinked = fs.remove_file("/tmp/link", { missing_ok = true })
local rmdir_ok, rmdir_err = fs.remove_dir("/tmp/empty")
print(present, linked, target, resolved, unlinked, rmdir_ok, rmdir_err)
local copied, copy_err = fs.copy("/tmp/x", "/tmp/y")
if copy_err ~= nil then
	print(copied, fs.error_kind(copy_err) == "not_found", copy_err)
end
local bytes: number? = fs.copy_dir("/tmp/tree", "/tmp/tree2")
local removed = fs.remove_dir_all("/tmp/tree2", { missing_ok = true })
fs.move("/tmp/y", "/tmp/z")
fs.rename("/tmp/z", "/tmp/w")
fs.set_permissions("/tmp/w", { mode = 420 })
fs.set_permissions("/tmp/w", { readonly = true })
local watcher = assert(fs.watch("/tmp/tree", { recursive = true }))
local change, change_err = watcher:receive(0.5)
if change then
	local kind: "create" | "modify" | "remove" | "rename" | "overflow" = change.kind
	print(kind, change.path)
elseif change_err then
	print(fs.error_kind(change_err) == "timed_out")
end
watcher:close()
print(bytes, removed, path.join("a", "b", "c"), path.parent("/a/b"), path.normalize("a/../b"), path.is_absolute("/a"))
print(#path.components("/a/b"), path.absolute("x"), path.file_name("a/b.txt"), path.separator)

-- a resource type has methods, called with a colon
local object = json.object()
object:set("name", "Ada")
print(object:len(), object:has("name"))
local list = json.array()
list:push(1)
list:set(1, "one")
-- a path is object keys and 1-based array indexes
print(object:get("name"), list:get(1), object:get("name", 1, "x"))

-- a parsed document is a handle, the null sentinel or a scalar: narrow it before calling a method
local document = assert(serde_json.from_str('{"users": [{"name": "Ada"}]}')) :: lib_json_Object
print(document:get("users", 1, "name"), document:keys())
local rows = assert(serde_json.from_str("[1, 2]")) :: lib_json_Array
print(rows:len(), rows:get(2))
local scalar = assert(serde_json.from_str("42"))
if type(scalar) == "number" then
	print(scalar + 1)
end
print(serde_json.from_str("null") == json.null)
local _null: lib_json_JsonNull = json.null

-- constructors live on a sub-table of the module, fields read as plain values
local parsed = assert(url.Url.parse("https://example.com/a?b=1"))
print(parsed.host, parsed.scheme:upper(), parsed.href)
local version = assert(semver.Version.parse("1.2.3"))
print(version.major + 1, version:increment("minor").minor)
local date = assert(chrono.Date.new(2026, 5, 20))
print(date.year, assert(date:format("%Y-%m-%d")))
local timeout = time.Duration.from_secs(5)
print(timeout:as_millis(), time.Duration.ZERO:is_zero())
print(time.sleep(timeout):as_secs(), time.Instant.now():elapsed():as_millis())
print(chrono.Duration.of({ hours = 1 }), glob.Glob.new("*.lua"))
-- the optional argument of a constructor
print(assert(tempfile.TempFile.new()), assert(tempfile.TempFile.new({ prefix = "x" })))
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
local statement = db:prepare("select * from t where a = ?")
print(statement:query(1))
db:close()
print(json.null)

-- a callback is a function; a function that returns several values
local running = task.spawn(function(...)
	return select("#", ...)
end, 1, 2, 3)
print(running:await())
print(running:is_finished())
task.defer(function() end)
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
print(server:addr(), server:wait(), thread.parent and thread.parent:send(1))
server:close()
-- a handler may return nothing, or only on some paths, and reads the request's fields
hyper.serve(listener, function(_req) end)
hyper.serve(listener, function(req)
	print(req.method, req.path, req.body, req.is_json, req.headers, req.json)
	if req.path == "/hello" then
		return "hello"
	end
end)
hyper.serve(listener, function(_req)
	return { status = 404 }
end)
hyper.serve(listener, function(_req)
	return { status = 201, body = "made", headers = { ["content-type"] = "text/plain" } }
end)
local response, request_err = hyper.get("http://127.0.0.1:1/")
print(response, request_err)

-- the SQL parameters of the network databases are trailing arguments too
local function network_databases(
	mysql: vnd_sqlx_mysql_Database,
	mysql_statement: vnd_sqlx_mysql_Statement,
	postgres: vnd_sqlx_postgres_Database
)
	print(mysql:execute("INSERT INTO t VALUES (?, ?)", 1, 2))
	print(mysql:query("SELECT * FROM t WHERE a = ? AND b = ?", 1, 2))
	print(mysql:query_one("SELECT * FROM t WHERE a = ?", 1))
	print(mysql_statement:execute(1, 2))
	print(mysql_statement:query(1, 2))
	print(mysql_statement:query_one(1))
	print(postgres:execute("INSERT INTO t VALUES ($1, $2)", 1, 2))
	print(postgres:query("SELECT * FROM t WHERE a = $1 AND b = $2", 1, 2))
	print(postgres:query_one("SELECT * FROM t WHERE a = $1", 1))
end
print(network_databases)

-- a handler reads its request, and may name the type of one
local function describe(req: vnd_hyper_Request)
	print(req.method, req.path, req.body, req.is_json, req.headers, req.json, req:header("accept"))
end
print(describe)

-- the test framework: a plain table of matchers, called with a dot
test.describe("fixture", function()
	test.test("matches", function()
		test.expect(1 + 1).toBe(2)
		test.expect(nil).toBeNil()
		test.expect(true).toBeTruthy()
		test.expect(false).toBeFalsy()
		test.expect({ 1, 2 }).toContain(2)
		test.expect(function() error("x") end).toThrow()
	end)
end)

-- a logger and an environment
log.info("hello", { user = "ada" })
print(env.os:upper())
