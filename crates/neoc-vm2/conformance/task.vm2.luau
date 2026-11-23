-- intentional: std:task has no spawn, defer, delay or Task:await yet (mlua-ir cannot declare a function the scheduler runs as a coroutine, nor a returned value kept as it is); a wrong argument is refused with its place
local task = require("std:task")

-- waiting for mlua-ir: when these are bound, this file says what they became instead
assert(task.spawn == nil and task.defer == nil and task.delay == nil)

local ok, err = pcall(task.cancel, 42)
assert(not ok and tostring(err):find("std:task.cancel: argument #1 'task'", 1, true), tostring(err))

-- a numeric string was a number
ok, err = pcall(task.wait, "0.01")
assert(not ok and tostring(err):find("std:task.wait: argument #1 'secs': expected a number", 1, true), tostring(err))

-- the module cannot be patched
assert(not pcall(function()
	task.wait = nil
end))

return true
