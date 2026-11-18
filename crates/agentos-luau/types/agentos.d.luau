-- AgentOS app API: shipped to app authors; consumed by luau-analyze / luau-lsp (--definitions).
-- In the real host this file is GENERATED from the same Rust source of truth as the protocol types.

export type JsonValue = nil | boolean | number | string | { JsonValue } | { [string]: JsonValue }

declare class ExecResult
	changes: number
	last_insert_rowid: number
end

declare class AppData
	query: (sql: string, params: { JsonValue }?) -> { { [string]: JsonValue } }
	exec: (sql: string, params: { JsonValue }?) -> ExecResult
end

type ToolDef = {
	name: string,
	description: string?,
	input_schema: { [string]: JsonValue },
	handler: (args: any) -> any,
}

declare class AppTools
	register: (def: ToolDef) -> ()
end

declare class AppState
	get: (key: string) -> JsonValue
	set: (key: string, value: JsonValue) -> ()
end

declare class AppApi
	id: string
	data: AppData
	tools: AppTools
	state: AppState
	on: (event: string, handler: (payload: any) -> any) -> ()
	notify: (n: { title: string, body: string? }) -> ()
	log: (msg: string) -> ()
end

declare app: AppApi
