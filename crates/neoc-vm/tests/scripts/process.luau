--!strict
-- std:process as a script sees it: its answers, then what it asserts. Real processes, in each platform's words (`sh`
-- on Unix, `cmd` and `ping` on Windows; `sort` is on both), so the answers are a file per platform
-- (process.unix.json, process.windows.json).
local process = require("std:process")
local windows = require("std:env").family == "windows"

local function sh(script: string, streams: { [string]: string }?): std_process_Child
  local opts: any = streams or {}
  opts.program = if windows then "cmd.exe" else "/bin/sh"
  opts.args = if windows then { "/D", "/C", script } else { "-c", script }
  local child = process.spawn(opts)
  assert(child, "the shell did not start")
  return child
end

local function read_all(child: std_process_Child, method: string?): string
  local seen = ""
  while true do
    local output, err = (child :: any)[method or "read"](child)
    assert(err == nil, "a read failed")
    if not output then
      return seen
    end
    seen ..= output
  end
end

-- without the platform's line ending, and `cmd`'s space before a redirection
local function text(output: string): string
  return (string.gsub(string.gsub(output, "\r", ""), "%s+$", ""))
end

-- the two outputs are apart, and the exit code is the program's own
local child = sh(if windows then "echo out& echo err 1>&2& exit 7" else "printf out; printf err >&2; exit 7")
local status = assert(child:wait())
local out, err = text(read_all(child)), text(read_all(child, "read_stderr"))
local ended =
  { status.code, status.success, status.signal == nil, child:id() == nil, assert(child:wait()).code } :: { any }
child:close()

-- what is written is the process's input, until it is closed
local sorter = assert(process.spawn { program = "sort" })
-- it has an id while it runs: it waits for its input, so it has not ended (the shell above may have, before it is asked)
local had_id = type(sorter:id()) == "number"
local wrote =
  { sorter:write("pear\n"), sorter:write("apple\n"), sorter:write(""), sorter:close_stdin(), sorter:close_stdin() }
local sorted = text(read_all(sorter))
local late, late_err = sorter:write("late\n")
local sorter_ok = assert(sorter:wait()).success

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
local quiet_ok = assert(quiet:wait()).success

-- a process that stays is killed, and a closed one has no streams
local staying = if windows
  then assert(process.spawn { program = "ping.exe", args = { "-n", "1000", "127.0.0.1" } })
  else sh("echo ready; exec sleep 1000")
local ready = staying:read() ~= nil
staying:kill()
local killed = assert(staying:wait())
staying:kill()
staying:close()
staying:close()
local closed_read, closed_read_err = staying:read()
local closed_write, closed_write_err = staying:write("x")

-- a command that cannot be started
local missing, missing_err = process.spawn { program = "/nowhere/neoc-no-such-program" }
local nowhere, nowhere_err = process.spawn { program = "sort", cwd = "/nowhere/neoc-no-such-directory" }
local unnamed, unnamed_err = process.spawn { program = "" }
local nul, nul_err = process.spawn { program = "a\0b" }
local bad_env, bad_env_err = process.spawn { program = "sort", env = { ["A=B"] = "1" } }

-- what it asserts
do
  -- the error has its kind: nothing to read out of a text
  assert((process :: any).error_kind == nil)

  -- a program that is not there, and a directory that is not
  local child, err = process.spawn { program = "/nowhere/neoc-no-such-program" }
  assert(child == nil and err ~= nil)
  assert(err.kind == "spawn" and err.cause == "not_found" and err.op == "std:process.spawn")
  assert(err.message == err.reason and tostring(err) == "std:process.spawn: spawn: " .. err.reason)
  local no_dir_child, no_dir = process.spawn { program = "sort", cwd = "/nowhere/neoc-no-such-directory" }
  assert(no_dir_child == nil and no_dir ~= nil and no_dir.kind == "spawn" and no_dir.cause == "no_directory")

  -- what the shape of a command does not say is still an error of the call
  local empty_child, empty = process.spawn { program = "" }
  assert(empty_child == nil and empty ~= nil and empty.kind == "invalid_input")
  assert(
    tostring(empty) == 'std:process.spawn: invalid_input: option "program" must be the name or the path of a program'
  )
  local nul_child, nul = process.spawn { program = "sort", args = { "a\0b" } }
  assert(nul_child == nil and nul ~= nil and nul.kind == "invalid_input" and nul.reason:find('"args"', 1, true))

  -- what is not a command is a wrong argument
  -- deliberate refusal test: none is a command
  for _, command in
    { {}, { progam = "sort" }, { program = "sort", stdin = "pipe" }, { program = "sort", args = "x" }, 7, "sort" } :: { any }
  do
    local ok, raised = pcall(function()
      process.spawn(command)
    end)
    assert(not ok and tostring(raised):find("std:process.spawn: argument #1 'command'", 1, true), tostring(raised))
  end

  local function sh(script: string, stdio: std_process_Stdio?): std_process_Child
    local started, why = process.spawn {
      program = if windows then "cmd.exe" else "/bin/sh",
      args = if windows then { "/D", "/C", script } else { "-c", script },
      stdin = stdio,
      stdout = stdio,
      stderr = stdio,
    }
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
end

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
  (pcall(child.write :: any, child, {})),
  (pcall(child.read :: any, child, "many")),
} :: { any }
