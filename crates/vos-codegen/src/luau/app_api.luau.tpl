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
--- the kinds of `std:fs` (`app.files.errorKind` reads the kind, `app.files.errorMessage` what follows it). A write
--- inside a `readOnly` tool raises.
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
	--- The message of an error a file call returned: everything after the kind (`"notes/a.txt: ..."`: the path, when
	--- the error is about one, then the wording), or nil for anything else. To show or log as it is; branch on
	--- `errorKind`.
	errorMessage: (err: string?) -> string?,
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

--- A tool of a running app, as an agent is shown it.
export type vos_HostTool = {
	--- `<appId>__<tool>`: what `app.tools.call` takes
	name: string,
	--- the app it belongs to
	app: string,
	title: string?,
	description: string,
	--- the JSON Schema of its input
	input_schema: { [string]: any },
	--- it changes nothing: it runs with every write refused
	read_only: boolean,
}

export type vos_ToolRegistry = {
	register: (def: vos_ToolDef) -> (),
	--- Every tool of every running app (this one's too), in name order: what the app can offer an agent of its own.
	all: () -> { vos_HostTool },
	--- Run a tool of a running app for an agent of this app's own: `agent` is who the tool is told called it (its
	--- `ctx.actor`), an id and a name of 1 to 200 bytes each. Nothing checks what that agent may call: the app decides
	--- whom it offers the tools to. The result is `(result, nil)`, or `(nil, message)` when the tool failed, does not
	--- exist or its app is not running; a name that is not `<appId>__<tool>` raises. A `readOnly` tool may not call one.
	call: (name: string, args: { [string]: any }?, agent: { id: string, name: string }) -> (any, string?),
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


-- ===== vos:docker =====
-- `local docker = require("vos:docker")`: the container framework, over `vnd:bollard`. The host carries it as Luau and
-- runs it in the app's own VM, so the Docker an app chose and the clients that reach it are that app's alone. It keeps
-- the choice in `app.kv` under `host`.

--- Whether Docker can be reached, as a page shows it: its version when it can, why not (`problem`, a sentence) when
--- it cannot. `host` is the chosen address; empty for this machine's own Docker.
export type vos_DockerStatus = { reachable: boolean, version: string, host: string, problem: string }

--- The end of what a container wrote, as text fit to show. `truncated`: there was more before it.
export type vos_DockerLogs = { text: string, truncated: boolean }

--- A port of the container, published on the machine's `host` port: TCP, or UDP when `udp`.
export type vos_DockerPort = { host: number, container: number, udp: boolean? }

--- A folder of the machine (`source`, a whole path) the container may read at `target`. It is never writable.
export type vos_DockerMount = { source: string, target: string }

--- What a container an app owns is made from. Everything it gets of the machine is here: nothing is published or
--- mounted that is not declared.
export type vos_DockerDeclaration = {
	image: string,
	entrypoint: { string }?,
	command: { string }?,
	--- `NAME=value`, each
	environment: { string }?,
	ports: { vos_DockerPort }?,
	mounts: { vos_DockerMount }?,
}

--- How an owned container is doing. `current`: it was made from the declaration it was compared with. `problem`: why
--- it could not be made or started the last time that was tried; empty when it could.
export type vos_DockerContainerStatus = {
	state: "absent" | "running" | "restarting" | "stopped",
	status: string,
	current: boolean,
	problem: string,
}

--- One container the app owns, found by a label that carries the app's id (`vos.app=<app.id>`), never by its name.
export type vos_DockerContainer = {
	--- Docker's filters for what happens to the container, for `docker.watch`.
	filters: { [string]: any },
	--- How the container is doing, and whether it is what `declared` says (never, for none). With Docker away it is
	--- `absent`.
	status: (self: vos_DockerContainer, declared: vos_DockerDeclaration?) -> vos_DockerContainerStatus,
	--- Have the container be what `declared` says: its image pulled when Docker has none, made and started; removed for
	--- none. One that is that already, and runs, is left running unless `always`. It is never changed: a change of the
	--- declaration is the container removed and made again. Answers with why it could not, as a sentence (a declaration
	--- that is not one is refused before Docker hears of it), and with nothing when it did.
	apply: (self: vos_DockerContainer, declared: vos_DockerDeclaration?, always: boolean?) -> string?,
	--- The end of what the container wrote (`docker.logs`). Nothing, and no reason, when there is no container.
	logs: (self: vos_DockerContainer, tail: number?) -> (vos_DockerLogs?, string?),
}

export type vos_Docker = {
	--- The chosen Docker's address; empty for this machine's own.
	host: () -> string,
	--- Choose the Docker the app uses: `unix:///...`, `tcp://...`, `http://...` or `https://...`, or empty for this
	--- machine's own. Answers with why the address was refused, and with nothing when it was taken (a Docker that is not
	--- running there is a fine choice: `status` says so, and it may start later).
	use: (host: string) -> string?,
	--- Whether Docker can be reached. Never fails: a Docker that is away is an answer.
	status: () -> vos_DockerStatus,
	--- Why a `vnd:bollard` call failed, as a sentence for the person: Docker's own message when it has one, the
	--- framework's words when Docker cannot be reached or is not answering.
	sentence: (err: string?) -> string,
	--- The client that reads what Docker has (it waits 5 seconds for an answer), or why there is none, as a sentence.
	reader: () -> (vnd_bollard_Client?, string?),
	--- The client that changes it (90 seconds).
	actor: () -> (vnd_bollard_Client?, string?),
	--- The client that pulls an image (10 minutes).
	puller: () -> (vnd_bollard_Client?, string?),
	--- The end of what a container wrote, by id or name: its output and its errors together, at most 256 KiB, as valid
	--- UTF-8 without a terminal's colour sequences and control characters. `tail` is how many lines from the end (200
	--- when left out, at most 10000).
	logs: (container: string, tail: number?) -> (vos_DockerLogs?, string?),
	--- Watch Docker for as long as the app is loaded, and call `changed` whenever what it has of `filters` (Docker's own,
	--- for its events) may be different: something happened to a container, Docker came or went, or another Docker was
	--- chosen. A Docker that is away is looked for again every 5 seconds. It never returns:
	--- `task.spawn(docker.watch, filters, changed)`.
	watch: (filters: { [string]: any }, changed: () -> ()) -> (),
	--- The container named `name` (a new one is given that name) that is this app's.
	container: (name: string) -> vos_DockerContainer,
}
