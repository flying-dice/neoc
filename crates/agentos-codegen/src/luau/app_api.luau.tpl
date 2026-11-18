-- ── The `app` module ────────────────────────────────────────────────────────────────────────────────────────
-- `local app = require("@agentos/app")` in app code. The host provides it natively (one VM per app); it is also a
-- global named `app` for the legacy entry points. Plain table types, not `declare class`: the `class` keyword
-- changed meaning in newer Luau releases, while aliases parse the same in every luau-lsp version.

type Json = any

type ManifestWindow = { w: number, h: number, minW: number, minH: number }

type Manifest = {
	id: string,
	name: string,
	icon: string,
	tint: PaletteToken?,
	kit: string?,
	window: ManifestWindow,
	permissions: { string }?,
}

type WindowSize = { w: number, h: number }

-- `w.client.search` is a Ref to client-managed state; `w.client:get(name)` reads a synced entry (handlers only).
type WindowClient = {
	get: (self: WindowClient, name: string) -> any,
	set: (self: WindowClient, name: string, value: any) -> (),
	[string]: Ref<any>,
}

type ToastOptions = {
	text: string,
	icon: string?,
	kind: ("info" | "success" | "error")?,
	action: { title: string, onTap: (() -> ())? }?,
	duration: number?,
}

type WindowContext = {
	id: string,
	params: { [string]: any },
	-- per-window server state: a value, tracked when read in render
	state: { [string]: any },
	client: WindowClient,
	-- Refs: sizeClass, heightClass, locale, timeZone, scheme, now
	env: { [string]: Ref<any> },
	toast: (self: WindowContext, options: ToastOptions) -> (),
	focus: (self: WindowContext, key: string) -> (),
	scrollTo: (self: WindowContext, key: string) -> (),
	announce: (self: WindowContext, text: string) -> (),
	copy: (self: WindowContext, text: string) -> (),
	close: (self: WindowContext) -> (),
	openWindow: (self: WindowContext, name: string, params: { [string]: any }?) -> (),
}

type WindowSpec = {
	title: string?,
	size: WindowSize?,
	client: { [string]: ClientDecl }?,
	state: { [string]: any }?,
	render: (w: WindowContext) -> Node,
	onOpen: ((w: WindowContext, params: { [string]: any }) -> ())?,
	onClose: ((w: WindowContext) -> ())?,
}

type KvApi = {
	get: (self: KvApi, key: string) -> any,
	set: (self: KvApi, key: string, value: any) -> (),
	delete: (self: KvApi, key: string) -> (),
}

type ExecResult = { changes: number, last_insert_id: number }

type SqlParams = { [string]: any }

type QueryOptions = { key: string?, page: number?, total: ("exact" | "estimate" | "none")? }

type WriteContext = { now: string, actor: Actor }

type RowOptions = {
	key: string?,
	rev: string?,
	write: ((key: any, field: string, value: any, ctx: WriteContext) -> ())?,
	conflict: ("merge" | "reject")?,
}

-- A live source handle. The SQL never leaves the host; the tree carries only the source id.
type RowsSource = { ["$source"]: string }

type RowSource = {
	["$source"]: string,
	bind: (self: RowSource, field: string) -> Ref<any>,
	[string]: Ref<any>,
}

type ScalarSource = { ["$source"]: string }

type Db = {
	migrate: (self: Db, steps: { [number]: string }) -> (),
	exec: (self: Db, sql: string, params: SqlParams?) -> ExecResult,
	all: (self: Db, sql: string, params: SqlParams?) -> { { [string]: any } },
	one: (self: Db, sql: string, params: SqlParams?) -> { [string]: any }?,
	tx: (self: Db, body: () -> ()) -> (),
	query: (self: Db, sql: string, options: QueryOptions?) -> (params: SqlParams?) -> RowsSource,
	row: (self: Db, sql: string, options: RowOptions?) -> (params: SqlParams?) -> RowSource,
	scalar: (self: Db, sql: string) -> (params: SqlParams?) -> ScalarSource,
}

type SchemaOptions = { description: string?, default: any?, min: number?, max: number? }

type Schema = { __schema: string }

type SchemaBuilders = {
	string: (options: SchemaOptions?) -> Schema,
	integer: (options: SchemaOptions?) -> Schema,
	number: (options: SchemaOptions?) -> Schema,
	boolean: (options: SchemaOptions?) -> Schema,
	array: (items: Schema, options: SchemaOptions?) -> Schema,
	object: (fields: { [string]: Schema }, options: SchemaOptions?) -> Schema,
	enum: (values: { string }, options: SchemaOptions?) -> Schema,
}

type ToolContext = {
	actor: Actor,
	now: string,
	fail: (self: ToolContext, message: string) -> any,
}

type ToolDef = {
	name: string,
	title: string?,
	description: string?,
	input: { [string]: Schema }?,
	readOnly: boolean?,
	run: (input: any, ctx: ToolContext) -> any,
}

type Notification = { title: string, body: string?, at: string?, actions: { any }? }

type Timers = {
	at: (time: string | number, body: () -> ()) -> (),
	every: (seconds: number, body: () -> ()) -> (),
}

-- ── Legacy (agentos-luau before wave A4): removed when the host drops `app.data`, `app.tools`, `app.on` ─────────
type LegacyExec = { changes: number, last_insert_rowid: number }

type LegacyData = {
	query: (sql: string, params: { Json }?) -> { { [string]: Json } },
	exec: (sql: string, params: { Json }?) -> LegacyExec,
}

type LegacyToolDef = {
	name: string,
	description: string?,
	input_schema: { [string]: Json },
	handler: (args: any) -> any,
}

type LegacyTools = { register: (def: LegacyToolDef) -> () }

type LegacyState = {
	get: (key: string) -> Json,
	set: (key: string, value: Json) -> (),
}

type AppApi = {
	manifest: (manifest: Manifest) -> (),
	window: (name: string, spec: WindowSpec) -> (),
	-- shared, in-memory server state: reads in render are tracked, assignments schedule a re-render
	state: (<T>(initial: T) -> T) & LegacyState,
	kv: KvApi,
	db: Db,
	tool: (def: ToolDef) -> (),
	t: SchemaBuilders,
	openWindow: (self: AppApi, name: string, params: { [string]: any }?) -> (),
	windows: (self: AppApi, name: string) -> { WindowContext },
	notify: ((notification: { title: string, body: string? }) -> ()) & ((self: AppApi, notification: Notification) -> ()),
	timer: Timers,
	log: (message: string) -> (),
	-- legacy
	id: string,
	data: LegacyData,
	tools: LegacyTools,
	on: (event: string, handler: (payload: any) -> any) -> (),
}

declare app: AppApi
