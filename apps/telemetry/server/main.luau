--!strict
-- The Telemetry app's backend (NEOC-522 proof of concept). The app reads what the host recorded through its host page
-- (`vos/telemetry/query`), not through here; this only answers `_probe`, a call to try tracing with: it does its work in
-- a span of its own (`lib:tracing`) and writes events in it, so both are in the trace of the page's call. It serves on
-- the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`; `_probe` is private, the page's own.
local tracing = require("lib:tracing")

RPC.on_request("_probe", function(params: telemetry_ProbeParams): telemetry_ProbeResult
  local fail = params.fail == true
  local units = tracing.span("count units", { fail = fail }):in_scope(function()
    tracing.info("probe answered", { units = 3 })
    if fail then
      tracing.error("probe failing on purpose")
      error("the probe failed on purpose")
    end
    return 3
  end)
  return { units = units }
end)
