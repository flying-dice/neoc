--!strict
-- Correct use of the `app` API, `vos:ui` and the std:, lib: and vnd: modules an app is offered, in Luau. `bun run
-- lint:luau` requires this file to check clean (no diagnostic, no lint warning) in strict mode against
-- types/generated/vos.d.luau; the matching wrong uses are in negative/. If the declarations stop loading or stop
-- matching the API, this file (or those) fails.
--
-- The check also requires every module an app is offered to be `require`d here (a line of the form
-- `= require("lib:json")`), so a new module needs a line in this file.
local app = require("vos:app")
local ui = require("vos:ui")
local collections = require("std:collections")
local env = require("std:env")
local fs = require("std:fs")
local io = require("std:io")
local net = require("std:net")
local path = require("std:path")
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
  env,
  io,
  net,
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
  hyper,
  jsonschema,
  quick_xml,
  serde_json,
  serde_yaml,
  sqlx_mysql,
  sqlx_postgres,
  testcontainers_modules,
  toml,
}

type Node = vos_Node
type Window = vos_WindowContext

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
local _kind: string? = fs.error_kind("fs.read: not_found: /tmp/x: gone")
local _slept: number = task.wait(0)

-- ── the globals an app has: the base library of Luau, `utf8` and `typeof` from the prelude, `bit32` ────────────
local _chars: number? = utf8.len("é")
local _same: boolean = typeof("a") == "string"
local _cloned = table.clone { 1, 2, 3 }
local _packed = table.pack(1, 2, 3)
local _parts: { string } = string.split("a,b", ",")
local _clamped: number = math.clamp(5, 0, 3)
local _bits: number = bit32.band(6, 3)

-- ── the app API ────────────────────────────────────────────────────────────────────────────────────────────────
app.manifest {
  id = "usage",
  name = "Usage",
  icon = "puzzlepiece",
  tint = "blue",
  window = { w = 320, h = 240, minW = 320, minH = 240 },
}

local state = app.state { count = 0, label = "" }
state.count += 1
state.label = "x"

app.db:migrate { [1] = "create table items (id integer primary key, name text not null)" }
local items = app.db:query("select id, name from items order by id", { key = "id", page = 50 })
local item = app.db:row("select id, name from items where id = :id", { key = "id" })
local itemCount = app.db:scalar("select count(*) from items")
local chat = app.thread()
local _written: number = app.db:exec("insert into items(name) values (:n)", { n = "a" }).changes
local _rows: { { [string]: any } } = app.db:all("select * from items")
local _first: { [string]: any }? = app.db:one("select * from items limit 1")
local _tx: number = app.db:tx(function()
  return 1
end)
app.kv:set("k", { 1, 2 })
local _stored: any = app.kv:get("k")
app.notify { title = "hello", body = "world" }
app:notify { title = "hello" }
app.log("usage")
app.shell.badge(3)
app.timer.every(60, function() end)
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

-- ── `vos:ui` ───────────────────────────────────────────────────────────────────────────────────────────────────
local function row(r: vos_RowRefs): Node
  return ui.Row { tag = r.id, title = r.name, subtitle = ui.concat("#", r.id), icon = "tray" }
end

local function screen(w: Window): Node
  local c = w.client
  local current = item { id = c.noteId }
  local list = items {}
  local save = function(e: vos_EventArg<{ value: string }>)
    app.kv:set("last", e.payload.value)
  end
  -- a list of actions is a literal; a list that mixes actions and functions needs its type (Luau infers a literal
  -- in a union position, as `onTap` is, from its first entry)
  local both: { vos_Action | ((vos_EventArg<{}>) -> ()) } = { ui.act.set(c.noteId, 0), function() end }
  return ui.VStack {
    spacing = 2,
    padding = { x = 2, top = 1 },
    frame = { minW = 100, maxW = 300 },
    ui.Text { "Hello", style = "title1", foreground = "secondary" },
    ui.Text { key = "n", ui.concat("Items: ", ui.fmt.number(list.total)), live = "polite" },
    ui.Button {
      key = "go",
      title = "Go",
      shape = "circle",
      onTap = function(e)
        w:announce(e.actor.name)
        w:toast { text = "gone", kind = "success" }
        w.client:set("noteId", 3)
      end,
    },
    ui.Button { key = "two", title = "Two", onTap = both },
    ui.Button { key = "three", title = "Three", onTap = { ui.act.set(c.noteId, 1), ui.act.toggle(c.flag) } },
    ui.TextField { key = "name", text = c.name, label = "Name", onSubmit = save },
    ui.TextEditor { key = "body", text = current:bind("name"), label = "Body", sync = "debounce:400" },
    ui.Toggle { key = "t", on = c.flag, title = "On" },
    ui.Show {
      when = ui.gt(list.total, 0),
      ui.List {
        key = "list",
        label = "Items",
        selection = c.noteId,
        ui.ForEach { source = list, row = row },
      },
      otherwise = ui.EmptyState { key = "none", icon = "tray", title = "No items" },
    },
    ui.Show {
      when = ui.eq(c.noteId, 0),
      ui.ChatTranscript { key = "chat", thread = chat { thread = c.noteId, agent = "claude" } },
    },
    ui.each({ 1, 2, 3 }, function(n: number): Node?
      if n == 2 then
        return nil
      end
      return ui.Spacer { key = `s{n}` }
    end),
    ui.Shortcut { key = "sc", shortcut = "cmd+k", onTrigger = ui.act.focus("name") },
  }
end

app.window("main", {
  title = "Usage",
  client = {
    noteId = { init = 0, persist = "app" },
    name = { init = "", persist = "session" },
    flag = { init = false },
  },
  render = screen,
  onOpen = function(w: Window, params)
    w:focus("name")
  end,
})

local _totals: { any } = { itemCount, ui.act.copy("x"), ui.act.push { name = "note", id = 1 }, ui.when(true, 1, 2) }
