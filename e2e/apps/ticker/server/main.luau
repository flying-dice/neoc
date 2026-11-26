--!strict
-- The backend of the fixture app of the background-work journeys (`e2e/background.journeys.spec.ts`): a counter that a
-- task of the app raises on its own, every 0.2 s, for as long as the app is loaded. The work starts at load without
-- holding it up (`task.spawn` at the top level), it goes on when the window is closed, it tells every open window of
-- each change with the `count` notification, and it stops when the app is unloaded.
local task = require("std:task")

local ticks = 0
local running = true

local function count(): ticker_Count
  return { ticks = ticks, running = running }
end

-- a change reaches the open windows at once, whoever made it (the task, or a control of another window)
local function changed(): ticker_Count
  local now = count()
  RPC.send_notification("count", now)
  return now
end

-- the background loop: it is the app's own, so it is stopped when the app is unloaded
task.spawn(function()
  while true do
    task.wait(0.2)
    if running then
      ticks += 1
      changed()
    end
  end
end)

RPC.on_request("_read", function(): ticker_ReadResult
  return count()
end)

RPC.on_request("_toggle", function(): ticker_ToggleResult
  running = not running
  return changed()
end)

RPC.on_request("_reset", function(): ticker_ResetResult
  ticks = 0
  return changed()
end)

RPC.on_request("_fail", function(): ticker_FailResult
  error("the Fail button always fails")
end)
