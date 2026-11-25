--!strict
-- The Processes app's backend: the machine's load and its programs, read through `vnd:sysinfo` (monitor.luau), and
-- two ways of ending a program the person has chosen (pins.luau). The windows ask for a reading as often as the
-- person lets them (every second, two, five, or not at all), and every answer is the whole reading, so what a window
-- draws is what the backend has.
--
-- Ending a program is the one thing here that can do harm, so it is narrow: it is only ever the program the person
-- chose, held by a reference that a reused PID cannot move; asking it to quit (`terminate`) is never turned into
-- forcing it; and what the operating system refused is told to the person in a sentence.
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
  elseif kind == "permission_denied" then
    return `This system did not let Processes stop {name}. It belongs to a user with more rights than yours: ask an administrator.`
  elseif kind == "unsupported" then
    return if force
      then `This system cannot stop {name}.`
      else "This system cannot ask a program to quit. Use Force Terminate."
  end
  return if err.message ~= "" then err.message else `{name} could not be stopped, and the system did not say why.`
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
      -- the process as the person chose it, even after it has quit; one that was not chosen, as it is now if it is
      local process = pins.get(selected) or monitor.find(current, selected)
      if process then
        snapshot.detail = monitor.detail(process)
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
    -- a start time the platform did not give (some system processes) cannot tell this process from one that is given its PID
    if process.start_time == 0 then
      return ctx:fail(
        `Processes cannot tell {process.name} apart from another program that is given its number, so it will not stop it.`
      )
    end
    local err = pins.hold(process)
    if err then
      return ctx:fail(sentence(err, process.name, false))
    end
    return monitor.detail(process)
  end,

  release = function(_ctx, params)
    pins.release(params.process)
    return { released = true }
  end,

  terminate = function(ctx, params)
    local process = pins.get(params.process)
    if not process then
      return ctx:fail("Choose the program again: Processes no longer holds it.")
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
