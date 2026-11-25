--!strict
-- The Telemetry app's backend (NEOC-522 proof of concept). The app reads what the host recorded through its host page
-- (`vos/telemetry/query`), not through here; this only answers `probe`, a call to try tracing with: it does its work in
-- a span of its own (`lib:tracing`) and writes events in it, so both are in the trace of the page's call.
local rpc = require("../.app/rpc")
local tracing = require("lib:tracing")

local handlers: rpc.Handlers = {
  probe = function(_ctx, args)
    local fail = args.fail == true
    local units = tracing.span("count units", { fail = fail }):in_scope(function()
      tracing.info("probe answered", { units = 3 })
      if fail then
        tracing.error("probe failing on purpose")
        error("the probe failed on purpose")
      end
      return 3
    end)
    return { units = units }
  end,
}

return handlers
