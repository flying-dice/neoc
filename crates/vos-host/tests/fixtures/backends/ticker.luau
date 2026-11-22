--!strict
-- Sample backend for the background-work test: a counter that a task of the app raises on its own, every 0.2 s, for
-- as long as the app is loaded. The work starts at load without holding it up (`task.spawn` at the top level), it
-- tells the open pages about every tick by itself (`app.emit`), it goes on when no page is open, and it stops when
-- the app is unloaded.
local app = require("vos:app")
local task = require("std:task")

local ticks = 0
local running = true

-- the background loop: it is the app's own, so it is stopped when the app is unloaded
task.spawn(function()
  while true do
    task.wait(0.2)
    if running then
      ticks += 1
      app.emit("ticked", { ticks = ticks })
    end
  end
end)

app.tool {
  name = "ticks",
  title = "Ticks",
  readOnly = true,
  description = "How many ticks the background loop has counted.",
  input = {},
  run = function()
    return { ticks = ticks, running = running }
  end,
}

-- what the page calls
local handlers: vos_Handlers = {
  state = function()
    return { ticks = ticks, running = running }
  end,
  toggle = function()
    running = not running
    return { running = running }
  end,
  reset = function()
    ticks = 0
    return { ticks = ticks }
  end,
  fail = function()
    error("the Fail button always fails")
  end,
}

return handlers
