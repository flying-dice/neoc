-- The trampoline of `sandbox::call_guarded`: call a script function from Rust under the engine's own `pcall`.
--
-- A synchronous `mlua::Function::call` runs the function under a message handler of mlua's that turns the error
-- value into text with `luaL_tolstring`, which runs the value's `__tostring`. On LuaJIT that is a handler re-entered
-- by its own failure: a `__tostring` that makes a protected call of its own and then errors is called again and
-- again until the C stack of the thread (2 MiB for a runtime worker) is gone, and the process aborts (see
-- `guarded_xpcall.lua` for the same thing in `xpcall`). The error must not reach that handler as a value with a
-- metatable: `pcall` here catches it, and what leaves is text, made with a `tostring` that a failing `__tostring`
-- cannot escape.

local pcall, tostring, type = pcall, tostring, type

local function finish(ok, ...)
  if ok then
    return true, ...
  end
  local e = ...
  if type(e) ~= "string" then
    local converted, text = pcall(tostring, e)
    e = converted and text or "<error object>"
  end
  return false, e
end

return function(f, ...)
  return finish(pcall(f, ...))
end
