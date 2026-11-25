--!strict
-- std:workers as a script sees it: its answers (workers.json), then what it asserts.
local workers = require("std:workers")

local signal = workers.on_shutdown()

-- what it asserts
do
  assert((workers :: any).shutdown_signal == nil)

  local signal = workers.on_shutdown()
  assert(not signal:is_fired())

  local ok, err = pcall(signal.is_fired :: any, signal, "extra") -- deliberate refusal test: an argument too many
  assert(not ok and tostring(err):find("std:workers.ShutdownSignal:is_fired", 1, true), tostring(err))
end

return {
  type(workers.on_shutdown),
  type(signal),
  signal:is_fired(),
  -- every call gives the same signal
  workers.on_shutdown():is_fired(),
  type(signal.wait),
} :: { any }
