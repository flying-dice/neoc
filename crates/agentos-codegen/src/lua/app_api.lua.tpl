-- ── The `app` module ────────────────────────────────────────────────────────────────────────────────────────
-- `local app = require("@agentos/app")` in app code. The host provides it natively (one VM per app); it is also a
-- global named `app`.

---@class ManifestWindow
---@field w number
---@field h number
---@field minW number
---@field minH number

---@class Manifest
---@field id string
---@field name string
---@field icon string
---@field tint PaletteToken?
---@field kit string?
---@field window ManifestWindow
---@field permissions string[]? "data", "notify", "shell", "agents"

---@class WindowSize
---@field w number
---@field h number

--- `w.client.search` is a Ref to client-managed state; `w.client:get(name)` reads a synced entry (handlers only).
---@class WindowClient
---@field [string] Ref<any>
local WindowClient = {}

---@param name string
---@return any
function WindowClient:get(name) end

---@param name string
---@param value any
function WindowClient:set(name, value) end

---@class ToastOptions
---@field text string
---@field icon string?
---@field kind ("info"|"success"|"error")?
---@field action { title: string, onTap?: fun() }?
---@field duration number?

---@class WindowContext
---@field id string
---@field params table<string, any>
---@field state table<string, any> per-window server state: a value, tracked when read in render
---@field client WindowClient
---@field env table<string, Ref<any>> Refs: sizeClass, heightClass, locale, timeZone, scheme, now
local WindowContext = {}

---@param options ToastOptions
function WindowContext:toast(options) end

---@param key string
function WindowContext:focus(key) end

---@param key string
function WindowContext:scrollTo(key) end

---@param text string
function WindowContext:announce(text) end

---@param text string
function WindowContext:copy(text) end

function WindowContext:close() end

---@param name string
---@param params table<string, any>?
function WindowContext:openWindow(name, params) end

---@class WindowSpec
---@field title string?
---@field size WindowSize?
---@field client table<string, ClientDecl>?
---@field state table<string, any>?
---@field render fun(w: WindowContext): Node
---@field onOpen fun(w: WindowContext, params: table<string, any>)?
---@field onClose fun(w: WindowContext)?

---@class KvApi
local KvApi = {}

---@param key string
---@return any
function KvApi:get(key) end

---@param key string
---@param value any
function KvApi:set(key, value) end

---@param key string
function KvApi:delete(key) end

---@class ExecResult
---@field changes number
---@field last_insert_id number

---@alias SqlParams table<string, any>

--- `key` defaults to "id". A windowed query (`page`) needs a total order: end its `order by` with the key.
---@class QueryOptions
---@field key string?
---@field page number?
---@field total ("exact"|"estimate"|"none")?

---@class WriteContext
---@field now string
---@field actor Actor

---@class RowOptions
---@field key string?
---@field rev string?
---@field write fun(key: any, field: string, value: any, ctx: WriteContext)?
---@field conflict ("merge"|"reject")?
---@field writable string[]? the fields a two-way binding may write (default: any)

--- A live source handle. The SQL never leaves the host; the tree carries only the source id.
---@class RowsSource : Source
---@field total Ref<number> how many rows match

---@class RowSource : Source
---@field [string] Ref<any>
local RowSource = {}

---@param field string
---@return Ref<any>
function RowSource:bind(field) end

---@class ScalarSource : Source

--- A chat thread source for `ChatTranscript`: which thread, and the agent it runs as (literals or Refs). The
--- thread's data is not a query: the client streams it from the host's `/agui`.
---@class ThreadParams
---@field thread string|Ref<string>
---@field agent string|Ref<string>

---@class ThreadSource : Source

--- What an app with the `agents` permission may know of an agent contact.
---@class AgentCard
---@field id string
---@field name string
---@field avatar string?
---@field role string

---@class AgentsApi
local AgentsApi = {}

---@return AgentCard[]
function AgentsApi:list() end

---@class Db
local Db = {}

---@param steps table<integer, string>
function Db:migrate(steps) end

---@param sql string
---@param params SqlParams?
---@return ExecResult
function Db:exec(sql, params) end

---@param sql string
---@param params SqlParams?
---@return table<string, any>[]
function Db:all(sql, params) end

---@param sql string
---@param params SqlParams?
---@return table<string, any>?
function Db:one(sql, params) end

--- The statements inside commit together or not at all; open views see one consistent change.
---@generic T
---@param body fun(): T
---@return T
function Db:tx(body) end

---@param sql string
---@param options QueryOptions?
---@return fun(params: SqlParams?): RowsSource
function Db:query(sql, options) end

---@param sql string
---@param options RowOptions?
---@return fun(params: SqlParams?): RowSource
function Db:row(sql, options) end

---@param sql string
---@return fun(params: SqlParams?): ScalarSource
function Db:scalar(sql) end

