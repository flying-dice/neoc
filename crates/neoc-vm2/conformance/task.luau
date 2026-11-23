-- std:task: what both VMs offer. `spawn`, `defer`, `delay` and `Task:await` are neoc-vm's only for now
-- (task.vm2.luau), and without them there is no task to cancel.
local task = require("std:task")

local slept = task.wait(0.02)

return {
	type(task.wait),
	type(task.cancel),
	slept >= 0.02 and slept < 2,
	-- no wait at all for what is not a time
	task.wait(0) < 1,
	task.wait(-1) < 1,
	task.wait(0 / 0) < 1,
	-- only a task can be cancelled
	(pcall(task.cancel, 42)),
	(pcall(task.cancel, nil)),
	(pcall(task.cancel, {})),
}
