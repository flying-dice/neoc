-- intentional: std:process errors are typed (kind, and cause / stream / what) and process.error_kind is gone; a command of the wrong shape is raised with its place (it was invalid_input); read's max is a whole number (a fraction, a negative or NaN is raised); program and cwd are paths
local process = require("std:process")
local windows = require("std:env").family == "windows"

-- the error has its kind: nothing to read out of a text
assert(process.error_kind == nil)

-- a program that is not there, and a directory that is not
local child, err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
assert(child == nil and err ~= nil)
assert(err.kind == "spawn" and err.cause == "not_found" and err.op == "std:process.spawn")
assert(err.message == err.reason and tostring(err) == "std:process.spawn: spawn: " .. err.reason)
child, err = process.spawn({ program = "sort", cwd = "/nowhere/neoc-no-such-directory" })
assert(child == nil and err ~= nil and err.kind == "spawn" and err.cause == "no_directory")

-- what the shape of a command does not say is still an error of the call
child, err = process.spawn({ program = "" })
assert(child == nil and err ~= nil and err.kind == "invalid_input")
assert(tostring(err) == 'std:process.spawn: invalid_input: option "program" must be the name or the path of a program')
child, err = process.spawn({ program = "sort", args = { "a\0b" } })
assert(child == nil and err ~= nil and err.kind == "invalid_input" and err.reason:find('"args"', 1, true))

-- what is not a command is a wrong argument
for _, command in { {}, { progam = "sort" }, { program = "sort", stdin = "pipe" }, { program = "sort", args = "x" }, 7, "sort" } do
	local ok, raised = pcall(process.spawn, command)
	assert(not ok and tostring(raised):find("std:process.spawn: argument #1 'command'", 1, true), tostring(raised))
end

local function sh(script, opts)
	opts = opts or {}
	opts.program = if windows then "cmd.exe" else "/bin/sh"
	opts.args = if windows then { "/D", "/C", script } else { "-c", script }
	local started, why = process.spawn(opts)
	assert(started, tostring(why))
	return started
end

-- the stream an error is about
local quiet = sh("echo lost", { stdin = "null", stdout = "null", stderr = "null" })
local _, written = quiet:write("x")
assert(written ~= nil and written.kind == "not_piped" and written.stream == "stdin")
assert(tostring(written) == "std:process.Child:write: not_piped: stdin is not a pipe")
local _, closed = quiet:close_stdin()
assert(closed ~= nil and closed.kind == "not_piped" and closed.stream == "stdin")
local _, read = quiet:read()
assert(read ~= nil and read.kind == "not_piped" and read.stream == "stdout")
_, read = quiet:read_stderr()
assert(read ~= nil and read.kind == "not_piped" and read.stream == "stderr")
local status = quiet:wait()
assert(status ~= nil and status.success and status.code == 0 and status.signal == nil)

-- a count of bytes is a whole number; one out of range is an error of the call
local counted = sh("echo abcdef")
for _, max in { -1, 1.5, 0 / 0, "3" } do
	local ok, raised = pcall(counted.read, counted, max)
	assert(not ok and tostring(raised):find("std:process.Child:read: argument #1 'max'", 1, true), tostring(raised))
end
local none, range = counted:read(0)
assert(none == nil and range ~= nil and range.kind == "invalid_input")
assert(counted:read(3) == "abc")

-- what is closed
counted:close()
local _, late = counted:write("x")
assert(late ~= nil and late.kind == "closed" and late.what == "stdin")
assert(tostring(late) == "std:process.Child:write: closed: stdin is closed")

return true
