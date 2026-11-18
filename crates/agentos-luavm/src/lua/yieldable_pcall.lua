-- Yield-safe `pcall` / `xpcall` for standard Lua 5.1 (installed by `sandbox::make_engine`).
--
-- PUC Lua 5.1 cannot yield across a `pcall`: `lua_pcall` is a C call, so an async wait (`task.wait`, any
-- async host function) inside a protected call fails with "attempt to yield across metamethod/C-call
-- boundary". LuaJIT allows it, and the task scheduler depends on it. This is the classic "coxpcall"
-- technique: run the protected function in its own coroutine, and when it yields, yield the same values
-- out of the calling coroutine and resume with whatever comes back.
--
-- Differences from the built-in, all of them edge cases: `xpcall` also forwards extra arguments (as in
-- Lua 5.2+), the message handler runs after the error has unwound the protected call rather than at the
-- error site (there is no `debug.traceback` in the sandbox to need the difference), and a non-callable `f`
-- reports "attempt to call upvalue 'f'" instead of "attempt to call a nil value".

local select, unpack, type = select, unpack, type
local raw_pcall = pcall
local create, resume, yield, status, running =
  coroutine.create, coroutine.resume, coroutine.yield, coroutine.status, coroutine.running

local function pack(...)
  return { n = select("#", ...), ... }
end

-- Resume `co` until it finishes or fails; `r` is the packed result of its first resume.
local function drive(co, handler, r)
  while true do
    if not r[1] then
      co = nil
      if handler then
        local ok, shaped = raw_pcall(handler, r[2])
        -- an erroring handler is "error in error handling", as in the built-in
        return false, ok and shaped or "error in error handling"
      end
      return false, r[2]
    end
    if status(co) == "dead" then
      return true, unpack(r, 2, r.n)
    end
    if not running() then
      return false, "attempt to yield from outside a coroutine"
    end
    -- the protected function yielded: yield the same values outward, resume it with the answer
    r = pack(resume(co, yield(unpack(r, 2, r.n))))
  end
end

local function protected(f)
  -- a closure, because `coroutine.create` only accepts Lua functions and `f` may be a host function
  return create(function(...)
    return f(...)
  end)
end

local function yieldable_pcall(f, ...)
  local co = protected(f)
  return drive(co, nil, pack(resume(co, ...)))
end

local function yieldable_xpcall(f, handler, ...)
  local co = protected(f)
  return drive(co, handler, pack(resume(co, ...)))
end

_G.pcall = yieldable_pcall
_G.xpcall = yieldable_xpcall
