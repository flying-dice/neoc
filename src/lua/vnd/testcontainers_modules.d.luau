--!strict
--
-- Type definitions for `vnd:testcontainers_modules` — paired with
-- `src/lua/vnd/testcontainers_modules.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Usage
--
-- Spin up MySQL, Postgres, or any Docker image for integration tests. All
-- constructors are async — they pull the image on first use and wait for the
-- container's readiness probe before returning.
--
-- Requires Docker on the host.
--
-- ```lua
-- local tc  = require("vnd:testcontainers_modules")
-- local db  = require("vnd:sqlx_mysql")
--
-- local container = tc.mysql()
-- local conn      = db.open(container:url())
-- -- … run tests …
-- container:stop()
-- ```
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Handle to a running MySQL container. Created by `testcontainers_modules.mysql()`.
]=]
export type MysqlContainer = {
    --[=[
        MySQL connection URL pre-formatted for `sqlx_mysql.open`.
        User `root`, no password, database `test`.
    ]=]
    url: (self: MysqlContainer) -> string,

    --[=[
        Container host (always `"127.0.0.1"`).
    ]=]
    host: (self: MysqlContainer) -> string,

    --[=[
        Mapped host port for MySQL's 3306.
    ]=]
    port: (self: MysqlContainer) -> number,

    --[=[
        Stop the container. Idempotent — safe to call multiple times.
        The container also stops on garbage-collection if not stopped explicitly.
    ]=]
    stop: (self: MysqlContainer) -> (),
}

--[=[
    Handle to a running Postgres container. Created by
    `testcontainers_modules.postgres()`.
]=]
export type PostgresContainer = {
    --[=[
        Postgres connection URL pre-formatted for `sqlx_postgres.open`.
        User `postgres`, password `postgres`, database `postgres`.
    ]=]
    url: (self: PostgresContainer) -> string,

    --[=[
        Container host (always `"127.0.0.1"`).
    ]=]
    host: (self: PostgresContainer) -> string,

    --[=[
        Mapped host port for Postgres's 5432.
    ]=]
    port: (self: PostgresContainer) -> number,

    --[=[
        Stop the container. Idempotent.
    ]=]
    stop: (self: PostgresContainer) -> (),
}

--[=[
    Handle to a running generic Docker container. Created by
    `testcontainers_modules.generic(opts)`.
]=]
export type GenericContainer = {
    --[=[
        Container host (always `"127.0.0.1"`).
    ]=]
    host: (self: GenericContainer) -> string,

    --[=[
        Mapped host port for the given container port. Only ports listed in
        `opts.ports` at start time are available; requesting an unmapped port
        raises a runtime error.
    ]=]
    port: (self: GenericContainer, container_port: number) -> number,

    --[=[
        Stop the container. Idempotent.
    ]=]
    stop: (self: GenericContainer) -> (),
}

--[=[
    Wait strategy for `generic` containers.
]=]
export type WaitStrategy =
    --[=[
        Wait until the container stdout log contains `message`.
        `"log"` and `"stdout"` are both accepted.
        ```lua
        wait = { type = "log", message = "Ready to accept connections" }
        ```
    ]=]
    { type: "log" | "stdout", message: string }
    --[=[
        Wait until the container stderr log contains `message`.
        ```lua
        wait = { type = "stderr", message = "Listening on" }
        ```
    ]=]
    | { type: "stderr", message: string }
    --[=[
        Wait for the given number of seconds.
        ```lua
        wait = { type = "seconds", seconds = 2 }
        ```
    ]=]
    | { type: "seconds", seconds: number }
    --[=[
        Wait until the container's Docker healthcheck reports healthy.
        ```lua
        wait = { type = "healthcheck" }
        ```
    ]=]
    | { type: "healthcheck" }

--[=[
    Options for `testcontainers_modules.generic(opts)`.
]=]
export type GenericOpts = {
    --[=[
        Docker image name, e.g. `"redis"` or `"nginx"`.
    ]=]
    image: string,

    --[=[
        Image tag. Defaults to `"latest"`.
    ]=]
    tag: string?,

    --[=[
        Container ports to expose. Each port is mapped to a random host port
        accessible via `:port(p)`. Defaults to `{}`.
    ]=]
    ports: {number}?,

    --[=[
        Environment variables to set inside the container.
    ]=]
    env: ({[string]: string})?,

    --[=[
        Command override for the container entrypoint.
    ]=]
    cmd: ({string})?,

    --[=[
        Readiness probe. The constructor blocks until this condition is met.
    ]=]
    wait: WaitStrategy?,
}

--[=[
    The module table returned by `require("vnd:testcontainers_modules")`.
]=]
export type TestcontainersModule = {
    --[=[
        Start a MySQL container with default settings.
        Async — yields until the container is ready to accept connections.

        Returns `(MysqlContainer, nil)` on success;
        `(nil, "testcontainers_modules.mysql: ...")` on error.

        ```lua
        local tc      = require("vnd:testcontainers_modules")
        local mysql   = require("vnd:sqlx_mysql")
        local c = tc.mysql()
        local db = mysql.open(c:url())
        ```
    ]=]
    mysql: () -> (MysqlContainer?, string?),

    --[=[
        Start a Postgres container with default settings.
        Async — yields until the container is ready.

        Returns `(PostgresContainer, nil)` on success;
        `(nil, "testcontainers_modules.postgres: ...")` on error.
    ]=]
    postgres: () -> (PostgresContainer?, string?),

    --[=[
        Start a generic Docker container from any image.
        Async — yields until the container passes its readiness probe.

        Returns `(GenericContainer, nil)` on success;
        `(nil, "testcontainers_modules.generic: ...")` on error.

        ```lua
        local tc    = require("vnd:testcontainers_modules")
        local redis = tc.generic({
            image = "redis",
            tag   = "7-alpine",
            ports = { 6379 },
            wait  = { type = "log", message = "Ready to accept connections" },
        })
        print(redis:host(), redis:port(6379))
        redis:stop()
        ```
    ]=]
    generic: (opts: GenericOpts) -> (GenericContainer?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local tc = require("vnd:testcontainers_modules")` resolves to `TestcontainersModule`.
declare function require(name: "vnd:testcontainers_modules"): TestcontainersModule
