--!strict
-- intentional: std:process errors are typed (kind, and cause / stream / what) and process.error_kind is gone; a command of the wrong shape is raised with its place (it was invalid_input); read's max is a whole number (a fraction, a negative or NaN is raised); program and cwd are paths
local process = require("std:process")
local windows = require("std:env").family == "windows"

-- the error has its kind: nothing to read out of a text
assert((process :: any).error_kind == nil)

-- a program that is not there, and a directory that is not
local child, err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
assert(child == nil and err ~= nil)
assert(err.kind == "spawn" and err.cause == "not_found" and err.op == "std:process.spawn")
assert(err.message == err.reason and tostring(err) == "std:process.spawn: spawn: " .. err.reason)
local no_dir_child, no_dir = process.spawn({ program = "sort", cwd = "/nowhere/neoc-no-such-directory" })
assert(no_dir_child == nil and no_dir ~= nil and no_dir.kind == "spawn" and no_dir.cause == "no_directory")

-- what the shape of a command does not say is still an error of the call
local empty_child, empty = process.spawn({ program = "" })
assert(empty_child == nil and empty ~= nil and empty.kind == "invalid_input")
assert(tostring(empty) == 'std:process.spawn: invalid_input: option "program" must be the name or the path of a program')
local nul_child, nul = process.spawn({ program = "sort", args = { "a\0b" } })
assert(nul_child == nil and nul ~= nil and nul.kind == "invalid_input" and nul.reason:find('"args"', 1, true))

-- what is not a command is a wrong argument
-- deliberate refusal test: none is a command
for _, command in { {}, { progam = "sort" }, { program = "sort", stdin = "pipe" }, { program = "sort", args = "x" }, 7, "sort" } :: { any } do
	local ok, raised = pcall(function()
		process.spawn(command)
	end)
	assert(not ok and tostring(raised):find("std:process.spawn: argument #1 'command'", 1, true), tostring(raised))
end

local function sh(script: string, stdio: std_process_Stdio?): std_process_Child
	local started, why = process.spawn({
		program = if windows then "cmd.exe" else "/bin/sh",
		args = if windows then { "/D", "/C", script } else { "-c", script },
		stdin = stdio,
		stdout = stdio,
		stderr = stdio,
	})
	if not started then
		error(tostring(why))
	end
	return started
end

-- the stream an error is about
local quiet = sh("echo lost", "null")
local _, written = quiet:write("x")
assert(written ~= nil and written.kind == "not_piped" and written.stream == "stdin")
assert(tostring(written) == "std:process.Child:write: not_piped: stdin is not a pipe")
local _, closed = quiet:close_stdin()
assert(closed ~= nil and closed.kind == "not_piped" and closed.stream == "stdin")
local _, read = quiet:read()
assert(read ~= nil and read.kind == "not_piped" and read.stream == "stdout")
local _, read_stderr = quiet:read_stderr()
assert(read_stderr ~= nil and read_stderr.kind == "not_piped" and read_stderr.stream == "stderr")
local status = quiet:wait()
assert(status ~= nil and status.success and status.code == 0 and status.signal == nil)

-- a count of bytes is a whole number; one out of range is an error of the call
local counted = sh("echo abcdef")
for _, max in { -1, 1.5, 0 / 0, "3" :: any } do -- deliberate refusal test: text is not a count
	local ok, raised = pcall(function()
		counted:read(max)
	end)
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
