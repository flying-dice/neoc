--!strict
-- intentional: std:task's spawn, defer and delay take a function (no coroutine) and pass the arguments after it on; Task:await gives the first return value or nil and a typed error (cancelled | failed with a reason), not every value or a text; a wrong argument is refused with its place
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

-- the arguments after the function are what it is called with: nils are kept, a table is that table
-- a body is `(...unknown) -> ()`: an untyped one of any arity is accepted, and narrows what it was given
local function add(a, b)
	return (a :: number) + (b :: number)
end
local function count(...)
	return select("#", ...)
end
local function same(a, b)
	return a == t and b == count
end
assert(task.spawn(add, 1, 2):await() == 3)
assert(task.defer(add, 2, 3):await() == 5)
assert(task.delay(0, add, 3, 4):await() == 7)
assert(task.spawn(count):await() == 0)
assert(task.spawn(count, nil):await() == 1)
assert(task.defer(count, 1, nil, nil):await() == 3)
assert(task.delay(0, count, nil, 2, nil):await() == 3)
assert(task.spawn(same, t, count):await() == true)
assert(task.spawn(function(a, b, c)
	return a == 1 and b == nil and c == "x"
end, 1, nil, "x"):await() == true)
-- a body that takes none still closes over what it needs
assert(task.spawn(function()
	return add(1, 2)
end):await() == 3)

-- a coroutine is not a function
local ok, refused = pcall(function()
	task.spawn(coroutine.create(function() end) :: any)
end)
assert(
	not ok and tostring(refused):find("std:task.spawn: argument #1 'body': expected a function", 1, true),
	tostring(refused)
)
ok, refused = pcall(function()
	task.delay(0, 42 :: any)
end)
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
assert(tostring(err) == "std:task.Task:await: failed: " .. err.message)

-- a task that was cancelled, by either spelling
local pending = task.delay(60, function() end)
assert(pending:cancel() and not task.cancel(pending))
local gone, cancelled = pending:await()
assert(gone == nil and cancelled ~= nil and cancelled.kind == "cancelled")
assert(tostring(cancelled) == "std:task.Task:await: cancelled: the task was cancelled")
local waiting = task.spawn(function()
	task.wait(60)
end)
assert(task.cancel(waiting) and waiting:is_finished())
local _, waiting_err = waiting:await()
assert(waiting_err ~= nil and waiting_err.kind == "cancelled")

ok, refused = pcall(function()
	task.cancel(42 :: any)
end)
assert(not ok and tostring(refused):find("std:task.cancel: argument #1 'task'", 1, true), tostring(refused))

-- a numeric string was a number
ok, refused = pcall(function()
	task.wait("0.01" :: any)
end)
assert(
	not ok and tostring(refused):find("std:task.wait: argument #1 'secs': expected a number", 1, true),
	tostring(refused)
)

-- the module cannot be patched
assert(not pcall(function()
	(task :: any).wait = nil
end))

return true
