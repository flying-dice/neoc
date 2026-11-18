-- `coroutine.resume` / `coroutine.wrap` for scripts whose host calls are async (installed by `app_env::new_env`).
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
-- Lua 5.1 still cannot yield out of a C boundary (a `table.sort` comparator, a `string.gsub` callback, a
-- metamethod or a `for ... in` iterator function): a host call there fails with Lua's own
-- "attempt to yield across metamethod/C-call boundary".

local pending, raw = ...
local select, unpack, error, type = select, unpack, error, type
local create, resume, yield, status = raw.create, raw.resume, raw.yield, raw.status

local function pack(...)
  return { n = select("#", ...), ... }
end

-- Resume `co` with the packed result `r` of its latest resume until it finishes, fails, or yields a value of
-- its own. Returns the packed `resume` result.
local function drive(co, r)
  while r[1] and r.n == 2 and r[2] == pending and status(co) == "suspended" do
    local signal = yield(pending)
    if signal ~= nil then
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

function M.resume(co, ...)
  local r = drive(co, pack(resume(co, ...)))
  return unpack(r, 1, r.n)
end

function M.wrap(f)
  local co = create(f)
  return function(...)
    local r = drive(co, pack(resume(co, ...)))
    if not r[1] then
      error(r[2], 0)
    end
    return unpack(r, 2, r.n)
  end
end

return M
