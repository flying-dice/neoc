--!strict
-- lib:agui as a script sees it: its answers (agui.json), then what it asserts. An agent in this process, on a
-- loopback port the operating system picks: it answers every run with the events of its path, and keeps what it was
-- sent. Nothing reaches the network.
local agui = require("lib:agui")
local hyper = require("vnd:hyper")
local net = require("std:net")
local out = {}

type Sent = { body: string, accept: string?, token: string? }
local received: { [string]: Sent } = {}
local script: { [string]: { string } } = {
  ["/text"] = {
    '{"type":"RUN_STARTED","threadId":"t","runId":"r"}',
    '{"type":"TEXT_MESSAGE_START","messageId":"m","role":"assistant"}',
    '{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"Hi "}',
    '{"type":"FROM_THE_FUTURE","x":1}',
    '{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"there"}',
    '{"type":"TEXT_MESSAGE_END","messageId":"m"}',
    '{"type":"RUN_FINISHED","threadId":"t","runId":"r","usage":[{"inputTokens":3}]}',
  },
  ["/activity"] = {
    '{"type":"ACTIVITY_SNAPSHOT","messageId":"a","activityType":"PLAN","content":{"steps":["read"]}}',
    '{"type":"ACTIVITY_DELTA","messageId":"a","activityType":"PLAN","patch":[{"op":"add","path":"/steps/-","value":"write"}]}',
  },
  ["/broken"] = {
    '{"type":"STEP_STARTED","stepName":"s"}',
    '{"type":"STEP_STARTED"}',
  },
}

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
  received[req.path] = { body = req.body, accept = req:header("accept"), token = req:header("authorization") }
  local events = script[req.path]
  if not events then
    return { status = 404, body = "no such agent" }
  end
  local lines = {}
  for _, event in events do
    table.insert(lines, "data: " .. event .. "\n\n")
  end
  return { status = 200, headers = { ["content-type"] = "text/event-stream" }, body = table.concat(lines) }
end))
local base = "http://" .. assert(server:addr())

local function input(): lib_agui_RunAgentInput
  local said: lib_agui_Message = { id = "u1", role = "user", content = "hello" }
  return { thread_id = "t", run_id = "r", messages = { said } }
end

-- every event of a run, which must end without an error
local function all(run: mlua_Stream<lib_agui_Event, lib_agui_RunError>): { lib_agui_Event }
  local events = {}
  while true do
    local event, err = run:next()
    assert(err == nil, tostring(err))
    if not event then
      return events
    end
    table.insert(events, event)
  end
end

-- a run sends the input and yields the agent's events; a type the module does not know is skipped
local run = assert(agui.run(base .. "/text", input(), { headers = { Authorization = "Bearer k" } }))
local text, types = "", {}
for _, event in all(run) do
  table.insert(types, event.type)
  if event.type == "TEXT_MESSAGE_START" then
    assert(event.message_id == "m" and event.role == "assistant")
  elseif event.type == "TEXT_MESSAGE_CONTENT" then
    assert(event.message_id == "m")
    text ..= event.delta
  elseif event.type == "RUN_FINISHED" then
    local usage = assert(event.usage)
    out.input_tokens = usage[1].input_tokens
  end
end
out.text = text
out.types = types
-- the stream has ended: nothing more, and no error
out.after_the_end = { run:next() } :: { any }

local sent = received["/text"]
out.accept = sent.accept
out.token = sent.token
-- the wire is camelCase
out.body = sent.body

-- an activity is a snapshot and the patches to it; a patch gives a new document and leaves the old one
local activity = all((assert(agui.run(base .. "/activity", input()))))
local snapshot, delta = activity[1], activity[2]
assert(snapshot.type == "ACTIVITY_SNAPSHOT" and delta.type == "ACTIVITY_DELTA")
out.activity = { snapshot.activity_type, snapshot.replace } :: { any }
local content = assert(agui.apply_patch(snapshot.content, delta.patch)) :: { steps: { string } }
out.patched = content.steps
out.unpatched = (snapshot.content :: { steps: { string } }).steps
out.no_patch = (assert(agui.apply_patch({ n = 1 }, {})) :: { n: number }).n

-- a patch that cannot be applied, and one that is none
local function patch_kind(patch: { unknown }): string
  local document, err = agui.apply_patch({ steps = { "a" } }, patch)
  assert(document == nil and err ~= nil)
  return err.kind
end
out.patch_failures = {
  patch_kind { { op = "remove", path = "/missing" } },
  patch_kind { { op = "test", path = "/steps/0", value = "b" } },
  patch_kind { { op = "explode" } },
}

-- a failure is `(nil, err)` with a kind, and what the kind carries
local function run_kind(url: string, opts: lib_agui_RunOptions?): string
  local none, err = agui.run(url, input(), opts)
  assert(none == nil and err ~= nil)
  return err.kind
end
local none, refused = agui.run(base .. "/nowhere", input())
assert(none == nil and refused ~= nil and refused.kind == "status")
out.status = { refused.status, refused.body, tostring(refused) } :: { any }
out.run_failures = {
  run_kind("ftp://example.com/"),
  run_kind(base .. "/text", { max_event = 0 }),
  run_kind(base .. "/text", { timeout = 0 }),
  run_kind(base .. "/text", { headers = { ["bad name"] = "x" } }),
}

-- an event that is not an AG-UI event ends the run with `protocol`, and nothing follows it
local broken = assert(agui.run(base .. "/broken", input()))
local step = assert(broken:next())
assert(step.type == "STEP_STARTED")
local nothing, why = broken:next()
assert(nothing == nil and why ~= nil)
out.broken = { step.step_name, why.kind, select("#", broken:next()) } :: { any }

-- a closed run has no more events
local closed = assert(agui.run(base .. "/text", input()))
closed:close()
closed:close()
out.closed = { closed:next() } :: { any }

-- a wrong argument is refused before anything is sent, with its place
local function wrong(...: any): string
  local ok, message = pcall(agui.run :: any, ...)
  assert(not ok)
  -- the first line: a traceback follows it
  return (assert(tostring(message):match("^[^\n]*")))
end
out.wrong = {
  wrong(base .. "/text", { thread_id = "t" }),
  wrong(base .. "/text", { thread_id = "t", run_id = "r", messages = { { id = "1", role = "robot" } } }),
  wrong(base .. "/text", input(), { follow_redirects = true }),
}

server:close()
return out
