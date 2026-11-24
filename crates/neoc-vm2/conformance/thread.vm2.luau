--!strict
-- intentional: spawn(source, args?, shared?, options?) and spawn_file(path, ...) replace spawn({ source | file, name, args, shared }); parent() and shared() are functions; what may be shared is a declared union checked as an argument; every failure is a typed error
local thread = require("std:thread")
local collections = require("std:collections")

-- a script that is not a worker
assert(thread.parent() == nil and next(thread.shared()) == nil)

-- the argument is the script's `...`, the result comes back as plain data
local worker, spawn_err = thread.spawn("local args = ... return { doubled = args.v * 2 }", { v = 21 })
assert(worker ~= nil and spawn_err == nil)
local result, await_err = worker:await()
assert(await_err == nil and result.doubled == 42)

-- the result is taken once
local again, already = worker:await()
assert(again == nil and already ~= nil)
assert(already.kind == "already_awaited" and already.op == "std:thread.Worker:await")

-- messages both ways, through parent()
local echo = assert(thread.spawn([[
	local parent = require("std:thread").parent()
	parent:send({ got = parent:receive() })
	return "done"
]]))
echo:send("ping")
assert(echo:receive().got == "ping" and echo:await() == "done")

-- once the worker is gone: receive returns the error, send raises it
local nothing, closed = echo:receive()
assert(nothing == nil and closed ~= nil and closed.kind == "closed")
local sent, raised = pcall(function(): ...unknown
	echo:send("late")
end)
assert(not sent and tostring(raised):find("std:thread.Worker:send: closed:", 1, true), tostring(raised))

-- shared handles are the same state on both sides, under the name they were given
local hits, seen = collections.Counter.new(), collections.Map.new()
-- annotated: the checker takes each field of a literal for its own type, not for the union
local shared: { [string]: std_thread_Shared } = { hits = hits, seen = seen }
local counting = assert(thread.spawn(
	[[
	local shared = require("std:thread").shared()
	shared.seen:set("worker", shared.hits:increment())
]],
	nil,
	shared
))
assert(counting:await() == nil and hits:get() == 1 and seen:get("worker") == 1)

-- what cannot be shared, or is not plain data, is refused where it is passed
local ok, err = pcall(function()
	thread.spawn("return 1", nil, { n = 42 } :: any) -- deliberate refusal test: a number is not shared
end)
assert(not ok and tostring(err):find("std:thread.spawn: argument #3 'shared'", 1, true), tostring(err))
ok, err = pcall(function()
	thread.spawn("return 1", { buffer.create(2) })
end)
assert(not ok and tostring(err):find("std:thread.spawn: argument #2 'args'", 1, true), tostring(err))
ok, err = pcall(function()
	thread.spawn({ source = "return 1" } :: any) -- deliberate refusal test: the table spawn took
end)
assert(not ok and tostring(err):find("std:thread.spawn: argument #1 'source': expected a string", 1, true), tostring(err))

-- a worker that fails: a typed error with what it raised, named after its script
local failing = assert(thread.spawn("error('boom')", nil, nil, { name = "sums" }))
local value, failed = failing:await()
assert(value == nil and failed ~= nil and failed.kind == "failed")
assert(failed.reason:find("boom", 1, true) and failed.reason:find("sums", 1, true), failed.reason)
assert(tostring(failed):find("std:thread.Worker:await: failed: ", 1, true) == 1)

-- a terminated worker
local stuck = assert(thread.spawn([[require("std:thread").parent():receive()]]))
stuck:terminate()
local _, terminated = stuck:await()
assert(terminated ~= nil and terminated.kind == "terminated")

-- a file that is not there
local none, unreadable = thread.spawn_file("no/such/worker.luau")
assert(none == nil and unreadable ~= nil)
assert(unreadable.kind == "unreadable" and unreadable.path == "no/such/worker.luau")
assert(type(unreadable.reason) == "string" and unreadable.op == "std:thread.spawn_file")

return true
