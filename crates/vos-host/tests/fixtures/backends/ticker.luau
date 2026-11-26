--!strict
-- Sample backend for the background-work test: a counter that a task of the app raises on its own, every 0.2 s, for
-- as long as the app is loaded. The work starts at load without holding it up (`task.spawn` at the top level), it
-- tells the open pages about every tick by itself (`RPC.send_notification`), it goes on when no page is open, and it stops when
-- the app is unloaded.
local RPC = require("vos:rpc")
local task = require("std:task")

local ticks = 0
local running = true

-- the background loop: it is the app's own, so it is stopped when the app is unloaded
task.spawn(function()
  while true do
    task.wait(0.2)
    if running then
      ticks += 1
      RPC.send_notification("ticked", { ticks = ticks })
    end
  end
end)

RPC.on_request("ticks", function()
  return { ticks = ticks, running = running }
end)

-- what the page calls
RPC.on_request("_state", function()
  return { ticks = ticks, running = running }
end)

RPC.on_request("_toggle", function()
  running = not running
  return { running = running }
end)

RPC.on_request("_reset", function()
  ticks = 0
  return { ticks = ticks }
end)

RPC.on_request("_fail", function()
  error("the Fail button always fails")
end)
