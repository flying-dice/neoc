--!strict
--
-- Type definitions for `vnd:rusqlite` — paired with
-- `src/lua/vnd/rusqlite.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Why sync
--
-- libsqlite3 is a synchronous C library. All methods here are sync
-- (`add_method`, not `add_async_method`) — no Tokio yield, no coroutine
-- suspension. This is intentional: the overhead of async ceremony exceeds the
-- benefit for sub-µs C calls. For slow queries on a high-concurrency path,
-- wrap the call site in `tokio::task::spawn_blocking` at the Rust level.
--
-- ## Parameterised queries
--
-- Use `?` placeholders. Parameters follow the SQL string as positional
-- arguments. Accepted Lua types: `nil` → `NULL`, `boolean` → integer 0/1,
-- `number` → integer or float, `string` → TEXT or BLOB.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    A pre-compiled SQL statement. Created by `db:prepare(sql)`. Calls go
    through rusqlite's `prepare_cached` — the same path as `db:execute(sql,
    ...)`. The practical win is skipping the Lua → Rust SQL string allocation
    on each invocation.

    ```lua
    local insert = db:prepare("INSERT INTO users (name) VALUES (?)")
    insert:execute("alice")
    insert:execute("bob")
    ```
]=]
export type Statement = {
    --[=[
        Execute the statement with positional parameters.

        Returns `(number, nil)` — the number of rows affected — on success;
        `(nil, "rusqlite.execute: ...")` on error.
    ]=]
    execute: (self: Statement, ...any) -> (number?, string?),

    --[=[
        Execute a SELECT query and return all result rows.

        Returns `(rows, nil)` where `rows` is a 1-indexed table of row tables,
        each keyed by column name; `(nil, "rusqlite.query: ...")` on error.
    ]=]
    query: (self: Statement, ...any) -> ({[string]: any}?, string?),

    --[=[
        Execute a SELECT query and return the first row (or `nil` if no rows
        matched).

        Returns `(row, nil)` — a table keyed by column name — or `(nil, nil)`
        if no rows matched; `(nil, "rusqlite.query_one: ...")` on error.
    ]=]
    query_one: (self: Statement, ...any) -> ({[string]: any}?, string?),
}

--[=[
    An open SQLite database connection. Created by `rusqlite.open(path)`.
    Uses rusqlite's `prepare_cached` internally, so repeated SQL strings skip
    the parse step.

    ```lua
    local db, err = (require("vnd:rusqlite")).open(":memory:")
    db:execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
    local n, err = db:execute("INSERT INTO users (name) VALUES (?)", "alice")
    local rows, err = db:query("SELECT * FROM users")
    db:close()
    ```
]=]
export type Database = {
    --[=[
        Execute a SQL statement (INSERT, UPDATE, DELETE, DDL…).

        The first argument is the SQL string. Subsequent arguments are
        positional parameters bound to `?` placeholders in order.

        Returns `(number, nil)` — rows affected — on success;
        `(nil, "rusqlite.execute: ...")` on error.
    ]=]
    execute: (self: Database, sql: string, ...any) -> (number?, string?),

    --[=[
        Execute a SELECT statement and return all result rows as a 1-indexed
        table of row tables (each keyed by column name).

        Returns `(rows, nil)` on success; `(nil, "rusqlite.query: ...")` on
        error.
    ]=]
    query: (self: Database, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Execute a SELECT statement and return the first matching row, or `nil`
        if no rows matched.

        Returns `(row, nil)` or `(nil, nil)` (no match) or
        `(nil, "rusqlite.query_one: ...")` on error.
    ]=]
    query_one: (self: Database, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Prepare a SQL statement for repeated execution.
        Returns a `Statement` handle that shares this connection.
    ]=]
    prepare: (self: Database, sql: string) -> Statement,

    --[=[
        Close the connection. Subsequent calls to any method on this handle
        return an error.
    ]=]
    close: (self: Database) -> (),
}

--[=[
    The module table returned by `require("vnd:rusqlite")`.
]=]
export type RusqliteModule = {
    --[=[
        Open a SQLite database file. Pass `":memory:"` for an in-memory database.

        Returns `(Database, nil)` on success; `(nil, "rusqlite.open: ...")` on
        error.

        ```lua
        local db, err = (require("vnd:rusqlite")).open(":memory:")
        ```
    ]=]
    open: (path: string) -> (Database?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local rusqlite = require("vnd:rusqlite")` resolves to `RusqliteModule`.
declare function require(name: "vnd:rusqlite"): RusqliteModule
