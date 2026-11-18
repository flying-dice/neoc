-- AgentOS app API: type definitions for app authors, consumed by `luau-lsp analyze --definitions`
-- (`bun run lint:luau`) and by editors via luau-lsp's `definitionFiles` setting.
--
-- Plain table types and `declare app:`, not `declare class`: the `class` keyword changed meaning in newer
-- Luau releases (`declare extern type`), while aliases parse the same in every luau-lsp version.
-- Wave A2 generates this file from the Rust schema; until then it is maintained by hand, and
-- `bun run lint:luau` includes a negative check so a broken file cannot pass silently.

type JsonValue = nil | boolean | number | string | { JsonValue } | { [string]: JsonValue }

type ExecResult = {
	changes: number,
	last_insert_rowid: number,
}

type AppData = {
	query: (sql: string, params: { JsonValue }?) -> { { [string]: JsonValue } },
	exec: (sql: string, params: { JsonValue }?) -> ExecResult,
}

type ToolDef = {
	name: string,
	description: string?,
	input_schema: { [string]: JsonValue },
	handler: (args: any) -> any,
}

type AppTools = {
	register: (def: ToolDef) -> (),
}

type AppState = {
	get: (key: string) -> JsonValue,
	set: (key: string, value: JsonValue) -> (),
}

type AppApi = {
	id: string,
	data: AppData,
	tools: AppTools,
	state: AppState,
	on: (event: string, handler: (payload: any) -> any) -> (),
	notify: (n: { title: string, body: string? }) -> (),
	log: (msg: string) -> (),
}

declare app: AppApi
