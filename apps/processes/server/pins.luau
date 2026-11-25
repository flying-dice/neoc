--!strict
-- The processes the windows have chosen. A chosen process is held by a native reference where the platform has one
-- (a pidfd, a process handle), so that ending it can only reach the process the person looked at, even if its PID
-- is reused in between. A reference is a descriptor or a handle that stays until it is let go, so the app holds few:
-- a window holds one at a time, and when more are held than any real use needs, the oldest is let go.
local rpc = require("../.app/rpc")

local pins = {}

local MAX_PINS = 16

local held: { [string]: vnd_sysinfo_Process } = {}
-- the keys of `held`, oldest first
local order: { string } = {}

local function key(ref: rpc.ProcessRef): string
  return `{ref.pid}:{ref.startTime}`
end

local function drop(at: string)
  local process = held[at]
  if process then
    process:release()
    held[at] = nil
  end
  local index = table.find(order, at)
  if index then
    table.remove(order, index)
  end
end

-- Hold a native reference to the process. Answers with why it could not: it has quit, or its PID is another's now.
function pins.hold(process: vnd_sysinfo_Process): vnd_sysinfo_Error?
  local at = key { pid = process.pid, startTime = process.start_time }
  if held[at] then
    return nil
  end
  local _, err = process:pin()
  if err then
    return err
  end
  held[at] = process
  table.insert(order, at)
  while #order > MAX_PINS do
    drop(order[1])
  end
  return nil
end

function pins.get(ref: rpc.ProcessRef): vnd_sysinfo_Process?
  return held[key(ref)]
end

function pins.release(ref: rpc.ProcessRef)
  drop(key(ref))
end

-- Let go of the reference to every process that has exited: it can be signalled no more, and the descriptor is of no
-- use. The process stays known, so a window that shows it can say it has quit.
function pins.sweep()
  for _, process in held do
    if not process:alive() then
      process:release()
    end
  end
end

return pins
