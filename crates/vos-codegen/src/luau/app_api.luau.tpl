-- ===== vos:app =====
-- `local app = require("vos:app")` in app code. The host provides it natively (one VM per app); it is also a
-- global named `app`.

export type vos_ManifestWindow = { w: number, h: number, minW: number, minH: number }

export type vos_Manifest = {
	id: string,
	name: string,
	icon: string,
	tint: vos_PaletteToken?,
	kit: string?,
	window: vos_ManifestWindow,
}

export type vos_WindowSize = { w: number, h: number }

--- `w.client.search` is a Ref to client-managed state; `w.client:get(name)` reads a synced entry (handlers only).
export type vos_WindowClient = {
	get: (self: vos_WindowClient, name: string) -> any,
	set: (self: vos_WindowClient, name: string, value: any) -> (),
	[string]: vos_Ref<any>,
}

export type vos_ToastOptions = {
	text: string,
	icon: string?,
	kind: ("info" | "success" | "error")?,
	action: { title: string, onTap: (() -> ())? }?,
	duration: number?,
}

export type vos_WindowContext = {
	id: string,
	params: { [string]: any },
	--- per-window server state: a value, tracked when read in render
	state: { [string]: any },
	client: vos_WindowClient,
	--- Refs: sizeClass, heightClass, locale, timeZone, scheme, now
	env: { [string]: vos_Ref<any> },
	toast: (self: vos_WindowContext, options: vos_ToastOptions) -> (),
	focus: (self: vos_WindowContext, key: string) -> (),
	scrollTo: (self: vos_WindowContext, key: string) -> (),
	announce: (self: vos_WindowContext, text: string) -> (),
	copy: (self: vos_WindowContext, text: string) -> (),
	close: (self: vos_WindowContext) -> (),
	openWindow: (self: vos_WindowContext, name: string, params: { [string]: any }?) -> (),
}

export type vos_WindowSpec = {
	title: string?,
	size: vos_WindowSize?,
	client: { [string]: vos_ClientDecl }?,
	state: { [string]: any }?,
	render: (w: vos_WindowContext) -> vos_Node,
	onOpen: ((w: vos_WindowContext, params: { [string]: any }) -> ())?,
	onClose: ((w: vos_WindowContext) -> ())?,
}

export type vos_KvApi = {
	get: (self: vos_KvApi, key: string) -> any,
	set: (self: vos_KvApi, key: string, value: any) -> (),
	delete: (self: vos_KvApi, key: string) -> (),
}

export type vos_ExecResult = { changes: number, last_insert_id: number }

export type vos_SqlParams = { [string]: any }

--- `key` defaults to "id". A windowed query (`page`) needs a total order: end its `order by` with the key.
export type vos_QueryOptions = {
	key: string?,
	page: number?,
	total: ("exact" | "estimate" | "none")?,
}

export type vos_WriteContext = { now: string, actor: vos_Actor }

export type vos_RowOptions = {
	key: string?,
	rev: string?,
	write: ((key: any, field: string, value: any, ctx: vos_WriteContext) -> ())?,
	conflict: ("merge" | "reject")?,
	--- the fields a two-way binding may write (default: any)
	writable: { string }?,
}

--- A live source handle. The SQL never leaves the host; the tree carries only the source id.
export type vos_RowsSource = {
	["$source"]: string,
	--- how many rows match
	total: vos_Ref<number>,
}

export type vos_RowSource = {
	["$source"]: string,
	bind: (self: vos_RowSource, field: string) -> vos_Ref<any>,
	[string]: vos_Ref<any>,
}

export type vos_ScalarSource = { ["$source"]: string }

--- A chat thread source for `ChatTranscript`: which thread, and the agent it runs as (literals or Refs). The
--- thread's data is not a query: the client streams it from the host's `/agui`.
export type vos_ThreadParams = {
	thread: string | vos_Ref<string>,
	agent: string | vos_Ref<string>,
}

export type vos_ThreadSource = { ["$source"]: string }

--- What an app may know of an agent contact.
export type vos_AgentCard = { id: string, name: string, avatar: string?, role: string }

export type vos_AgentsApi = {
	list: (self: vos_AgentsApi) -> { vos_AgentCard },
}

