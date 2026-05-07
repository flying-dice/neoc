--!strict
--
-- Type definitions for `std:net` — paired with `src/lua/std/net.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Network primitives
--
-- `TcpListener` is the building block for the HTTP server (`vnd:hyper`).
-- Bind once and pass to `hyper.serve`, or share via
-- `thread.spawn({ shared = { listener = listener } })` to fan out.
--
-- `TcpListener` implements the Sendable protocol.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Opaque handle to a bound TCP socket, ready to accept connections.

    Created by `TcpListener.bind(addr)`. Pass it to `(require("vnd:hyper")).serve`
    to start an HTTP server, or share it across workers via
    `thread.spawn({ shared = {…} })` for multi-worker fan-out.

    Implements the Sendable protocol.

    ```lua
    local TcpListener = require("std:net").TcpListener
    local listener, err = TcpListener.bind("0.0.0.0:3000")
    print(listener:addr())  -- "0.0.0.0:3000" or "0.0.0.0:<actual-port>"
    ```
]=]
export type TcpListenerHandle = {
    --[=[
        Return the local address the socket is bound to, as a string like
        `"127.0.0.1:8080"` or `"0.0.0.0:3000"`. Useful when binding to
        port 0 to discover the assigned ephemeral port.
    ]=]
    addr: (self: TcpListenerHandle) -> string,
}

--[=[
    Constructor table for `TcpListener`.
]=]
export type TcpListenerConstructor = {
    --[=[
        Bind a TCP socket to `addr` (e.g. `"0.0.0.0:3000"` or
        `"127.0.0.1:0"` for an ephemeral port).

        Returns `(TcpListenerHandle, nil)` on success;
        `(nil, "TcpListener.bind: ...")` on error (address in use, etc.).

        ```lua
        local TcpListener = require("std:net").TcpListener
        local listener, err = TcpListener.bind("0.0.0.0:8080")
        ```
    ]=]
    bind: (addr: string) -> (TcpListenerHandle?, string?),
}

--[=[
    The module table returned by `require("std:net")`.
]=]
export type NetModule = {
    --[=[
        Constructor table for `TcpListener`. Call `.bind(addr)` to create a
        bound socket handle.

        ```lua
        local listener, err = (require("std:net")).TcpListener.bind("0.0.0.0:3000")
        ```
    ]=]
    TcpListener: TcpListenerConstructor,
}

-- Literal-string overload — picked up by luau-lsp so
-- `local net = require("std:net")` resolves `net` to `NetModule`.
declare function require(name: "std:net"): NetModule
