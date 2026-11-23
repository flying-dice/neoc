-- std:workers: the shutdown signal of the process, which nothing fires here.
local workers = require("std:workers")

local signal = workers.on_shutdown()

return {
	type(workers.on_shutdown),
	type(signal),
	signal:is_fired(),
	-- every call gives the same signal
	workers.on_shutdown():is_fired(),
	type(signal.wait),
}
