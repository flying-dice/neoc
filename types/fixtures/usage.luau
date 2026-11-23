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
local process = require("std:process")
local task = require("std:task")
local thread = require("std:thread")
local time = require("std:time")
local workers = require("std:workers")
local bollard = require("vnd:bollard")
local hyper = require("vnd:hyper")
local jsonschema = require("vnd:jsonschema")
local keyring = require("vnd:keyring")
local portable_pty = require("vnd:portable_pty")
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
	pkey, random, template, tempfile, text, unicode, vcr, collections, env, io, net, thread, workers, bollard, hyper,
	jsonschema, keyring, portable_pty, quick_xml, serde_json, serde_yaml, sqlx_mysql, sqlx_postgres, testcontainers_modules, toml,
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
	print(copied, fs.error_kind(copy_err) == "not_found", (fs.error_message(copy_err) or copy_err):upper())
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
-- a path is object keys and 1-based array indexes; a node is a handle, the null sentinel, a scalar or nil
print(object:get("name"), list:get(1), object:get("name", 1, "x"))
local name = object:get("name")
if type(name) == "string" then
	print(name:upper())
end
for _, key in ipairs(object:keys()) do
	print(key:upper())
end

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
-- a reader in, a writer out (`fs.open` is `any`; the cast names the handle a writer is)
print(serde_json.from_reader(io.from_string("[1]")))
local out = assert(fs.open({ path = "out.json", mode = "w" })) :: std_io_WriterHandle
print(serde_json.to_writer({ a = 1 }, out))
print(serde_yaml.decode(io.from_string("a: 1")), serde_yaml.encode_to({ a = 1 }, out))
print(toml.decode(io.from_string("a = 1")), toml.encode_to({ a = 1 }, out))
print(quick_xml.decode(io.from_string("<a>1</a>")), quick_xml.encode_to({ a = 1 }, out))
-- the null sentinel is tested by value
local _null: lib_json_JsonNull = json.null
print(document:get("missing") == json.null)

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

-- the client: options are typed, the response is a table, a failure has a kind
local fetched, fetch_err = hyper.get("http://127.0.0.1:1/feed.xml", {
	headers = { Accept = "application/xml" },
	connect_timeout = 5,
	idle_timeout = 30,
	timeout = 120,
	follow_redirects = 4,
	max_body = 1024 * 1024,
	ca_pem = "-----BEGIN CERTIFICATE-----",
})
if fetched then
	print(fetched.status + 1, fetched.body:upper(), fetched.headers["content-type"], fetched.url:lower())
elseif hyper.error_kind(fetch_err) == "timed_out" then
	print("the server is not answering")
else
	print((hyper.error_message(fetch_err) or "it did not say why"):upper())
end
print(hyper.get("http://127.0.0.1:1/", { follow_redirects = false }))
print(hyper.post({ url = "http://127.0.0.1:1/", body = buffer.create(4), headers = { ["Content-Type"] = "application/octet-stream" } }))
print(hyper.request({ method = "put", url = "http://127.0.0.1:1/", body = "x", timeout = 1.5 }))

-- Docker: a client sends nothing until its first call; Docker's own tables come back untyped, a failure has a kind
local docker = assert(bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 5 }))
local containers, list_err = docker:list_containers({ all = true, filters = { label = { "app=web" }, status = "running" } })
if containers then
	print(containers[1].Names[1])
elseif bollard.error_kind(list_err) == "connect" then
	print("Docker is not running")
else
	print((bollard.error_message(list_err) or "Docker did not say why"):upper())
end
print(docker:create_container({ name = "web", Image = "nginx", HostConfig = { RestartPolicy = { Name = "always" } } }))
print(docker:stop_container("web", { timeout = 5 }), docker:remove_container("web", { force = true, volumes = true }))
local ran = docker:exec("web", { "nginx", "-t" }, { user = "root" })
if ran then
	print(ran.stdout:upper(), ran.stderr, (ran.exit_code or 0) + 1)
end
local following = docker:logs("web", { follow = true, tail = 100 })
if following then
	local piece, piece_err = following:next()
	print(piece, piece_err)
	following:close()
end
print(docker:pull_image("nginx:1.27", { platform = "linux/arm64" }), docker:events({ filters = { type = "container" } }))

-- a pseudo-terminal: a command on it, its output as bytes until the end (`nil, nil`), and how it ended
local shell, spawn_err = portable_pty.spawn_command({ rows = 24, cols = 80, env = { TERM = "xterm-256color" } })
if shell then
	print(shell:write("echo hello\n"), shell:resize(40, 120), (shell:process_id() or 0) + 1)
	local output, read_err = shell:read(4096)
	print(if output then output:upper() else portable_pty.error_kind(read_err))
	shell:kill()
	local status = shell:wait()
	if status then
		print(status.exit_code + 1, status.success, status.signal)
	end
	shell:close()
elseif portable_pty.error_kind(spawn_err) == "spawn" then
	print("the shell could not be started: " .. (portable_pty.error_message(spawn_err) or ""))
end
print(portable_pty.spawn_command({ program = "ls", args = { "-l" }, cwd = "/tmp" }))

-- a child process: its input and its two outputs as bytes until the end (`nil, nil`), and how it ended
local helper, helper_err = process.spawn({
	program = "cat",
	args = { "-u" },
	cwd = "/tmp",
	env = { NO_COLOR = "1" },
	clear_env = false,
	stdin = "piped",
	stdout = "piped",
	stderr = "null",
})
if helper then
	print(helper:write("hello\n"), helper:close_stdin(), (helper:id() or 0) + 1)
	local reply, reply_err = helper:read(4096)
	print(if reply then reply:upper() else process.error_kind(reply_err), helper:read_stderr())
	helper:kill()
	local ended = helper:wait()
	if ended then
		print((ended.code or 0) + (ended.signal or 0), ended.success)
	end
	helper:close()
elseif process.error_kind(helper_err) == "spawn" then
	print("the helper could not be started")
end

-- the credential store: a mock here; no credential is an answer (`nil, nil`), and a failure has a kind
local credentials = keyring.mock()
local credential, credential_err = credentials:entry("example.com", "ada")
if credential then
	print(credential:set_password("secret"), credential:delete_credential(), credential:get_specifiers())
	local password, password_err = credential:get_password()
	print(if password then password:upper() else keyring.error_kind(password_err))
elseif keyring.error_kind(credential_err) == "invalid_input" then
	print("not a service the store takes")
end
local found = credentials:search({ service = "example.com" })
if found then
	print(#found, found[1]:get_secret())
end
local system_store, store_err = keyring.store()
print(system_store, keyring.error_kind(store_err) == "no_store", (keyring.error_message(store_err) or ""):upper())

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
	-- the parsed body is nil when it is empty or not JSON
	local body = req.json
	if body ~= nil then
		print((body :: lib_json_Object):get("id"))
	end
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
