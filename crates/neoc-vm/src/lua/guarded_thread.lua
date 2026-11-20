-- The Lua halves of `guard.rs`: `convert`, which makes the text of an error value, and `drive`, the body of a thread
-- that runs a coroutine of the script's own for the host.
--
-- When a thread the host resumes dies of an error, mlua turns the error value into text with `luaL_tolstring`, which
-- runs the value's `__tostring`, under a message handler of its own. On LuaJIT that handler is re-entered by its own
-- failure: a `__tostring` that makes a protected call of its own and then errors is called again and again until the
-- C stack is gone, and the process aborts (`guard.rs` has the whole story; `guarded_xpcall.lua` and `guarded_call.lua` close
-- the same recursion for `xpcall` and for a synchronous host call). Neither a Lua `pcall`
-- around the call nor any hook in mlua can stop that, so the error must not leave the thread as a value at all.
--
-- The body of a thread for script code is the builtin `xpcall`, called with the function and `handler`, which runs
-- at the point of the error, while the stack is intact: it has `guard.rs` take the traceback there (the text mlua
-- would have taken from the dead thread) and hands the error value on without calling anything of the script's.
-- `xpcall` returns it (the thread ends normally, with `false` and the value) and the host makes the text with
-- `convert`, a `tostring` that a failing `__tostring` cannot escape.

local report, pcall, tostring, type, select, unpack, resume, yield, status = ...

-- A userdata is a Rust error (what a host function that failed raised) or a Rust panic: mlua knows what to do
-- with it, and it has no `__tostring` of a script's to run. It goes on as it is, without being looked at (looking at
-- a panic from Rust would raise it here, in the handler).
local function handler(e)
  if type(e) == "userdata" then
    return e
  end
  return report(e)
end

-- The text of an error value that is not a string. A value whose conversion fails is reported as that, not
-- hidden: its `__tostring` ran, and failed.
local function convert(value)
  local converted, shown = pcall(tostring, value)
  if converted and type(shown) == "string" then
    return shown
  elseif converted then
    return "error object's __tostring did not return a string"
  elseif type(shown) == "string" then
    return "error object's __tostring failed: " .. shown
  end
  return "error object's __tostring failed"
end

-- A coroutine of the script's own that the host drives (`task.spawn(coroutine.create(f))`): the host cannot
-- put a handler under it, so it is resumed from here, what it yields is passed on to the host, and the host
-- resumes this thread (and with it the coroutine) when it is ready. A failure ends the coroutine, whose
-- stack is no longer there to take a traceback from; what this thread returns is `false` and the value.
local function pack(...)
  return { n = select("#", ...), ... }
end

local function drive(co, ...)
  local r = pack(resume(co, ...))
  while r[1] and status(co) == "suspended" do
    yield(unpack(r, 2, r.n))
    r = pack(resume(co))
  end
  if not r[1] and type(r[2]) == "table" then
    -- as the message handler answers: a table the script made is not told from the answer
    return false, { r[2] }
  end
  return unpack(r, 1, r.n)
end

return handler, convert, drive
