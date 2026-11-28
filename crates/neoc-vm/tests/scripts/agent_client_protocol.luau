--!strict
-- vnd:agent_client_protocol as a script sees it: its answers (agent_client_protocol.json), then what it asserts.
-- A connection runs over a lib:websocket socket, and no script module serves one, so this is the module's surface
-- and its types; a whole session against an agent is the crate's own tests
-- (`crates/neoc-vm-vnd/agent_client_protocol/src/tests.rs`). Nothing reaches the network.
local acp = require("vnd:agent_client_protocol")
local out = {}

-- the roles as the SDK names them: a builder, then a transport to connect it over
local builder = acp.Client.v1():name("scripts")
out.surface = { type(builder.connect), type(acp.Lines.new) }

-- a transport is a socket and nothing else
local function wrong(...: any): string
  local ok, message = pcall(acp.Lines.new :: any, ...)
  assert(not ok)
  -- the first line: a traceback follows it
  return (assert(tostring(message):match("^[^\n]*")))
end
out.wrong = { wrong(nil), wrong("ws://127.0.0.1:1/acp") }

-- what a session's `read_update` gives, told apart by `type`: the declarations narrow it
local function describe(message: vnd_agent_client_protocol_SessionMessage): string
  if message.type == "update" then
    local update = message.update
    if update.type == "agent_message_chunk" and update.content.type == "text" then
      return "text " .. update.content.text
    elseif update.type == "tool_call" then
      return "tool " .. update.title .. " " .. (update.kind or "?")
    elseif update.type == "other" then
      return "other " .. update.session_update
    end
    return update.type
  elseif message.type == "request_permission" then
    return "permission " .. message.id .. " " .. message.options[1].option_id
  end
  return "done " .. message.stop_reason
end
out.messages = {
  describe { type = "update", update = { type = "agent_message_chunk", content = { type = "text", text = "hi" } } },
  describe { type = "update", update = { type = "tool_call", tool_call_id = "c", title = "Edit", kind = "edit" } },
  describe { type = "update", update = { type = "other", session_update = "usage_update", update = {} } },
  describe {
    type = "request_permission",
    id = "7",
    tool_call = { tool_call_id = "c" },
    options = { { option_id = "yes", name = "Allow", kind = "allow_once" } },
  },
  describe { type = "prompt_response", stop_reason = "end_turn" },
}

return out
