--!strict
-- vnd:portable_pty as a script sees it: its answers, then what it asserts. Real terminals, in each platform's words
-- (`sh` and `cat` on Unix, `cmd` on a ConPTY on Windows), so the answers are a file per platform
-- (portable_pty.unix.json, portable_pty.windows.json). The output is read up to what is looked for and never to its
-- end.
local portable_pty = require("vnd:portable_pty")
local windows = require("std:env").family == "windows"
local out = {}

local function fails(value: unknown, err: unknown): boolean
  return value == nil and err ~= nil
end

-- what ends a line of input: a ConPTY takes keys, and the key is Enter
local enter = if windows then "\r" else "\n"

local function open(opts: any): vnd_portable_pty_Pty
  local pty = portable_pty.spawn_command(opts)
  assert(pty, "the command did not start")
  if windows then
    -- a pseudo-console asks where the cursor is before anything runs on it: the top left corner
    pty:write("\27[1;1R")
  end
  return pty
end

local function sh(script: string, size: { rows: number, cols: number }?): vnd_portable_pty_Pty
  local opts: any = size or {}
  opts.program = if windows then "cmd.exe" else "/bin/sh"
  opts.args = if windows then { "/D", "/C", script } else { "-c", script }
  return open(opts)
end

local function read_until(pty: vnd_portable_pty_Pty, needle: string): boolean
  local seen = ""
  while not string.find(seen, needle, 1, true) do
    local output, err = pty:read()
    assert(err == nil, "a read failed")
    assert(output, "the output ended before `" .. needle .. "`")
    seen ..= output
  end
  return true
end

-- a command: what it wrote, and the exit code is its own. A process that has ended has no id, so on Unix the shell
-- waits for a line before it exits: the id is read while it surely runs (a `printf out; exit 7` could end first).
local pty = sh(if windows then "echo out& exit 7" else "printf out; read _; exit 7", { rows = 30, cols = 100 })
local had_id = type(pty:process_id()) == "number"
local wrote = read_until(pty, "out")
if not windows then
  assert(pty:write(enter))
end
local status = assert(pty:wait())
out.command = {
  had_id,
  wrote,
  status.exit_code,
  status.success,
  status.signal == nil,
  pty:process_id() == nil,
  assert(pty:wait()).exit_code,
} :: { any }
pty:close()
out.after_close_the_answer_stays = assert(pty:wait()).exit_code

-- a process that stays: what is written is its input, and comes back (the terminal echoes it)
local shell
if windows then
  shell = open { program = "cmd.exe", args = { "/D", "/Q", "/K", "echo ready" } }
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
local killed = assert(shell:wait())
out.killed = { killed.success, killed.exit_code, killed.signal == nil, shell:process_id() == nil } :: { any }

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
  assert(shell:wait()).exit_code == killed.exit_code,
}

-- the default shell, at the default size: it starts, and it can be ended
local default = assert(portable_pty.spawn_command())
out.default = { type(default:process_id()) == "number" }
default:close()
table.insert(out.default, assert(default:wait()).success)

-- what cannot be started
out.refused = {
  fails(portable_pty.spawn_command { program = "/nowhere/neoc-no-such-program" }),
  fails(portable_pty.spawn_command { program = "neoc-no-such-program", args = { "x" } }),
  fails(portable_pty.spawn_command { cwd = "/nowhere/neoc-no-such-directory" }),
  fails(portable_pty.spawn_command { program = "" }),
  fails(portable_pty.spawn_command { program = "a\0b" }),
  fails(portable_pty.spawn_command { args = { "-l" } }),
  fails(portable_pty.spawn_command { cwd = "" }),
  fails(portable_pty.spawn_command { rows = 0 }),
  fails(portable_pty.spawn_command { cols = 40000 }),
  fails(portable_pty.spawn_command { env = { ["A=B"] = "1" } }),
}

-- what it asserts
do
  -- the error has its kind: nothing to read out of a text
  assert((portable_pty :: any).error_kind == nil and (portable_pty :: any).error_message == nil)

  -- a program that is not there, and a directory that is not
  local none, err = portable_pty.spawn_command { program = "/nowhere/neoc-no-such-program" }
  assert(none == nil and err ~= nil)
  assert(err.kind == "spawn" and err.op == "vnd:portable_pty.spawn_command")
  assert(err.message == err.reason and tostring(err) == "vnd:portable_pty.spawn_command: spawn: " .. err.reason)
  local no_dir, dir_err = portable_pty.spawn_command { cwd = "/nowhere/neoc-no-such-directory" }
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
  for _, opts in
    {
      { progam = "sh" },
      { program = 7 },
      { program = "sh", args = "-c" },
      { rows = 1.5 },
      { cols = -1 },
      { cols = "80" },
      7,
      "sh",
    } :: { any }
  do
    local ok, raised = pcall(function()
      portable_pty.spawn_command(opts)
    end)
    assert(
      not ok and tostring(raised):find("vnd:portable_pty.spawn_command: argument #1 'opts'", 1, true),
      tostring(raised)
    )
  end

  -- a terminal: the output ends once the process has ended, on every platform
  local pty = assert(portable_pty.spawn_command {
    program = if windows then "cmd.exe" else "/bin/sh",
    args = if windows then { "/D", "/C", "echo out& exit 7" } else { "-c", "printf out; exit 7" },
    rows = 30,
    cols = 100,
  })
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
  assert(
    size_err.op == "vnd:portable_pty.Pty:resize"
      and size_err.message == 'option "rows" must be a whole number from 1 to 32767'
  )
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
end

return out
