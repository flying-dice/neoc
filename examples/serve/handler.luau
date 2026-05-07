-- Multi-worker HTTP server example.
--
-- Run with:
--   cargo run -- examples/serve/handler.luau
--
-- A single main script binds the listener, then spawns worker threads
-- that share the fd via thread.spawn({ shared = {…} }). Each worker
-- is its own Lua VM racing on the same listener.

local thread      = require("std:thread")
local TcpListener = require("std:net").TcpListener
local Counter     = require("std:collections").Counter
local hyper       = require("vnd:hyper")
local json        = require("vnd:serde_json")

local listener = TcpListener.bind("0.0.0.0:3000")
local hits     = Counter.new(0)

for i = 1, 4 do
    thread.spawn({
        source = [[
            local hyper = require("vnd:hyper")
            local json  = require("vnd:serde_json")
            local shared = (require("std:thread")).shared

            local server = hyper.serve(shared.listener, function(req)
                if req.method == "GET" and req.path == "/" then
                    shared.hits:increment()
                    return "OK"
                end
                local id = req.path:match("^/users/([^/]+)$")
                if req.method == "GET" and id then
                    return {
                        status  = 200,
                        headers = { ["content-type"] = "application/json" },
                        body    = json.to_string({ id = id, hits = shared.hits:get() }),
                    }
                end
                if req.method == "POST" and req.path == "/echo" then
                    return { status = 200, body = req.body }
                end
                return { status = 404, body = "not found" }
            end)
        ]],
        shared = { listener = listener, hits = hits },
    })
end

-- Park the main thread on the shutdown signal.
local workers = require("std:workers")
workers.on_shutdown():wait()
