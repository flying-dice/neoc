-- `xpcall` whose message handler cannot fail the error handling (installed by `sandbox::base_engine`).
--
-- LuaJIT runs an `xpcall` handler at the point of the error, and keeps the handler installed while it runs: once the
-- handler has made a protected call of its own (`pcall`, or a host function that does), an error that leaves the
-- handler is handled *by the handler again*, and so on until LuaJIT's own limit ("stack overflow") ends it. That
-- limit is thousands of nested C frames deep: past the stack of a 2 MiB thread, so a script that writes
--   xpcall(f, function(e) pcall(error, "x") error("again") end)
-- aborts the process. A yield in the handler is such an error as well (a handler cannot yield: it runs on the C
-- stack of the error), and so is any host call that waits.
--
-- The handler therefore runs under a `pcall` of its own, so nothing leaves it: a handler that fails answers what
-- LuaJIT answers for a handler that fails without a protected call of its own, "error in error handling".
-- `xpcall` stays the engine's own, so a yield across it (from `f`) works as before; only the handler is wrapped.

local raw_xpcall, raw_pcall, type = xpcall, pcall, type

local function guarded_xpcall(f, handler, ...)
  if type(handler) ~= "function" then
    return raw_xpcall(f, handler, ...)
  end
  return raw_xpcall(f, function(e)
    local ok, handled = raw_pcall(handler, e)
    if ok then
      return handled
    end
    return "error in error handling"
  end, ...)
end

_G.xpcall = guarded_xpcall