export type vos_Db = {
	migrate: (self: vos_Db, steps: { [number]: string }) -> (),
	exec: (self: vos_Db, sql: string, params: vos_SqlParams?) -> vos_ExecResult,
	all: (self: vos_Db, sql: string, params: vos_SqlParams?) -> { { [string]: any } },
	one: (self: vos_Db, sql: string, params: vos_SqlParams?) -> { [string]: any }?,
	--- The statements inside commit together or not at all; open views see one consistent change.
	tx: <T>(self: vos_Db, body: () -> T) -> T,
	query: (self: vos_Db, sql: string, options: vos_QueryOptions?) -> (params: vos_SqlParams?) -> vos_RowsSource,
	row: (self: vos_Db, sql: string, options: vos_RowOptions?) -> (params: vos_SqlParams?) -> vos_RowSource,
	scalar: (self: vos_Db, sql: string) -> (params: vos_SqlParams?) -> vos_ScalarSource,
}

--- A field is required unless it is `optional` or has a `default`.
export type vos_SchemaOptions = {
	description: string?,
	default: any?,
	min: number?,
	max: number?,
	minLength: number?,
	maxLength: number?,
	optional: boolean?,
}

export type vos_Schema = { __schema: string }

export type vos_SchemaBuilders = {
	string: (options: vos_SchemaOptions?) -> vos_Schema,
	integer: (options: vos_SchemaOptions?) -> vos_Schema,
	number: (options: vos_SchemaOptions?) -> vos_Schema,
	boolean: (options: vos_SchemaOptions?) -> vos_Schema,
	array: (items: vos_Schema, options: vos_SchemaOptions?) -> vos_Schema,
	object: (fields: { [string]: vos_Schema }, options: vos_SchemaOptions?) -> vos_Schema,
	enum: (values: { string }, options: vos_SchemaOptions?) -> vos_Schema,
}

export type vos_ToolContext = {
	actor: vos_Actor,
	now: string,
	fail: (self: vos_ToolContext, message: string) -> any,
}

export type vos_ToolDef = {
	--- a local identifier: [a-z][a-z0-9_]* (agents see it as `<appId>__<name>`)
	name: string,
	title: string?,
	description: string?,
	input: { [string]: vos_Schema }?,
	--- enforced: the tool runs with app writes blocked (db writes, app.kv set/delete, db:migrate raise) and an agent may run it without a permission prompt
	readOnly: boolean?,
	run: (input: any, ctx: vos_ToolContext) -> any,
}

export type vos_Notification = { title: string, body: string?, at: string?, actions: { any }? }

export type vos_Timers = {
	at: (time: string | number, body: () -> ()) -> (),
	every: (seconds: number, body: () -> ()) -> (),
}

export type vos_MenuCommand = { id: string, title: string, shortcut: string?, run: () -> () }

--- The desktop shell's side of the app: declarations only until the shell implements them.
export type vos_Shell = {
	--- a string, a number (0 clears) or nil
	badge: (value: (string | number)?) -> (),
	command: (def: vos_MenuCommand) -> (),
	--- e.g. "note/:id"
	deepLink: (pattern: string, handler: (params: { [string]: string }) -> ()) -> (),
}

export type vos_ToolRegistry = { register: (def: vos_ToolDef) -> () }

export type vos_AppApi = {
	id: string,
	kv: vos_KvApi,
	db: vos_Db,
	--- the agent contacts
	agents: vos_AgentsApi,
	t: vos_SchemaBuilders,
	shell: vos_Shell,
	timer: vos_Timers,
	--- the same as `app.tool`
	tools: vos_ToolRegistry,
	manifest: (manifest: vos_Manifest) -> (),
	window: (name: string, spec: vos_WindowSpec) -> (),
	--- Shared, in-memory server state: reads in render are tracked, assignments schedule a re-render.
	state: <T>(initial: T) -> T,
	tool: (def: vos_ToolDef) -> (),
	--- Declare a chat thread source once; `chat { thread = id, agent = id }` makes the handle a `ChatTranscript` takes.
	thread: () -> ((params: vos_ThreadParams) -> vos_ThreadSource),
	openWindow: (self: vos_AppApi, name: string, params: { [string]: any }?) -> (),
	--- The open instances of the declared window `name`, in the order they were opened, each the `w` its `render`
	--- got. An undeclared name raises `app:windows: no window ... is declared`. Listing is a read, so a `readOnly`
	--- tool may do it; changing a window through its `w` is not.
	windows: (self: vos_AppApi, name: string) -> { vos_WindowContext },
	--- `app.notify { ... }` or `app:notify { ... }`.
	notify: ((notification: vos_Notification) -> ()) & ((self: vos_AppApi, notification: vos_Notification) -> ()),
	log: (message: string) -> (),
}

