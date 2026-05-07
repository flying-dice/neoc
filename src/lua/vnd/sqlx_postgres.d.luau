--!strict
--
-- Type definitions for `vnd:sqlx_postgres` — paired with
-- `src/lua/vnd/sqlx_postgres.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Async
--
-- All methods are async — they yield the calling coroutine until the query
-- completes.
--
-- ## Placeholders
--
-- Postgres uses **positional** placeholders `$1, $2, …` (not `?`).
-- Parameters follow the SQL string as positional arguments.
--
-- ## TLS
--
-- Not enabled in this build. Connect to plaintext servers only (consistent
-- with `vnd:hyper` being HTTP-only). Add `sslmode=disable` to the URL if the
-- server defaults to requiring TLS.
--
-- ## NULL handling
--
-- Postgres is strictly typed at bind time. To insert SQL `NULL` into a
-- non-text column, prefer the `NULL` literal in SQL (e.g. `INSERT INTO …
-- VALUES (NULL)`) rather than passing Lua `nil` as a parameter — the latter
-- binds as `Option::<i64>::None`, which the server may reject against e.g.
-- `TEXT` columns.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    An open Postgres connection pool. Created by `sqlx_postgres.open(url)`.

    ```lua
    local db, err = (require("vnd:sqlx_postgres")).open("postgres://postgres@127.0.0.1/test")
    db:execute("CREATE TABLE users (id INT PRIMARY KEY, name TEXT)")
    db:execute("INSERT INTO users (id, name) VALUES ($1, $2)", 1, "alice")
    local rows = db:query("SELECT * FROM users")
    db:close()
    ```
]=]
export type PgDatabase = {
    --[=[
        Execute a SQL statement (INSERT, UPDATE, DELETE, DDL…) with optional
        positional parameters (`$1, $2, …`).

        Returns `(number, nil)` — rows affected — on success;
        `(nil, "sqlx_postgres.execute: ...")` on error.
    ]=]
    execute: (self: PgDatabase, sql: string, ...any) -> (number?, string?),

    --[=[
        Execute a SELECT and return all rows as a 1-indexed table of row
        tables (each keyed by column name).

        Returns `(rows, nil)` on success; `(nil, "sqlx_postgres.query: ...")` on
        error.
    ]=]
    query: (self: PgDatabase, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Execute a SELECT and return the first row, or `nil` if none matched.

        Returns `(row, nil)`, `(nil, nil)` (no rows), or `(nil, err)`.
    ]=]
    query_one: (self: PgDatabase, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Close all connections in the pool. Subsequent calls return an error.
    ]=]
    close: (self: PgDatabase) -> (),
}

--[=[
    The module table returned by `require("vnd:sqlx_postgres")`.
]=]
export type SqlxPostgresModule = {
    --[=[
        Open a Postgres connection pool. `url` is a standard Postgres URL:
        `postgres://user:pass@host:5432/dbname`.

        Returns `(PgDatabase, nil)` on success;
        `(nil, "sqlx_postgres.open: ...")` on error.

        ```lua
        local db, err = (require("vnd:sqlx_postgres")).open("postgres://postgres@127.0.0.1/postgres")
        ```
    ]=]
    open: (url: string) -> (PgDatabase?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local pg = require("vnd:sqlx_postgres")` resolves to `SqlxPostgresModule`.
declare function require(name: "vnd:sqlx_postgres"): SqlxPostgresModule
