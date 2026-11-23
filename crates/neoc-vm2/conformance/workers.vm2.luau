-- intentional: the deprecated alias shutdown_signal() is gone (it was on_shutdown() under an older name); an argument too many is refused
local workers = require("std:workers")

assert(workers.shutdown_signal == nil)

local signal = workers.on_shutdown()
assert(not signal:is_fired())

local ok, err = pcall(signal.is_fired, signal, "extra")
assert(not ok and tostring(err):find("std:workers.ShutdownSignal:is_fired", 1, true), tostring(err))

return true
