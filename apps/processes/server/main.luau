--!strict
-- The Processes app's backend: the machine's load and its programs, read through `vnd:sysinfo` (monitor.luau), and
-- two ways of ending a program the person has chosen (pins.luau). The windows ask for a reading as often as the
-- person lets them (every second, two, five, or not at all), and every answer is the whole reading, so what a window
-- draws is what the backend has.
--
-- Ending a program is the one thing here that can do harm, so it is narrow: it is only ever the program a window chose,
-- through the id the backend issued for that choice, held by a reference that a reused PID cannot move; a choice that
-- is no longer held is refused, never made again; asking it to quit (`terminate`) is never turned into forcing it; and
-- what the operating system refused is told to the person in a sentence.
local monitor = require("./monitor")
local pins = require("./pins")
local rpc = require("../.app/rpc")

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
    return `Processes cannot tell {name} apart from another program that is given its number, so it will not stop it.`
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
local function controlOf(selection: pins.Selection): rpc.Control
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

local function detailOf(selection: pins.Selection): rpc.Detail
  return monitor.detail(selection.process, controlOf(selection))
end

local handlers: rpc.Handlers = {
  snapshot = function(ctx, params)
    local current, problem = monitor.refresh()
    if not current then
      return ctx:fail(problem or "This machine could not be measured.")
    end
    pins.sweep()
    local overview, processes = monitor.read(current)
    local snapshot: rpc.Snapshot = { overview = overview, processes = processes }
    local selected = params.selected
    if selected then
      -- the process as the window chose it, even after it has quit; an id that is not held is said so, and never looked up by PID
      local selection = pins.get(selected)
      if selection then
        snapshot.detail = detailOf(selection)
      else
        snapshot.selectionLost = true
      end
    end
    return snapshot
  end,

  select = function(ctx, params)
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
    return { selection = selection.id, detail = detailOf(selection) }
  end,

  release = function(_ctx, params)
    pins.release(params.selection)
    return { released = true }
  end,

  terminate = function(ctx, params)
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
  end,
}

return handlers
