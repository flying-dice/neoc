-- Run by agui.test.ts: one run against the agent at AGUI_URL, and every event it yields, a line of JSON each, as
-- lib:agui gives it to a script.
local agui = require("lib:agui")
local env = require("std:env")
local json = require("vnd:serde_json")

local run = assert(agui.run({
  url = assert(env.var("AGUI_URL")),
  input = {
    thread_id = "thread-1",
    run_id = "run-1",
    messages = { { id = "u1", role = "user", content = assert(env.var("AGUI_SAY")) } },
    tools = { { name = "notes__add", description = "Add a note", parameters = { type = "object" } } },
    state = { count = 1 },
  },
}))
while true do
  local event, err = run:next()
  if err then
    print("ERROR " .. err)
    break
  end
  if not event then
    break
  end
  local line = assert(json.to_string(event))
  print(line)
end
