-- The Lua halves of `guard.rs`: `handler`, the message handler of the thread body; `convert` and `callmeta`, which make the
-- text of an error value; and `drive`, the body of a thread that runs a coroutine of the script's own for the host.
--
-- When a thread the host resumes dies of an error, mlua turns the error value into text with `luaL_tolstring`, which
-- runs the value's `__tostring`, under a message handler of its own. On LuaJIT that handler is re-entered by its own
-- failure: a `__tostring` that makes a protected call of its own and then errors is called again and again until the
-- C stack is gone, and the process aborts (`guard.rs` has the whole story; `guarded_xpcall.lua` closes the same recursion
-- for the `xpcall` a script calls). Neither a Lua `pcall`
-- around the call nor any hook in mlua can stop that, so the error must not leave the thread as a value at all.
--
-- The body of a thread for script code is the builtin `xpcall`, called with the function and `handler`, which runs
-- at the point of the error, while the stack is intact: it has `guard.rs` take the traceback there (the text mlua
-- would have taken from the dead thread) and hands the error value on without calling anything of the script's.
-- `xpcall` returns it (the thread ends normally, with `false` and the value) and the host makes the text with
-- `convert` or `callmeta`, which a failing `__tostring` cannot escape.

local report, pcall, tostring, type, select, unpack, resume, yield, status = ...

-- A userdata goes on as it is, without being looked at: it is most often a Rust error (what a host function that failed
-- raised) or a Rust panic, which mlua knows what to do with and which has no `__tostring` of a script's to run (looking
-- at a panic from Rust would raise it here, in the handler). The host tells a Rust error from another userdata when the
-- thread has returned it (`guard.rs`: such a value is then described there, without a traceback).
local function handler(e)
  if type(e) == "userdata" then
    return e
  end
  return report(e)
end

-- The text of a value that is not a string and has no `__tostring` of its own to run: `nil`, a boolean, a number.
local function convert(value)
  local converted, shown = pcall(tostring, value)
  if converted and type(shown) == "string" then
    return shown
  end
  return "error object's __tostring failed"
end

-- The text of a value from its `__tostring` metamethod `f`, as `luaL_tolstring` takes it (a string, or a number, which
-- it accepts), with a `pcall` around the call: a `__tostring` that fails is reported as that, with its reason when
-- the reason is text, and cannot fail the host.
local function callmeta(f, value)
  local ok, text = pcall(f, value)
  if not ok then
    if type(text) == "string" then
      return "error object's __tostring failed: " .. text
    end
    return "error object's __tostring failed"
  end
  local kind = type(text)
  if kind == "string" then
    return text
  elseif kind == "number" then
    return tostring(text)
  end
  -- (luaL_tolstring's own complaint, which is the text of the error)
  return "'__tostring' must return a string"
end

-- A coroutine of the script's own that the host drives (`task.spawn(coroutine.create(f))`): the host cannot
-- put a handler under it, so it is resumed from here, what it yields is passed on to the host, and the host
-- resumes this thread (and with it the coroutine) when it is ready. A failure is returned as `false` and the value
-- (boxed, as the message handler answers). There is no handler at the error, and no `debug` library to ask for the
-- traceback of the coroutine that died, so the text of its error has none.
local function pack(...)
  return { n = select("#", ...), ... }
end

local function drive(co, ...)
  local r = pack(resume(co, ...))
  while r[1] and status(co) == "suspended" do
    yield(unpack(r, 2, r.n))
    r = pack(resume(co))
  end
  if r[1] then
    return unpack(r, 1, r.n)
  elseif type(r[2]) == "userdata" then
    return false, r[2]
  end
  -- boxed, so that a value the script made is not mistaken for the host's own answers
  return false, { r[2] }
end

return handler, convert, callmeta, drive