--- A field is required unless it is `optional` or has a `default`.
---@class SchemaOptions
---@field description string?
---@field default any?
---@field min number?
---@field max number?
---@field minLength number?
---@field maxLength number?
---@field optional boolean?

---@class Schema
---@field __schema string

---@class SchemaBuilders
---@field string fun(options: SchemaOptions?): Schema
---@field integer fun(options: SchemaOptions?): Schema
---@field number fun(options: SchemaOptions?): Schema
---@field boolean fun(options: SchemaOptions?): Schema
---@field array fun(items: Schema, options: SchemaOptions?): Schema
---@field object fun(fields: table<string, Schema>, options: SchemaOptions?): Schema
---@field enum fun(values: string[], options: SchemaOptions?): Schema

---@class ToolContext
---@field actor Actor
---@field now string
local ToolContext = {}

---@param message string
---@return any
function ToolContext:fail(message) end

---@class ToolDef
---@field name string a local identifier: [a-z][a-z0-9_]* (agents see it as `<appId>__<name>`)
---@field title string?
---@field description string?
---@field input table<string, Schema>?
---@field readOnly boolean? enforced: the tool runs with app writes blocked (db writes, app.kv set/delete, db:migrate raise) and an agent may run it without a permission prompt
---@field run fun(input: any, ctx: ToolContext): any

---@class Notification
---@field title string
---@field body string?
---@field at string?
---@field actions any[]?

---@class Timers
---@field at fun(time: string|number, body: fun())
---@field every fun(seconds: number, body: fun())

---@class MenuCommand
---@field id string
---@field title string
---@field shortcut string?
---@field run fun()

--- The desktop shell's side of the app: declarations only until the shell implements them.
---@class Shell
---@field badge fun(value: (string|number)?) a string, a number (0 clears) or nil
---@field command fun(def: MenuCommand)
---@field deepLink fun(pattern: string, handler: fun(params: table<string, string>)) e.g. "note/:id"

---@class ToolRegistry
---@field register fun(def: ToolDef)

---@class AppApi
---@field id string
---@field kv KvApi
---@field db Db
---@field agents AgentsApi the agent contacts (needs the "agents" permission)
---@field t SchemaBuilders
---@field shell Shell
---@field timer Timers
---@field tools ToolRegistry the same as `app.tool`
local AppApi = {}

---@param manifest Manifest
function AppApi.manifest(manifest) end

---@param name string
---@param spec WindowSpec
function AppApi.window(name, spec) end

--- Shared, in-memory server state: reads in render are tracked, assignments schedule a re-render.
---@generic T
---@param initial T
---@return T
function AppApi.state(initial) end

---@param def ToolDef
function AppApi.tool(def) end

--- Declare a chat thread source once; `chat { thread = id, agent = id }` makes the handle a `ChatTranscript` takes.
---@return fun(params: ThreadParams): ThreadSource
function AppApi.thread() end

---@param name string
---@param params table<string, any>?
function AppApi:openWindow(name, params) end

---@param name string
---@return WindowContext[]
function AppApi:windows(name) end

--- `app.notify { ... }` or `app:notify { ... }`.
---@overload fun(notification: Notification)
---@param notification Notification
function AppApi:notify(notification) end

---@param message string
function AppApi.log(message) end

---@type AppApi
app = nil

-- ── The prelude ─────────────────────────────────────────────────────────────────────────────────────────────
-- What the host adds to Lua 5.1 before an app runs (lua/prelude.lua): `typeof`, `table.clone`, `table.move`,
-- `table.pack`, `table.unpack` and the `utf8` library, with Lua 5.3's semantics.

--- The type of a value, as `type` (the host hands apps no userdata).
---@param value any
---@return string
function typeof(value) end

--- A shallow copy of a table that keeps its metatable.
---@generic T: table
---@param source T
---@return T
function table.clone(source) end

--- Copies `source[first..last]` to `target[to..]` (`target` defaults to `source`); the ranges may overlap.
---@generic T: table
---@param source table
---@param first integer
---@param last integer
---@param to integer
---@param target? T
---@return T|table
function table.move(source, first, last, to, target) end

---@param ... any
---@return table
function table.pack(...) end

---@param list table
---@param i? integer
---@param j? integer
---@return any ...
function table.unpack(list, i, j) end

---@class utf8lib
---@field charpattern string
utf8 = {}

---@param s string
---@param i? integer
---@param j? integer
---@return integer? length the number of characters, or nil
---@return integer? position where the first invalid byte is
function utf8.len(s, i, j) end

---@param s string
---@param n integer
---@param i? integer
---@return integer?
function utf8.offset(s, n, i) end

---@param s string
---@param i? integer
---@param j? integer
---@return integer ...
function utf8.codepoint(s, i, j) end

---@param s string
---@return fun(s: string, i: integer): integer?, integer?
---@return string
---@return integer
function utf8.codes(s) end

---@param ... integer
---@return string
function utf8.char(...) end
