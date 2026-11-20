-- `xpcall` that bounds how often a failing message handler is run again (installed by `sandbox::base_engine`).
--
-- LuaJIT runs an `xpcall` handler at the point of the error and keeps the handler installed while it runs: once
-- the handler has made a protected call of its own (`pcall`, or a host function that does), an error that leaves
-- the handler is handled *by the handler again*, with that error. When the second run succeeds that is the result
-- `xpcall` gives (the handler's own bug is handled like the original error, and a script may rely on it). When it
-- never does,
--   xpcall(f, function(e) pcall(error, "x") error("again") end)
-- recurses until LuaJIT's own limit ("stack overflow"), thousands of nested C frames deep: past the stack of a
-- 2 MiB thread, so the process aborts.
--
-- The handler runs inside a wrapper that counts the runs that are on the C stack together. Up to `LIMIT` it
-- is `xpcall` as LuaJIT has it, the re-runs included; past `LIMIT` the wrapper answers `error in error handling`
-- (what LuaJIT answers when a handler fails without a protected call of its own) instead of running the
-- handler again. The protected function stays the builtin's, so a yield across it works.
--
-- The wrapper is made once per handler (a weak cache), so a handler that is used again and again costs a table
-- lookup, not a closure, per call. A handler made fresh for every call is made fresh here too.

local raw_xpcall, type, setmetatable = xpcall, type, setmetatable
local running = coroutine.running

-- 24 runs are far more than a handler that fails and is handled once or twice needs, and far below what a
-- 256 KiB stack holds (the guard is tested on one).
local LIMIT = 24

-- How many wrapper runs are on the C stack of a coroutine now, one count per coroutine (a weak table; `MAIN` stands
-- for the main thread, which `running` answers `nil` for). A run that raises leaves its count up; the `xpcall` that
-- started the chain puts it back (`done`), so a failure never carries over. The count is per coroutine because
-- LuaJIT yields across `xpcall`: an `xpcall` suspended in one coroutine and finished later, from inside a handler
-- run by another, must not write its own old count over this one's.
local MAIN = {}
local depths = setmetatable({}, { __mode = "k" })

local cache = setmetatable({}, { __mode = "kv" })

local function wrap(handler)
  return function(e)
    local thread = running() or MAIN
    local mine = depths[thread] or 0
    if mine >= LIMIT then
      return "error in error handling"
    end
    depths[thread] = mine + 1
    local handled = handler(e)
    depths[thread] = mine
    return handled
  end
end

local function done(thread, saved, ...)
  depths[thread] = saved
  return ...
end

local function guarded_xpcall(f, handler, ...)
  local wrapped = cache[handler]
  if wrapped == nil then
    if type(handler) ~= "function" then
      return raw_xpcall(f, handler, ...)
    end
    wrapped = wrap(handler)
    cache[handler] = wrapped
  end
  local thread = running() or MAIN
  return done(thread, depths[thread], raw_xpcall(f, wrapped, ...))
end

_G.xpcall = guarded_xpcall
