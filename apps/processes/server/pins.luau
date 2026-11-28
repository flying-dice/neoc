--!strict
-- The processes the windows have chosen. A choice is a selection: an id the backend issues, one per `select` call, so
-- that two windows that look at the same process each hold their own and neither can let go of, or act through, the
-- other's. A selection holds a native reference to its process where the platform has one (a pidfd, a process handle),
-- so that ending it can only reach the process the person looked at, even if its PID is reused in between.
--
-- Fail closed: an id the backend did not issue (a window that outlived a restart), or no longer holds (released, or
-- let go because more are held than any real use needs), is not a selection, and nothing is chosen on its behalf.
-- A reference that was let go is finished: it is never taken again. A process that could be looked at but not held for
-- ending (the system would not let Processes, or the platform does not know which process it is) is a selection
-- with `blocked` set, so that it can be inspected and cannot be ended.
local random = require("lib:random")
local tracing = require("lib:tracing")

local pins = {}

-- how many selections are held at once: a window holds one at a time, and when more are held than any real use needs,
-- the oldest is let go
local MAX_SELECTIONS = 64

export type Selection = {
  id: string,
  process: vnd_sysinfo_Process,
  -- why this process cannot be ended through this selection, when it cannot
  blocked: vnd_sysinfo_Error?,
}

-- an id from an earlier run of the backend is not one of this run's
local run = random.uuid_v4()
local issued = 0

local held: { [string]: Selection } = {}
-- the ids of `held`, oldest first
local order: { string } = {}

local function drop(id: string)
  local selection = held[id]
  if selection then
    selection.process:release()
    held[id] = nil
  end
  local index = table.find(order, id)
  if index then
    table.remove(order, index)
  end
end

-- Choose the process. Answers with why it could not: it has quit, or its PID is another's now. Anything else the system
-- says about holding it does not stop it being looked at: it is `blocked`.
function pins.hold(process: vnd_sysinfo_Process): (Selection?, vnd_sysinfo_Error?)
  local _, err = process:pin()
  if err and (err.kind == "process_gone" or err.kind == "pid_reused") then
    return nil, err
  end
  if err then
    tracing.debug(
      "could not hold the process, so it is only looked at",
      { error = err.message, kind = err.kind, pid = process.pid }
    )
  end
  issued += 1
  local selection: Selection = { id = `{run}:{issued}`, process = process, blocked = err }
  held[selection.id] = selection
  table.insert(order, selection.id)
  while #order > MAX_SELECTIONS do
    drop(order[1])
  end
  return selection, nil
end

function pins.get(id: string): Selection?
  return held[id]
end

function pins.release(id: string)
  drop(id)
end

-- Let go of the reference to every process that has exited: it can be signalled no more, and the descriptor is of no
-- use. The selection stays, so a window that shows the process can say it has quit.
function pins.sweep()
  for _, selection in held do
    if not selection.process:alive() then
      selection.process:release()
    end
  end
end

return pins
