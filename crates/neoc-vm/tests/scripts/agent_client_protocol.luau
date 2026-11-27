--!strict
-- vnd:agent_client_protocol as a script sees it: its answers (agent_client_protocol.json), then what it asserts.
-- No script module serves a WebSocket, so this is the module's failures and its types; a whole turn against an agent
-- is the crate's own tests (`crates/neoc-vm-vnd/agent_client_protocol/src/tests.rs`). Nothing reaches the network:
-- an HTTP server in this process, on a loopback port the operating system picks, refuses the upgrade.
local acp = require("vnd:agent_client_protocol")
local hyper = require("vnd:hyper")
local net = require("std:net")
local out = {}

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
  if req.path == "/acp" and req:header("origin") ~= nil then
    return { status = 403, body = "origin" }
  end
  return { status = 404, body = "no such agent" }
end))
local base = "ws://" .. assert(server:addr())

-- a failure is `(nil, err)` with a kind, and what the kind carries
local function kind(url: string, opts: vnd_agent_client_protocol_ConnectOptions?): string
  local conn, err = acp.connect(url, opts)
  assert(conn == nil and err ~= nil)
  return err.kind
end
local none, refused = acp.connect(base .. "/acp?agentId=nobody")
assert(none == nil and refused ~= nil and refused.kind == "status")
out.status = { refused.code, refused.body, tostring(refused) } :: { any }
local _, forbidden = acp.connect(base .. "/acp", { headers = { Origin = "http://elsewhere" } })
out.forbidden = forbidden and forbidden.kind == "status" and forbidden.code
out.failures = {
  kind("http://127.0.0.1:1/acp"),
  kind("not a url"),
  kind(base .. "/acp", { timeout = 0 }),
  kind(base .. "/acp", { headers = { ["bad name"] = "x" } }),
}

-- a wrong argument is refused before anything is sent, with its place
local function wrong(...: any): string
  local ok, message = pcall(acp.connect :: any, ...)
  assert(not ok)
  -- the first line: a traceback follows it
  return (assert(tostring(message):match("^[^\n]*")))
end
out.wrong = {
  wrong(nil),
  wrong(base .. "/acp", { origin = "x" }),
}

-- what a turn gives, told apart by `type`: the declarations narrow it
local function describe(event: vnd_agent_client_protocol_TurnEvent): string
  if event.type == "update" then
    local update = event.update
    if update.type == "agent_message_chunk" and update.content.type == "text" then
      return "text " .. update.content.text
    elseif update.type == "tool_call" then
      return "tool " .. update.title .. " " .. (update.kind or "?")
    elseif update.type == "other" then
      return "other " .. update.session_update
    end
    return update.type
  elseif event.type == "permission" then
    return "permission " .. event.id .. " " .. event.options[1].option_id
  end
  return "done " .. event.stop_reason
end
out.events = {
  describe { type = "update", update = { type = "agent_message_chunk", content = { type = "text", text = "hi" } } },
  describe { type = "update", update = { type = "tool_call", tool_call_id = "c", title = "Edit", kind = "edit" } },
  describe { type = "update", update = { type = "other", session_update = "usage_update", update = {} } },
  describe {
    type = "permission",
    id = "7",
    tool_call = { tool_call_id = "c" },
    options = { { option_id = "yes", name = "Allow", kind = "allow_once" } },
  },
  describe { type = "done", stop_reason = "end_turn" },
}

server:close()
return out
