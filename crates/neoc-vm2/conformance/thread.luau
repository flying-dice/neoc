-- std:thread: what both VMs offer the same way. Starting a worker was reshaped (thread.vm2.luau), and with it
-- everything a worker is needed for.
local thread = require("std:thread")

local cores = thread.available_parallelism()

return {
	type(thread.spawn),
	type(cores),
	cores >= 1,
	math.floor(cores) == cores,
}
