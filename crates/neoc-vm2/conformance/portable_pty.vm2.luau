--!strict
-- intentional: vnd:portable_pty errors are typed (kind: invalid_input, spawn, closed, io, other; reason) and error_kind / error_message are gone; options of the wrong shape are raised with their place (they were invalid_input); read's max and resize's rows and cols are whole numbers (a fraction or a negative number is raised); on Windows the output ends when the process does (neoc-vm's never ended before close()) and a resize after that is closed.
local portable_pty = require("vnd:portable_pty")
local windows = require("std:env").family == "windows"

-- the error has its kind: nothing to read out of a text
assert((portable_pty :: any).error_kind == nil and (portable_pty :: any).error_message == nil)

-- a program that is not there, and a directory that is not
local none, err = portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })
assert(none == nil and err ~= nil)
assert(err.kind == "spawn" and err.op == "vnd:portable_pty.spawn_command")
assert(err.message == err.reason and tostring(err) == "vnd:portable_pty.spawn_command: spawn: " .. err.reason)
local no_dir, dir_err = portable_pty.spawn_command({ cwd = "/nowhere/neoc-no-such-directory" })
assert(no_dir == nil and dir_err ~= nil and dir_err.kind == "spawn")
assert(dir_err.reason:find("/nowhere/neoc-no-such-directory", 1, true))

-- what the shape of the options does not say is still an error of the call
local function invalid(opts: vnd_portable_pty_Spawn, reason: string)
	local pty, refused = portable_pty.spawn_command(opts)
	assert(pty == nil and refused ~= nil and refused.kind == "invalid_input")
	assert(refused.reason == reason, refused.reason)
	assert(tostring(refused) == "vnd:portable_pty.spawn_command: invalid_input: " .. reason)
end
invalid({ program = "" }, 'option "program" must be the name or the path of a program')
invalid({ args = { "-l" } }, 'option "args" needs "program": the default shell takes no arguments')
invalid({ program = "sh", args = { "a\0b" } }, 'option "args" cannot contain a NUL byte')
invalid({ cwd = "" }, 'option "cwd" must be the path of a directory')
invalid({ env = { ["A=B"] = "1" } }, 'option "env" holds a name that is empty or contains `=` or a NUL byte')
invalid({ rows = 0 }, 'option "rows" must be a whole number from 1 to 32767')
invalid({ cols = 40000 }, 'option "cols" must be a whole number from 1 to 32767')

-- what is not of the shape is a wrong argument
for _, opts in { { progam = "sh" }, { program = 7 }, { program = "sh", args = "-c" }, { rows = 1.5 }, { cols = -1 }, { cols = "80" }, 7, "sh" } :: { any } do
	local ok, raised = pcall(function()
		portable_pty.spawn_command(opts)
	end)
	assert(not ok and tostring(raised):find("vnd:portable_pty.spawn_command: argument #1 'opts'", 1, true), tostring(raised))
end

-- a terminal: the output ends once the process has ended, on every platform
local pty = assert(portable_pty.spawn_command({
	program = if windows then "cmd.exe" else "/bin/sh",
	args = if windows then { "/D", "/C", "echo out& exit 7" } else { "-c", "printf out; exit 7" },
	rows = 30,
	cols = 100,
}))
if windows then
	-- a pseudo-console asks where the cursor is before anything runs on it: the top left corner
	assert(pty:write("\27[1;1R"))
end
local status = assert(pty:wait())
assert(status.exit_code == 7 and status.success == false and status.signal == nil)
local seen = ""
while true do
	local output, read_err = pty:read()
	assert(read_err == nil, tostring(read_err))
	if output == nil then
		break
	end
	seen ..= output
end
assert(seen:find("out", 1, true), seen)
assert(pty:process_id() == nil)

-- a size and a `max` out of range are errors; what is not a whole number is a wrong argument
local resized, size_err = pty:resize(0, 80)
assert(resized == nil and size_err ~= nil and size_err.kind == "invalid_input")
assert(size_err.op == "vnd:portable_pty.Pty:resize" and size_err.message == 'option "rows" must be a whole number from 1 to 32767')
local bytes, max_err = pty:read(65537)
assert(bytes == nil and max_err ~= nil and max_err.kind == "invalid_input")
for _, call in
	{
		function()
			pty:resize(1.5, 80)
		end,
		function()
			pty:resize(24, -1)
		end,
		function()
			pty:resize(24, "80" :: any)
		end,
		function()
			pty:read(1.5)
		end,
		function()
			pty:read(-1)
		end,
		function()
			pty:write({} :: any)
		end,
	}
do
	assert(not pcall(call))
end

-- closed: typed, with the method that was called
pty:close()
pty:close()
local after, after_err = pty:read()
assert(after == nil and after_err == nil)
local written, closed = pty:write("x")
assert(written == nil and closed ~= nil and closed.kind == "closed")
assert(closed.op == "vnd:portable_pty.Pty:write")
assert(tostring(closed) == "vnd:portable_pty.Pty:write: closed: the terminal is closed")
local _, resize_closed = pty:resize(24, 80)
assert(resize_closed ~= nil and resize_closed.kind == "closed")
assert(assert(pty:wait()).exit_code == 7)
