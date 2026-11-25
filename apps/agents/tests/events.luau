--!strict
-- Run by agui.test.ts: one run against the agent at AGUI_URL, and every event it yields, a line of JSON each, as
-- lib:agui gives it to a script.
local agui = require("lib:agui")
local env = require("std:env")
local json = require("vnd:serde_json")

-- this script runs under `neoc` itself, where `print` writes a line; an app's backend, as which it is checked, has none
local say = print :: any

local said: lib_agui_Message = { id = "u1", role = "user", content = env.var("AGUI_SAY") or "" }
local run, refused = agui.run(env.var("AGUI_URL") or "", {
  thread_id = "thread-1",
  run_id = "run-1",
  messages = { said },
  tools = { { name = "notes__add", description = "Add a note", parameters = { type = "object" } } },
  state = { count = 1 },
})
if not run then
  error(tostring(refused))
end
while true do
  local event, err = run:next()
  if err then
    say(`ERROR {tostring(err)}`)
    break
  end
  if not event then
    break
  end
  say(json.to_string(event) or "")
end
