-- intentional: std:task's spawn, defer and delay take a function only (no arguments passed on, no coroutine); Task:await gives the first return value or nil and a typed error (cancelled | failed with a reason), not every value or a text; a wrong argument is refused with its place
local task = require("std:task")

assert(type(task.spawn) == "function" and type(task.defer) == "function" and type(task.delay) == "function")

-- one value, as it is; no error beside it
local t = {}
local value, err = task.spawn(function()
	return t, "second"
end):await()
assert(value == t and err == nil)
value, err = task.spawn(function() end):await()
assert(value == nil and err == nil)

-- arguments are the closure's business
local function add(a, b)
	return a + b
end
assert(task.spawn(function()
	return add(1, 2)
end):await() == 3)
assert(task.defer(function()
	return add(2, 3)
end):await() == 5)
assert(task.delay(0, function()
	return add(3, 4)
end):await() == 7)
local ok, refused = pcall(task.spawn, add, 1, 2)
assert(not ok and tostring(refused):find("std:task.spawn", 1, true), tostring(refused))

-- a coroutine is not a function
ok, refused = pcall(task.spawn, coroutine.create(function() end))
assert(
	not ok and tostring(refused):find("std:task.spawn: argument #1 'body': expected a function", 1, true),
	tostring(refused)
)
ok, refused = pcall(task.delay, 0, 42)
assert(
	not ok and tostring(refused):find("std:task.delay: argument #2 'body': expected a function", 1, true),
	tostring(refused)
)

-- a task that failed: typed, with what was raised
value, err = task.spawn(function()
	error("boom")
end):await()
assert(value == nil and err ~= nil)
assert(err.kind == "failed" and err.reason:find("boom", 1, true) and err.op == "std:task.Task:await")
assert(tostring(err) == "std:task.Task:await: failed: " .. err.reason)

-- a task that was cancelled, by either spelling
local pending = task.delay(60, function() end)
assert(pending:cancel() and not task.cancel(pending))
value, err = pending:await()
assert(value == nil and err ~= nil and err.kind == "cancelled")
assert(tostring(err) == "std:task.Task:await: cancelled: the task was cancelled")
local waiting = task.spawn(function()
	task.wait(60)
end)
assert(task.cancel(waiting) and waiting:is_finished())
assert(select(2, waiting:await()).kind == "cancelled")

ok, refused = pcall(task.cancel, 42)
assert(not ok and tostring(refused):find("std:task.cancel: argument #1 'task'", 1, true), tostring(refused))

-- a numeric string was a number
ok, refused = pcall(task.wait, "0.01")
assert(
	not ok and tostring(refused):find("std:task.wait: argument #1 'secs': expected a number", 1, true),
	tostring(refused)
)

-- the module cannot be patched
assert(not pcall(function()
	task.wait = nil
end))

return true
