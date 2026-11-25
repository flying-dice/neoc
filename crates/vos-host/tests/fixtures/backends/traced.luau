--!strict
-- Sample backend for the telemetry test (NEOC-521 POC): a handler that opens a span of its own (`lib:tracing`), writes
-- an event in it, starts a task in it, and logs through `lib:log`, so each can be found in the trace of the page's call.
local log = require("lib:log")
local task = require("std:task")
local tracing = require("lib:tracing")

local handlers: vos_app_Handlers = {
  overview = function()
    local units = tracing.span("list units", { scope = "system" }):in_scope(function()
      tracing.info("listed units", { units = 3 })
      task.spawn(function()
        tracing.info("counted in the background")
      end)
      return 3
    end)
    log.info("read the overview", { units = units })
    return { units = units }
  end,
}

return handlers
