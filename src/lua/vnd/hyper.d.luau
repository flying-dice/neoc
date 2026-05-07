--!strict
--
-- Type definitions for `vnd:hyper` — paired with `src/lua/vnd/hyper.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## HTTP client
--
-- HTTP-only (no TLS). URLs must start with `http://`. Client functions are
-- async — they yield the calling coroutine until the response is ready, so
-- other coroutines keep running concurrently.
--
-- ## HTTP server
--
-- `serve(listener, handler)` mirrors hyper's `service_fn` model. The listener
-- (built by `(require("std:net")).TcpListener.bind(addr)`) is consumed by an
-- accept loop; every request is dispatched to the handler function.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    An incoming HTTP request, as seen by the handler function passed to `serve`.
]=]
export type Request = {
    --[=[
        HTTP method in uppercase, e.g. `"GET"`, `"POST"`, `"PUT"`.
    ]=]
    method: string,

    --[=[
        Request path, e.g. `"/api/users"`. Does not include the query string.
    ]=]
    path: string,

    --[=[
        Query string, e.g. `"?id=1&page=2"`, or `""` if absent.
    ]=]
    query: string,

    --[=[
        Request headers as a flat table keyed by lowercase header name.
        Multi-value headers retain the last value seen.
    ]=]
    headers: {[string]: string},

    --[=[
        Raw request body bytes as a string. Empty string for methods without
        a body (GET, HEAD, etc.).
    ]=]
    body: string,

    --[=[
        Parsed JSON body. Present only when the `Content-Type` header is
        `application/json` and the body is valid JSON; `nil` otherwise.
        Returns a `JsonObject` or `JsonArray` handle (see `lib:json`).
    ]=]
    json: any?,
}

--[=[
    Response table returned by the handler function.
]=]
export type Response = {
    --[=[
        HTTP status code, e.g. `200`, `404`, `500`.
    ]=]
    status: number,

    --[=[
        Optional response headers. Defaults to an empty table.
    ]=]
    headers: ({[string]: string})?,

    --[=[
        Response body as a string. May be binary. Defaults to `""`.
    ]=]
    body: string?,
}

--[=[
    HTTP response returned by the client functions (`get`, `post`, `request`).
]=]
export type ClientResponse = {
    --[=[
        HTTP status code.
    ]=]
    status: number,

    --[=[
        Response headers as a flat table keyed by lowercase header name.
    ]=]
    headers: {[string]: string},

    --[=[
        Raw response body bytes as a Lua string.
    ]=]
    body: string,
}

--[=[
    Options for `hyper.post` and `hyper.request`.
]=]
export type RequestOpts = {
    --[=[
        Target URL. Must be `http://...` (no TLS).
    ]=]
    url: string,

    --[=[
        HTTP method for `request`. Not used by `post` (always `"POST"`).
    ]=]
    method: string?,

    --[=[
        Optional request body.
    ]=]
    body: string?,

    --[=[
        Optional request headers.
    ]=]
    headers: ({[string]: string})?,
}

--[=[
    Handle to a running HTTP server, returned by `hyper.serve`.

    ```lua
    local server = hyper.serve(listener, handler)
    print("listening on " .. server:addr())
    server:wait()  -- block until Ctrl-C or server:close()
    ```
]=]
export type ServerHandle = {
    --[=[
        Return the local address the server is bound to, e.g. `"0.0.0.0:3000"`.
    ]=]
    addr: (self: ServerHandle) -> string,

    --[=[
        Initiate graceful shutdown. Returns immediately; use `:wait()` to block
        until the server has stopped accepting connections.
    ]=]
    close: (self: ServerHandle) -> (),

    --[=[
        Block until the server stops accepting connections. Returns when
        `:close()` is called or the process-wide shutdown signal fires
        (Ctrl-C).
    ]=]
    wait: (self: ServerHandle) -> (),
}

--[=[
    The module table returned by `require("vnd:hyper")`.
]=]
export type HyperModule = {
    --[=[
        Perform an HTTP GET request.

        Returns `(ClientResponse, nil)` on success;
        `(nil, "hyper.get: ...")` on error (connection failure, non-HTTP URL, etc.).

        ```lua
        local hyper = require("vnd:hyper")
        local resp, err = hyper.get("http://example.com/")
        print(resp.status, resp.body)
        ```
    ]=]
    get: (url: string) -> (ClientResponse?, string?),

    --[=[
        Perform an HTTP POST request.

        `opts.url` is required. `opts.body` and `opts.headers` are optional.

        Returns `(ClientResponse, nil)` on success;
        `(nil, "hyper.post: ...")` on error.

        ```lua
        local resp, err = hyper.post({
            url     = "http://example.com/api",
            body    = '{"key":"value"}',
            headers = { ["content-type"] = "application/json" },
        })
        ```
    ]=]
    post: (opts: RequestOpts) -> (ClientResponse?, string?),

    --[=[
        Perform an HTTP request with an arbitrary method.

        `opts.method` and `opts.url` are required. `opts.body` and
        `opts.headers` are optional.

        Returns `(ClientResponse, nil)` on success;
        `(nil, "hyper.request: ...")` on error.

        ```lua
        local resp, err = hyper.request({
            method = "DELETE",
            url    = "http://example.com/api/users/1",
        })
        ```
    ]=]
    request: (opts: RequestOpts) -> (ClientResponse?, string?),

    --[=[
        Attach `handler` to `listener` and start accepting HTTP connections.

        `handler` is called for each request and must return a `Response`-shaped
        table with at least `status`. `body` and `headers` are optional.

        Returns `(ServerHandle, nil)` on success;
        `(nil, "hyper.serve: ...")` on error.

        ```lua
        local net   = require("std:net")
        local hyper = require("vnd:hyper")

        local listener = net.TcpListener.bind("0.0.0.0:3000")
        local server   = hyper.serve(listener, function(req)
            return { status = 200, body = "hello" }
        end)
        server:wait()
        ```
    ]=]
    serve: (listener: any, handler: (req: Request) -> Response) -> (ServerHandle?, string?),
}

-- Literal-string overload — picked up by luau-lsp so
-- `local hyper = require("vnd:hyper")` resolves `hyper` to `HyperModule`.
declare function require(name: "vnd:hyper"): HyperModule
