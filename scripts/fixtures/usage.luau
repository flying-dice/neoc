--!strict
-- Correct use of the `app` API and the std:, lib: and vnd: modules an app is offered, in Luau. `bun run
-- lint:luau` requires this file to check clean (no diagnostic, no lint warning) in strict mode against
-- types/generated/vos.d.luau; the matching wrong uses are in negative/. If the declarations stop loading or stop
-- matching the API, this file (or those) fails.
--
-- The check also requires every module an app is offered to be `require`d here (a line of the form
-- `= require("lib:json")`), so a new module needs a line in this file.
local app = require("vos:app")
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
local archive = require("lib:archive")
local args = require("lib:args")
local base64 = require("lib:base64")
local cache = require("lib:cache")
local chrono = require("lib:chrono")
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
local text = require("lib:text")
local unicode = require("lib:unicode")
local url = require("lib:url")
local vcr = require("lib:vcr")
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
  env,
  io,
  net,
  process,
  thread,
  archive,
  args,
  cache,
  chrono,
  compress,
  crypto,
  csv,
  diff,
  glob,
  hex,
  hmac,
  html,
  http,
  ini,
  mime,
  pkey,
  regex,
  template,
  tempfile,
  text,
  unicode,
  vcr,
  bollard,
  hyper,
  jsonschema,
  keyring,
  portable_pty,
  quick_xml,
  serde_json,
  serde_yaml,
  sqlx_mysql,
  sqlx_postgres,
  testcontainers_modules,
  toml,
}

-- ── the std: and lib: modules are typed: a fallible function returns the value, then an error message ───────────
local encoded = base64.encode("hello")
local decoded, decodeError = base64.decode(encoded)
if decoded == nil then
  error(decodeError)
end
local _upper: string = decoded:upper()
local _hashed: number = #assert(hash.hash("sha256", "data"))
local object = json.object()
object:set("name", "Ada")
local _has: boolean = object:has("name")
local parsed = assert(url.Url.parse("https://example.com/a?b=1"))
local _host: string? = parsed.host
local version = assert(semver.Version.parse("1.2.3"))
local _major: number = version.major + 1
local timeout = time.Duration.from_secs(5)
local _millis: number = timeout:as_millis()
local counter = collections.Counter.new()
local _count: number = counter:increment()
local _db = assert(rusqlite.open(":memory:"))
log.info("typed")
local _dice: number? = random.random_int(1, 6)
local _file: string? = fs.read("/tmp/x")
local _entries = assert(fs.read_dir("/tmp"))
local _name: string? = path.file_name("/tmp/x")
local _gone, fsErr = fs.read("/tmp/gone")
local _kind: std_fs_FsErrorKind? = if fsErr then fsErr.kind else nil
local _message: string? = if fsErr then fsErr.message else nil
local _slept: number = task.wait(0)

-- ── the globals an app has: the base library of Luau, `typeof`, `utf8` from the prelude, `bit32` ────────────
local _chars: number? = utf8.len("é")
local _same: boolean = typeof("a") == "string"
local _cloned = table.clone { 1, 2, 3 }
local _packed = table.pack(1, 2, 3)
local _parts: { string } = string.split("a,b", ",")
local _clamped: number = math.clamp(5, 0, 3)
local _bits: number = bit32.band(6, 3)

-- ── the app API ────────────────────────────────────────────────────────────────────────────────────────────────
app.db:migrate { [1] = "create table items (id integer primary key, name text not null)" }
local _written: number = app.db:exec("insert into items(name) values (:n)", { n = "a" }).changes
local _rows: { { [string]: any } } = app.db:all("select * from items")
local _first: { [string]: any }? = app.db:one("select * from items limit 1")
local _tx: number = app.db:tx(function()
  return 1
end)
app.kv:set("k", { 1, 2 })
local _stored: any = app.kv:get("k")
local docs: vos_app_Location = app.files.documents
local _made: boolean?, _madeErr: vos_app_FileError? = docs:createDir("notes")
local _wrote: boolean? = docs:write("notes/a.txt", "hello")
local _text: string?, readErr: vos_app_FileError? = docs:read("notes/a.txt")
-- a failure is a typed value: its kind is one of a declared set, its message is for a person
local _missing: boolean = readErr ~= nil and readErr.kind == "not_found"
local _errorKind: vos_app_FileErrorKind? = if readErr then readErr.kind else nil
local _why: string? = if readErr then readErr.message else nil
local entries: { vos_app_FileInfo } = docs:list("notes") or {}
for _, entry in entries do
  local _entryName: string, _size: number, _modified: number? = entry.name, entry.size, entry.modified
end
local _rootEntries = app.files.cache:list()
local info: vos_app_FileInfo? = docs:info("notes/a.txt")
if info and info.kind == "file" then
  docs:move("notes/a.txt", "notes/b.txt")
end
docs:copy("notes", "backup")
docs:remove("backup", { recursive = true })
app.files.temp:remove("scratch.txt")
app.notify { title = "hello", body = "world" }
app.notify { title = "hello" }
app.log("usage")
app.emit("items.changed", { id = 1 })
app.emit("items.cleared")
app.openWindow()
app.openWindow { item = 1 }
local _id: string = app.id
for _, card in ipairs(app.agents:list()) do
  local _name: string = card.name
end

app.tool {
  name = "add",
  title = "Add",
  description = "Add an item",
  readOnly = false,
  input = { name = app.t.string { description = "The name" }, n = app.t.integer { default = 1, min = 1, max = 5 } },
  run = function(input: { name: string, n: number }, ctx)
    if input.n > 3 then
      return ctx:fail("too many")
    end
    return { added = input.name, by = ctx.actor.name, at = ctx.now }
  end,
}

-- ── the handlers: what the entry returns, and the app's page calls with `rpc(name, args)` ───────────────────────
local handlers: vos_app_Handlers = {
  list = function(_ctx: vos_app_Context)
    return app.db:all("select id, name from items order by id")
  end,
  rename = function(ctx: vos_app_Context, args: { id: number, name: string })
    if args.name == "" then
      return ctx:fail("A name cannot be empty")
    end
    local changed = app.db:exec("update items set name = :name where id = :id", { id = args.id, name = args.name })
    app.emit("items.changed", { id = args.id, by = ctx.actor.name, kind = ctx.actor.kind, at = ctx.now })
    return changed.changes
  end,
}

return handlers
