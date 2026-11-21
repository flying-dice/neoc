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

--- An entry of `app.files`: `kind` does not follow a symbolic link; `modified` is seconds since the Unix epoch
--- (`nil` where the filesystem does not record it).
export type vos_FileInfo = {
	name: string,
	kind: "file" | "dir" | "symlink" | "other",
	size: number,
	modified: number?,
}

--- One managed location of the app (`app.files.documents`, `.cache`, `.temp`). A path is relative to it,
--- written with `/` (`"notes/a.txt"`; `""` is the location itself); an absolute path, `..`, `:`, a backslash,
--- NUL or a name over 255 bytes is `invalid_input`, and a symbolic link inside it is never followed. A failure
--- returns `nil, "files.<method>: <kind>: [<path>: ]<message>"` (no path when the path itself is refused), with
--- the kinds of `std:fs` (`app.files.errorKind` reads it). A write inside a `readOnly` tool or a render raises.
--- A call holds the app's turn until it ends. Scoped to the app by convention: not a sandbox.
export type vos_Files = {
	--- The whole file (binary-safe); at most 64 MiB (`too_large`). A directory is `is_directory`.
	read: (self: vos_Files, path: string) -> (string?, string?),
	--- Create or replace the file in one step (a reader never sees half of it). The parent must exist (`not_found`).
	write: (self: vos_Files, path: string, data: string) -> (boolean?, string?),
	--- The entries of a directory (default: the location), sorted by name.
	list: (self: vos_Files, path: string?) -> ({ vos_FileInfo }?, string?),
	--- What `path` is: type, size, modification time.
	info: (self: vos_Files, path: string) -> (vos_FileInfo?, string?),
	--- Create a directory and the missing ones above it; one already there is not an error.
	createDir: (self: vos_Files, path: string) -> (boolean?, string?),
	--- Remove a file, or a directory: empty (`not_empty` otherwise) unless `{ recursive = true }`.
	remove: (self: vos_Files, path: string, options: { recursive: boolean? }?) -> (boolean?, string?),
	--- Rename or move inside the location; an existing file at `to` is replaced.
	move: (self: vos_Files, from: string, to: string) -> (boolean?, string?),
	--- Copy a file (an existing file at `to` is replaced) or a directory tree (`to` must not exist).
	copy: (self: vos_Files, from: string, to: string) -> (boolean?, string?),
}

--- The app's managed files: one shared set per app (not per person or device), found from the app's identity.
--- `documents` for what the person would miss, `cache` for what the app can rebuild, `temp` for scratch files.
--- All three are deleted when the app's data is deleted (uninstall with purge, "delete data"), and kept otherwise.
export type vos_FilesApi = {
	documents: vos_Files,
	cache: vos_Files,
	temp: vos_Files,
	--- The kind of an error a file call returned (`"not_found"`, `"already_exists"`, ...), or nil for anything else.
	errorKind: (err: string?) -> string?,
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
	--- enforced: the tool runs with app writes blocked (db writes, app.kv set/delete, db:migrate, app.files writes raise) and an agent may run it without a permission prompt
	readOnly: boolean?,
	run: (input: any, ctx: vos_ToolContext) -> any,
}

export type vos_Notification = { title: string, body: string?, at: string?, actions: { any }? }

--- The desktop shell's side of the app: a deep link is accepted and dropped until the shell shows it.
export type vos_Shell = {
	--- e.g. "note/:id"
	deepLink: (pattern: string, handler: (params: { [string]: string }) -> ()) -> (),
}

export type vos_ToolRegistry = { register: (def: vos_ToolDef) -> () }

export type vos_AppApi = {
	id: string,
	kv: vos_KvApi,
	db: vos_Db,
	files: vos_FilesApi,
	--- the agent contacts
	agents: vos_AgentsApi,
	t: vos_SchemaBuilders,
	shell: vos_Shell,
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

