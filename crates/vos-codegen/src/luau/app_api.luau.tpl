-- ===== vos:app =====
-- `local app = require("vos:app")` in an app's backend. The host provides it natively (one VM per app); it is also a
-- global named `app`. The backend's entry returns the handlers its page calls (`vos_Handlers`).

--- Who is calling: the person (`"user"`, from the app's page) or an agent (`"agent"`, through a tool).
export type vos_Actor = { kind: "user" | "agent", id: string, name: string }

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
--- the kinds of `std:fs` (`app.files.errorKind` reads it). A write inside a `readOnly` tool raises.
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
	--- The statements inside commit together or not at all.
	tx: <T>(self: vos_Db, body: () -> T) -> T,
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

--- What a handler of an app's backend gets: who is calling and when (the app's page, so `actor.kind` is `"user"`).
--- `ctx:fail(message)` ends the call with `message`, which the page's `rpc` rejects with (no file or line).
export type vos_HandlerContext = {
	actor: vos_Actor,
	now: string,
	--- Ends the call with an application error (-32010) whose message the page's `rpc` rejects with: no file or line.
	--- To fail with another JSON-RPC code, raise `error("RPC-32602 Title cannot be empty", 0)`.
	fail: (self: vos_HandlerContext, message: string) -> any,
}

--- What an app's entry returns: the handlers its page reaches with `rpc(name, args)`, as `handler(ctx, args)`. An
--- entry that returns anything else (nothing included) does not load; an app with nothing to call returns `{}`.
export type vos_Handlers = { [string]: (ctx: vos_HandlerContext, args: any) -> any }

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

--- A notification: a title and an optional body, and no other key. The host adds which app sent it.
export type vos_Notification = {
	--- 1 to 200 characters, not blank
	title: string,
	--- up to 2000 characters
	body: string?,
	--- removed: a notification is delivered now, there is no scheduling
	at: nil,
	--- removed: a notification has no buttons
	actions: nil,
}

export type vos_ToolRegistry = {
	register: (def: vos_ToolDef) -> (),
}

export type vos_AppApi = {
	id: string,
	kv: vos_KvApi,
	db: vos_Db,
	files: vos_FilesApi,
	--- the agent contacts
	agents: vos_AgentsApi,
	t: vos_SchemaBuilders,
	--- the same as `app.tool`
	tools: vos_ToolRegistry,
	tool: (def: vos_ToolDef) -> (),
	--- Open the app's window on the person's screen, or bring it forward if it is open (every connected shell is asked;
	--- with none connected nothing happens). An app has one window, the frame of its page: leave `name` out (or pass
	--- `"main"`; any other name raises). A change to the world like `notify`: a `readOnly` tool may not.
	openWindow: (self: vos_AppApi, name: string?, params: { [string]: any }?) -> (),
	--- `app.notify { title, body }` or `app:notify { ... }`: tell the person something now. It reaches the clients
	--- connected now; one that is not connected never gets it (nothing is kept for later). The host sends the app's id
	--- with it, never text from the app, and each shell client draws it as a banner above its windows with the app's name
	--- and icon from its own app list: not on a client where one of the app's windows is in front, on every other client,
	--- with or without a window open. A page with no shell (the app opened on its own) draws none. The title is shown on
	--- one line (line breaks and control characters become spaces; direction overrides and characters that draw
	--- nothing are removed, but not the joiners and variation selectors that make an emoji or a word) and both are text,
	--- never markup.
	--- Raises (each message starts `app.notify`): no table ("takes a table"), a key other than title and body
	--- ("unknown key", "unknown positional entry"), no title ("needs"), a title that is blank or over 200
	--- characters, a body over 2000, a value that is not a string, a call in a readOnly tool, and a 65th
	--- while 64 from the same call are still waiting ("already has 64 notifications waiting"). The host delivers
	--- waiting notifications whenever a call of the app ends, and when a call waits (`task.wait`, a request, a task or
	--- request handler pausing, or a tool that has notified and then works on), so a tool that notifies half way is
	--- heard then, not when it returns. The limit guards against a runaway loop; it is not a quota to plan around.
	notify: ((notification: vos_Notification) -> ()) & ((self: vos_AppApi, notification: vos_Notification) -> ()),
	log: (message: string) -> (),
	--- Tell the app's open pages something changed outside them (`host.on(name, fn)` in the page): after an
	--- agent's tool changed data, or from a background task. `name` is 1 to 64 letters, digits and `. _ : -` (not
	--- `host:...`), `data` is JSON of at most 64 KiB. A change to the world like `notify`: a `readOnly` tool may not.
	emit: (name: string, data: any?) -> (),
}

