--!strict
-- lib:websocket as a script sees it: its answers (websocket.json), then what it asserts.
-- No script module serves a WebSocket, so this is the module's failures; a socket's messages and its close are the
-- crate's own tests (`crates/neoc-vm-lib/websocket/src/tests.rs`). Nothing reaches the network: an HTTP server in this
-- process, on a loopback port the operating system picks, refuses the upgrade.
local websocket = require("lib:websocket")
local hyper = require("vnd:hyper")
local net = require("std:net")
local out = {}

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
  if req:header("origin") ~= nil then
    return { status = 403, body = "origin" }
  end
  return { status = 404, body = "no such socket" }
end))
local base = "ws://" .. assert(server:addr())

-- a failure is `(nil, err)` with a kind, and what the kind carries
local function kind(url: string, opts: lib_websocket_ConnectOptions?): string
  local ws, err = websocket.connect(url, opts)
  assert(ws == nil and err ~= nil)
  return err.kind
end
local none, refused = websocket.connect(base .. "/socket")
assert(none == nil and refused ~= nil and refused.kind == "status")
out.status = { refused.code, refused.body, tostring(refused) } :: { any }
local _, forbidden = websocket.connect(base .. "/socket", { headers = { Origin = "http://elsewhere" } })
out.forbidden = forbidden and forbidden.kind == "status" and forbidden.code
out.failures = {
  kind("not a url"),
  kind("http://127.0.0.1/socket"),
  kind(base .. "/socket", { headers = { ["bad name"] = "x" } }),
  kind(base .. "/socket", { headers = { Upgrade = "h2c" } }),
}

-- a wrong argument is refused before anything is sent, with its place
local function wrong(...: any): string
  local ok, message = pcall(websocket.connect :: any, ...)
  assert(not ok)
  -- the first line: a traceback follows it
  return (assert(tostring(message):match("^[^\n]*")))
end
out.wrong = {
  wrong(nil),
  wrong(base .. "/socket", { origin = "x" }),
}

server:close()
return out
