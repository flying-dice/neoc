--!strict
-- Sample backend for the telemetry test (NEOC-521 POC): a handler that opens a span of its own (`lib:tracing`), writes
-- an event in it, starts a task in it, and logs through `lib:log`, so each can be found in the trace of the page's call.
local log = require("lib:log")
local RPC = require("vos:rpc")
local task = require("std:task")
local tracing = require("lib:tracing")

RPC.on_request("_overview", function()
  local units = tracing.span("list units", { scope = "system" }):in_scope(function()
    tracing.info("listed units", { units = 3 })
    task.spawn(function()
      tracing.info("counted in the background")
    end)
    return 3
  end)
  log.info("read the overview", { units = units })
  return { units = units }
end)

-- three ways a call goes wrong, none of them logged by the app: each must still be a line in the logs, in the call's
-- trace. The app refuses (a reason it gives), its code fails (a bug), and work it started in the background fails
-- after the call has answered.
RPC.on_request("_refuse", function(_params: any, ctx: any)
  return ctx:fail("Docker cannot be reached at tcp://nowhere.")
end)

RPC.on_request("_crash", function()
  local missing: any = nil
  return { units = missing.count }
end)

RPC.on_request("_background", function()
  task.spawn(function()
    task.wait(0)
    error("the feed went away")
  end)
  return { units = 0 }
end)
