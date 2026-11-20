-- `coroutine.create` / `resume` / `wrap` for scripts whose host calls are async (installed by `script_env::new_env`).
--
-- An async host function that is not ready suspends the coroutine it runs in by yielding a private marker
-- (`pending`); the embedder's executor polls the future and resumes the coroutine when it is ready. That
-- works as long as the coroutine is the one the executor resumed. When a script runs a host call inside a
-- coroutine of its own, the marker would instead come back from the script's `coroutine.resume` as a
-- "yielded value" and the call would never finish.
--
-- These versions tell the two apart: a marker yield is passed outward (yielded from the coroutine that called
-- `resume`, so it reaches the executor through any depth of nesting) and the inner coroutine is resumed with
-- whatever answer comes back; any other yield is the script's own and is returned from `resume`, as usual.
--
-- A call that is dropped while it waits (cancellation, a timeout) is never resumed: mlua drops the thread the
-- executor was polling and nothing runs in it again, so no script code runs after the drop. The host call the
-- inner coroutine was waiting at is abandoned by the call's end (`lifecycle`, rule 3): its future is dropped and
-- whatever it held is released, and a coroutine of the script's own that is resumed later finds the call gone
-- ("this host call was abandoned") instead of running it. (mlua also has a terminate signal for a parked async
-- wrapper; it is only sent when the parked frame is mlua's own wrapper, never to the yield below, so nothing
-- here handles it.)
--
-- LuaJIT yields across `pcall`, `xpcall`, metamethods and `for ... in` iterator functions, so a host call in any of
-- those suspends the call like any other. It still cannot yield out of a C function that calls back into Lua (a
-- `string.gsub` callback, a `table.sort` comparator), `string.format("%s")` of a `__tostring`, the message handler
-- of an `xpcall` or a host function that calls Lua synchronously: a host call there fails with Lua's own "attempt to
-- yield across C-call boundary".
--
-- WHO MAY RESUME WHAT. A script reaches threads it did not make: `coroutine.running()` returns the thread it runs in,
-- which may be the top-level thread of a call the host is driving or a task's thread (a `pcall` runs in the thread that
-- called it, so inside a coroutine of the script's own it is that coroutine, which the script may resume). Resuming one of those from another call would run its code in the wrong call (a
-- call acting under another call's restrictions, a task that never finishes). So a script drives only what it made itself: `create` and
-- `wrap` record the coroutine in `owned` (weak keys), and `resume` refuses everything else with Lua's own "cannot resume
-- non-suspended coroutine", as it does for a coroutine that is running. The refusal is by default, so no thread the host
-- or a module makes internally has to be found and marked: the host machinery uses the raw primitives captured below.
-- A coroutine of its own is refused only while somebody else drives it (`driven`, see `is_driven`).

local pending, raw, driven, owned = ...
local select, unpack, error, type = select, unpack, error, type
local create, resume, yield, status, running = raw.create, raw.resume, raw.yield, raw.status, raw.running

-- A coroutine whose host call is in flight is *running*, as far as the script can tell: the thread that resumed it
-- waits for it. `driven` (a table with weak keys *and* weak values, shared with the task scheduler) maps such a
-- coroutine to that thread; a script that resumes it from somewhere else (a task, another call) gets Lua's own
-- "cannot resume non-suspended coroutine" instead of stealing the call from the thread that is waiting for it. A
-- coroutine handed to `task.spawn`, `task.defer` or `task.delay` is the scheduler's: its entry is `true`. A thread's
-- entry means something only while that thread is suspended: if its yield failed (it was inside a C boundary) the
-- thread went on, and the coroutine is merely left behind.
--
-- A resumer whose call was dropped is suspended for ever, and the status check cannot tell that from one that is
-- waiting for its wake-up. The weak value can: the thread is reachable only through the executor's call (and the
-- coroutines that resumed it), so once that call is dropped nothing refers to the thread, the garbage collector
-- clears the entry, and the coroutine can be resumed again (it finds its host call abandoned). Clearing an entry for
-- a thread nothing can resume allows no second driver, and a strong value would keep the dead thread, its stack
-- and the coroutine (Lua 5.1 and LuaJIT have no ephemerons: the value refers back to the key) for ever.
local function is_driven(co)
  local driver = driven[co]
  if driver == nil then
    return false
  end
  if driver == true or status(driver) == "suspended" then
    return true
  end
  driven[co] = nil
  return false
end

-- Whether a script may resume `co`: it made it, and nobody else is driving it.
local function resumable(co)
  return owned[co] == true and not is_driven(co)
end

local function pack(...)
  return { n = select("#", ...), ... }
end

-- Resume `co` with the packed result `r` of its latest resume until it finishes, fails, or yields a value of
-- its own. Returns the packed `resume` result.
local function drive(co, r)
  while r[1] and r.n == 2 and r[2] == pending and status(co) == "suspended" do
    local me = running()
    driven[co] = me
    yield(pending)
    -- the scheduler may have taken the coroutine over meanwhile: only take back what is ours
    if driven[co] == me then
      driven[co] = nil
    end
    r = pack(resume(co))
  end
  return r
end

local M = {}

function M.create(f)
  local co = create(f)
  owned[co] = true
  return co
end

function M.resume(co, ...)
  if type(co) ~= "thread" then
    return resume(co, ...) -- Lua's own "bad argument"
  end
  if not resumable(co) then
    return false, "cannot resume non-suspended coroutine"
  end
  local r = drive(co, pack(resume(co, ...)))
  return unpack(r, 1, r.n)
end

function M.wrap(f)
  local co = M.create(f)
  return function(...)
    if is_driven(co) then
      error("cannot resume non-suspended coroutine", 0)
    end
    local r = drive(co, pack(resume(co, ...)))
    if not r[1] then
      error(r[2], 0)
    end
    return unpack(r, 2, r.n)
  end
end

return M
