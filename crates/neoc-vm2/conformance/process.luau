-- std:process: real processes, in each platform's words (`sh` on Unix, `cmd` and `ping` on Windows; `sort` is on
-- both). The shape of a failure only: what an error is differs on purpose (process.vm2.luau).
local process = require("std:process")
local windows = require("std:env").family == "windows"

local function sh(script, opts)
	opts = opts or {}
	opts.program = if windows then "cmd.exe" else "/bin/sh"
	opts.args = if windows then { "/D", "/C", script } else { "-c", script }
	local child = process.spawn(opts)
	assert(child, "the shell did not start")
	return child
end

local function read_all(child, method)
	local seen = ""
	while true do
		local output, err = child[method or "read"](child)
		assert(err == nil, "a read failed")
		if not output then
			return seen
		end
		seen ..= output
	end
end

-- without the platform's line ending, and `cmd`'s space before a redirection
local function text(output)
	return (string.gsub(string.gsub(output, "\r", ""), "%s+$", ""))
end

-- the two outputs are apart, and the exit code is the program's own
local child = sh(if windows then "echo out& echo err 1>&2& exit 7" else "printf out; printf err >&2; exit 7")
local had_id = type(child:id()) == "number"
local status = child:wait()
local out, err = text(read_all(child)), text(read_all(child, "read_stderr"))
local ended = { status.code, status.success, status.signal == nil, child:id() == nil, child:wait().code }
child:close()

-- what is written is the process's input, until it is closed
local sorter = process.spawn({ program = "sort" })
local wrote = { sorter:write("pear\n"), sorter:write("apple\n"), sorter:write(""), sorter:close_stdin(), sorter:close_stdin() }
local sorted = text(read_all(sorter))
local late, late_err = sorter:write("late\n")
local sorter_ok = sorter:wait().success

-- a read takes no more than it was told
local counted = sh("echo abcdef")
local first = counted:read(3)
local rest = text(read_all(counted))
local no_bytes, no_bytes_err = counted:read(0)
local too_many, too_many_err = counted:read(65537)

-- a stream that is not a pipe is not read or written
local quiet = sh("echo lost", { stdin = "null", stdout = "null", stderr = "inherit" })
local w, w_err = quiet:write("x")
local c, c_err = quiet:close_stdin()
local r, r_err = quiet:read()
local quiet_ok = quiet:wait().success

-- a process that stays is killed, and a closed one has no streams
local staying = if windows
	then process.spawn({ program = "ping.exe", args = { "-n", "1000", "127.0.0.1" } })
	else sh("echo ready; exec sleep 1000")
local ready = staying:read() ~= nil
staying:kill()
local killed = staying:wait()
staying:kill()
staying:close()
staying:close()
local closed_read, closed_read_err = staying:read()
local closed_write, closed_write_err = staying:write("x")

-- a command that cannot be started
local missing, missing_err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
local nowhere, nowhere_err = process.spawn({ program = "sort", cwd = "/nowhere/neoc-no-such-directory" })
local unnamed, unnamed_err = process.spawn({ program = "" })
local nul, nul_err = process.spawn({ program = "a\0b" })
local bad_env, bad_env_err = process.spawn({ program = "sort", env = { ["A=B"] = "1" } })

return {
	had_id,
	out,
	err,
	ended,
	wrote,
	sorted,
	late == nil and late_err ~= nil,
	sorter_ok,
	first,
	rest,
	no_bytes == nil and no_bytes_err ~= nil,
	too_many == nil and too_many_err ~= nil,
	w == nil and w_err ~= nil,
	c == nil and c_err ~= nil,
	r == nil and r_err ~= nil,
	quiet_ok,
	ready,
	{ killed.success, killed.code == nil, killed.signal == nil, staying:id() == nil },
	closed_read == nil and closed_read_err == nil,
	closed_write == nil and closed_write_err ~= nil,
	missing == nil and missing_err ~= nil,
	nowhere == nil and nowhere_err ~= nil,
	unnamed == nil and unnamed_err ~= nil,
	nul == nil and nul_err ~= nil,
	bad_env == nil and bad_env_err ~= nil,
	-- wrong types are raised
	(pcall(child.write, child, {})),
	(pcall(child.read, child, "many")),
}
