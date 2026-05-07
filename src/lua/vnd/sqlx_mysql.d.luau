--!strict
--
-- Type definitions for `vnd:sqlx_mysql` — paired with
-- `src/lua/vnd/sqlx_mysql.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Async
--
-- All methods are async — they yield the calling coroutine until the query
-- completes. Real network IO underneath, so yielding is worthwhile.
--
-- ## Connection pooling
--
-- The `Database` handle wraps a connection pool (default 10 connections).
-- The same handle is safe to share across coroutines that query concurrently.
--
-- ## Placeholders
--
-- MySQL uses `?` positional placeholders. Parameters follow the SQL string as
-- positional arguments. Accepted Lua types: `nil` → NULL, `boolean` → 0/1,
-- `number`, `string`.
--
-- ## TLS
--
-- Not enabled in this build. Connect to plaintext servers only. Configure
-- MySQL 8.0+ servers to allow plaintext, or run via a sidecar.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    A pre-compiled MySQL statement for repeated execution. Created by
    `db:prepare(sql)`. sqlx caches prepared statements per connection.
]=]
export type MysqlStatement = {
    --[=[
        Execute the prepared statement with positional parameters.
        Returns `(number, nil)` — rows affected — on success;
        `(nil, "sqlx_mysql.execute: ...")` on error.
    ]=]
    execute: (self: MysqlStatement, ...any) -> (number?, string?),

    --[=[
        Run a SELECT and return all rows.
        Returns `(rows, nil)` on success; `(nil, "sqlx_mysql.query: ...")` on error.
    ]=]
    query: (self: MysqlStatement, ...any) -> ({[string]: any}?, string?),

    --[=[
        Run a SELECT and return the first row or `nil` if none matched.
        Returns `(row, nil)`, `(nil, nil)` (no match), or `(nil, err)`.
    ]=]
    query_one: (self: MysqlStatement, ...any) -> ({[string]: any}?, string?),
}

--[=[
    An open MySQL connection pool. Created by `sqlx_mysql.open(url)`.

    ```lua
    local db, err = (require("vnd:sqlx_mysql")).open("mysql://root@127.0.0.1/test")
    db:execute("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(64))")
    db:execute("INSERT INTO users (id, name) VALUES (?, ?)", 1, "alice")
    local rows = db:query("SELECT * FROM users")
    db:close()
    ```
]=]
export type MysqlDatabase = {
    --[=[
        Execute a SQL statement (INSERT, UPDATE, DELETE, DDL…) with optional
        positional parameters.

        Returns `(number, nil)` — rows affected — on success;
        `(nil, "sqlx_mysql.execute: ...")` on error.
    ]=]
    execute: (self: MysqlDatabase, sql: string, ...any) -> (number?, string?),

    --[=[
        Execute a SELECT and return all result rows as a 1-indexed table of
        row tables (each keyed by column name).

        Returns `(rows, nil)` on success; `(nil, "sqlx_mysql.query: ...")` on error.
    ]=]
    query: (self: MysqlDatabase, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Execute a SELECT and return the first row, or `nil` if none matched.

        Returns `(row, nil)`, `(nil, nil)` (no rows), or `(nil, err)`.
    ]=]
    query_one: (self: MysqlDatabase, sql: string, ...any) -> ({[string]: any}?, string?),

    --[=[
        Prepare a SQL statement for repeated execution. Returns immediately
        (no network round-trip at prepare time; sqlx caches per connection).
    ]=]
    prepare: (self: MysqlDatabase, sql: string) -> MysqlStatement,

    --[=[
        Close all connections in the pool. Subsequent calls to any method
        return an error.
    ]=]
    close: (self: MysqlDatabase) -> (),
}

--[=[
    The module table returned by `require("vnd:sqlx_mysql")`.
]=]
export type SqlxMysqlModule = {
    --[=[
        Open a MySQL connection pool. `url` is a standard MySQL URL:
        `mysql://user:pass@host:3306/dbname`.

        Returns `(MysqlDatabase, nil)` on success;
        `(nil, "sqlx_mysql.open: ...")` on error (invalid URL, connection
        refused, etc.).

        ```lua
        local db, err = (require("vnd:sqlx_mysql")).open("mysql://root@127.0.0.1/test")
        ```
    ]=]
    open: (url: string) -> (MysqlDatabase?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local sqlx_mysql = require("vnd:sqlx_mysql")` resolves to `SqlxMysqlModule`.
declare function require(name: "vnd:sqlx_mysql"): SqlxMysqlModule
