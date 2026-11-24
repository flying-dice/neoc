-- std:task: what both VMs offer. A task's body is a function, and a task that returns one value (what neoc-vm2
-- keeps of them: task.vm2.luau).
local task = require("std:task")

local slept = task.wait(0.02)

-- the first slice runs inside `spawn`, the rest alongside the caller
local order = {}
local spawned = task.spawn(function()
	table.insert(order, "spawned")
	task.wait(0)
	table.insert(order, "resumed")
	return "done"
end)
table.insert(order, "caller")
local done = spawned:await()

-- deferred tasks run after the batch, in the order they were queued
local first = task.defer(function()
	table.insert(order, "first")
end)
local second = task.defer(function()
	table.insert(order, "second")
end)
table.insert(order, "batch")
first:await()
second:await()

local late = task.delay(0.01, function()
	table.insert(order, "delayed")
	return 42
end)
table.insert(order, "before")
local answer = late:await()

-- a table a task returns is that table
local shared = {}
local same = task.spawn(function()
	task.wait(0)
	return shared
end):await() == shared

-- the arguments after the function are what it is called with, nils and live values included
local function count(...)
	return select("#", ...)
end
local passed = {
	task.spawn(function(a, b)
		return a + b
	end, 1, 2):await(),
	task.defer(function(a, b)
		return a .. b
	end, "x", "y"):await(),
	task.delay(0, function(a, b, c)
		return a * b * c
	end, 2, 3, 4):await(),
	task.spawn(count):await(),
	task.spawn(count, nil):await(),
	task.defer(count, 1, nil, nil):await(),
	task.delay(0, count, nil, 2, nil):await(),
	task.spawn(function(t, f)
		return t == shared and f == count
	end, shared, count):await(),
}

-- cancelling: once, and a cancelled task never runs and has no value
local ran = false
local pending = task.delay(60, function()
	ran = true
end)
local before = pending:is_finished()
local cancelled, again = pending:cancel(), task.cancel(pending)
local none, why = pending:await()

local failed, reason = task.spawn(function()
	error("boom")
end):await()

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
	order,
	done,
	answer,
	same,
	spawned:is_finished(),
	spawned:cancel(),
	(spawned:await()),
	{ before, cancelled, again, pending:is_finished(), ran },
	none == nil and why ~= nil,
	failed == nil and reason ~= nil,
	-- what is not a function is no task
	(pcall(task.spawn, 42)),
	(pcall(task.defer, nil)),
	(pcall(task.delay, 0, "body")),
	passed,
}
