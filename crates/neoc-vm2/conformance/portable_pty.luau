-- vnd:portable_pty: real terminals, in each platform's words (`sh` and `cat` on Unix, `cmd` on a ConPTY on Windows).
-- The shape of a failure only: what an error is differs on purpose (portable_pty.vm2.luau). The output is read up
-- to what is looked for and never to its end: on Windows neoc-vm's does not end before `close()`.
local portable_pty = require("vnd:portable_pty")
local windows = require("std:env").family == "windows"
local out = {}

local function fails(value, err)
	return value == nil and err ~= nil
end

-- what ends a line of input: a ConPTY takes keys, and the key is Enter
local enter = if windows then "\r" else "\n"

local function open(opts)
	local pty = portable_pty.spawn_command(opts)
	assert(pty, "the command did not start")
	if windows then
		-- a pseudo-console asks where the cursor is before anything runs on it: the top left corner
		pty:write("\27[1;1R")
	end
	return pty
end

local function sh(script, opts)
	opts = opts or {}
	opts.program = if windows then "cmd.exe" else "/bin/sh"
	opts.args = if windows then { "/D", "/C", script } else { "-c", script }
	return open(opts)
end

local function read_until(pty, needle)
	local seen = ""
	while not string.find(seen, needle, 1, true) do
		local output, err = pty:read()
		assert(err == nil, "a read failed")
		assert(output, "the output ended before `" .. needle .. "`")
		seen ..= output
	end
	return true
end

-- a command: what it wrote, and the exit code is its own
local pty = sh(if windows then "echo out& exit 7" else "printf out; exit 7", { rows = 30, cols = 100 })
local had_id = type(pty:process_id()) == "number"
local wrote = read_until(pty, "out")
local status = pty:wait()
out.command = { had_id, wrote, status.exit_code, status.success, status.signal == nil, pty:process_id() == nil, pty:wait().exit_code }
pty:close()
out.after_close_the_answer_stays = pty:wait().exit_code

-- a process that stays: what is written is its input, and comes back (the terminal echoes it)
local shell
if windows then
	shell = open({ program = "cmd.exe", args = { "/D", "/Q", "/K", "echo ready" } })
else
	shell = sh("echo ready; cat")
end
out.ready = read_until(shell, "ready")
out.write = shell:write("hello" .. enter)
out.echo = read_until(shell, "hello")
out.empty_write = shell:write("")
out.resize = shell:resize(40, 100)

-- what is out of range is refused, and nothing is sent
out.limits = {
	fails(shell:read(0)),
	fails(shell:read(65537)),
	fails(shell:resize(0, 80)),
	fails(shell:resize(24, 40000)),
	fails(shell:write(string.rep("x", 1024 * 1024 + 1))),
}

-- kill: the process ends, and not by itself
shell:kill()
local killed = shell:wait()
out.killed = { killed.success, killed.exit_code, killed.signal == nil, shell:process_id() == nil }

-- close: idempotent; reads end, writes and resizes fail, the answer of `wait` stays
shell:close()
shell:close()
shell:kill()
local output, read_err = shell:read()
out.closed = {
	output == nil and read_err == nil,
	fails(shell:write("x")),
	fails(shell:write("")),
	fails(shell:resize(10, 10)),
	shell:process_id() == nil,
	shell:wait().exit_code == killed.exit_code,
}

-- the default shell, at the default size: it starts, and it can be ended
local default = portable_pty.spawn_command()
out.default = { type(default:process_id()) == "number" }
default:close()
table.insert(out.default, default:wait().success)

-- what cannot be started
out.refused = {
	fails(portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })),
	fails(portable_pty.spawn_command({ program = "neoc-no-such-program", args = { "x" } })),
	fails(portable_pty.spawn_command({ cwd = "/nowhere/neoc-no-such-directory" })),
	fails(portable_pty.spawn_command({ program = "" })),
	fails(portable_pty.spawn_command({ program = "a\0b" })),
	fails(portable_pty.spawn_command({ args = { "-l" } })),
	fails(portable_pty.spawn_command({ cwd = "" })),
	fails(portable_pty.spawn_command({ rows = 0 })),
	fails(portable_pty.spawn_command({ cols = 40000 })),
	fails(portable_pty.spawn_command({ env = { ["A=B"] = "1" } })),
}

return out
