--!strict
-- The Processes app's backend: the machine's load and its programs, read through `vnd:sysinfo` (monitor.luau), and
-- two ways of ending a program the person has chosen (pins.luau). The windows ask for a reading as often as the
-- person lets them (every second, two, five, or not at all), and every answer is the whole reading, so what a window
-- draws is what the backend has.
--
-- Ending a program is the one thing here that can do harm, so it is narrow: it is only ever the program a window chose,
-- through the id the backend issued for that choice, held by a reference that a reused PID cannot move; a choice that
-- is no longer held is refused, never made again; asking it to quit (`terminate`) is never turned into forcing it; and
-- what the operating system refused is told to the person in a sentence. It serves on the RPC bus through the global
-- `RPC`, typed by the generated `types/vos.d.luau`; the page's methods are private (`_snapshot`, `_select`, ...), and no
-- agent is offered any of them.
local monitor = require("./monitor")
local pins = require("./pins")

-- Why the operating system or this app would not end a program, for the person who pressed the button.
local function sentence(err: vnd_sysinfo_Error, name: string, force: boolean): string
  local kind = err.kind
  if kind == "process_gone" then
    return `{name} has already quit. Nothing was stopped.`
  elseif kind == "pid_reused" then
    return `{name} has quit, and another program has its number now. Nothing was stopped.`
  elseif kind == "own_process" then
    return "Processes runs inside this program, so it cannot stop it."
  elseif kind == "unknown_identity" then
    return `Processes cannot tell {name} apart from another program that is given its number (yet, if it only just started), so it will not stop it. Choose it again in a moment.`
  elseif kind == "released" then
    return "Choose the program again: Processes no longer holds it."
  elseif kind == "permission_denied" then
    return `This system did not let Processes stop {name}. It belongs to a user with more rights than yours: ask an administrator.`
  elseif kind == "unsupported" then
    return if force
      then `This system cannot stop {name}.`
      else "This system cannot ask a program to quit. Use Force Terminate."
  end
  return if err.message ~= "" then err.message else `{name} could not be stopped, and the system did not say why.`
end

local LOST = "Choose the program again: Processes no longer holds it."

-- Whether the window may offer to end the program, and if not, why: looking at a program never needs the right to end it.
local function controlOf(selection: pins.Selection): processes_Control
  local process = selection.process
  if not process:alive() then
    return { available = false, reason = `{process.name} has quit.` }
  end
  local blocked = selection.blocked
  if blocked then
    return { available = false, reason = sentence(blocked, process.name, true) }
  end
  return { available = true }
end

-- What is shown of a choice. A program only just seen is shown as it is, but not held (`unknown_identity`): once a later
-- reading has told it apart, the reading has another observation than the one the person chose, so what is shown is
-- that program, read, and it still cannot be ended through this choice: the person chooses it again.
local function detailOf(selection: pins.Selection, current: vnd_sysinfo_System): processes_Detail
  local process = selection.process
  local blocked = selection.blocked
  if blocked and blocked.kind == "unknown_identity" and not process:alive() then
    local live = current:process(process.pid)
    if live and live.start_time == process.start_time then
      return monitor.detail(live, { available = false, reason = sentence(blocked, live.name, true) })
    end
  end
  return monitor.detail(process, controlOf(selection))
end

RPC.on_request("_snapshot", function(params: processes_SnapshotParams, ctx: vos_app_Context): processes_SnapshotResult
  local current, problem = monitor.refresh()
  if not current then
    return ctx:fail(problem or "This machine could not be measured.")
  end
  pins.sweep()
  local overview, processes = monitor.read(current)
  local snapshot: processes_Snapshot = { overview = overview, processes = processes }
  local selected = params.selected
  if selected then
    -- the process as the window chose it, even after it has quit; an id that is not held is said so, and never looked up by PID
    local selection = pins.get(selected)
    if selection then
      snapshot.detail = detailOf(selection, current)
    else
      snapshot.selectionLost = true
    end
  end
  return snapshot
end)

RPC.on_request("_select", function(params: processes_SelectParams, ctx: vos_app_Context): processes_SelectResult
  local current, problem = monitor.system()
  if not current then
    return ctx:fail(problem or "This machine could not be measured.")
  end
  local process = monitor.find(current, params.process)
  if not process then
    return ctx:fail("That program has quit.")
  end
  local selection, err = pins.hold(process)
  if not selection then
    return ctx:fail(sentence(err :: vnd_sysinfo_Error, process.name, false))
  end
  return { selection = selection.id, detail = detailOf(selection, current) }
end)

RPC.on_request("_release", function(params: processes_ReleaseParams): processes_ReleaseResult
  pins.release(params.selection)
  return { released = true }
end)

RPC.on_request(
  "_terminate",
  function(params: processes_TerminateParams, ctx: vos_app_Context): processes_TerminateResult
    -- only what this window chose, and still holds: nothing is chosen again on the way
    local selection = pins.get(params.selection)
    if not selection then
      return ctx:fail(LOST)
    end
    local process = selection.process
    local blocked = selection.blocked
    if blocked then
      return ctx:fail(sentence(blocked, process.name, params.force))
    end
    local err
    if params.force then
      local _, killed = process:kill()
      err = killed
    else
      local _, asked = process:terminate()
      err = asked
    end
    if err then
      return ctx:fail(sentence(err, process.name, params.force))
    end
    return { sent = true, force = params.force }
  end
)
