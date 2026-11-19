-- `coroutine.create` / `resume` / `wrap` for scripts whose host calls are async (installed by `app_env::new_env`).
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
-- When the executor drops a call that is waiting (cancellation), it resumes the coroutine with a termination
-- signal instead of an empty answer. The signal is handed to the inner coroutine so that its future is
-- dropped, and this coroutine then stays suspended: control must never return into the script.
--
-- Lua 5.1 still cannot yield out of a C boundary (a `string.gsub` callback, a
-- metamethod or a `for ... in` iterator function): a host call there fails with Lua's own
-- "attempt to yield across metamethod/C-call boundary".
--
-- WHO MAY RESUME WHAT. A script reaches threads it did not make: `coroutine.running()` returns the thread it runs in,
-- which may be the top-level thread of a call the host is driving, a task's thread, or the protected coroutine of a
-- `pcall` (`yieldable_pcall.lua`). Resuming one of those from another call would run its code in the wrong call (a
-- `readOnly` call writing state, a task that never finishes). So a script drives only what it made itself: `create` and
-- `wrap` record the coroutine in `owned` (weak keys), and `resume` refuses everything else with Lua's own "cannot resume
-- non-suspended coroutine", as it does for a coroutine that is running. The refusal is by default, so no thread the host
-- or a module makes internally has to be found and marked: the host machinery uses the raw primitives captured below.
-- A coroutine of its own is refused only while somebody else drives it (`driven`, see `is_driven`).

local pending, raw, driven, owned, null = ...
local select, unpack, error, type, getmetatable = select, unpack, error, type, getmetatable
local create, resume, yield, status, running = raw.create, raw.resume, raw.yield, raw.status, raw.running

-- A coroutine whose host call is in flight is *running*, as far as the script can tell: the thread that resumed it
-- waits for it. `driven` (a table with weak keys, shared with the task scheduler) maps such a coroutine to that
-- thread; a script that resumes it from somewhere else (a task, another call) gets Lua's own "cannot resume
-- non-suspended coroutine" instead of stealing the call from the thread that is waiting for it. A coroutine handed
-- to `task.spawn`, `task.defer` or `task.delay` is the scheduler's: its entry is `true`. A thread's entry means
-- something only while that thread is suspended: if its yield failed (it was inside a C boundary) the thread went
-- on, and the coroutine is merely left behind.
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

-- The executor drops a call that is waiting by resuming the thread with a light userdata of its own (the
-- "terminate" signal). The only light userdata a script can hold is the null sentinel (`null`); `pending` is the
-- marker threads yield, never an answer, so it is no signal either. That is how the signal is told from anything a
-- script resumed the thread with: a resume value that is nothing of the kind changes nothing.
local function is_terminate(signal)
  return type(signal) == "userdata" and signal ~= null and signal ~= pending and getmetatable(signal) == nil
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
    local signal = yield(pending)
    -- the scheduler may have taken the coroutine over meanwhile: only take back what is ours
    if driven[co] == me then
      driven[co] = nil
    end
    if is_terminate(signal) then
      resume(co, signal) -- the executor is dropping this call: let the inner future go
      while true do
        yield()
      end
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
