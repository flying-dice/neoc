-- The prelude. Apps are Luau, and the app's environment (neoc-vm's `script_env`) copies only a few things from the
-- engine: the base functions, `string`, `table`, `math`, `coroutine` and `bit32`. Two members of Luau's own standard
-- library are not among them, and apps (and the Luau declarations luau-lsp checks them against) expect both, so
-- they are provided here: `typeof` and the `utf8` library, with Lua 5.3's semantics.
--
-- What it used to polyfill for Lua 5.1 source and no longer does: `table.pack`, `table.clone`, `table.move` and
-- `table.unpack`. They are Luau's own and the environment's copy of `table` has them (these definitions were
-- already skipped, `define` never overwrites), so an app gets the real ones.
--
-- The host runs this file once per app, before any app chunk or `vos:ui`, and passes the table those chunks
-- will run in as the chunk's `...` argument (`types/generated/prelude.lua`, copied by `bun run codegen`). It
-- installs into that table rather than reaching for `getfenv()`, which an app's sandbox does not have. Nothing
-- is overwritten: a member the environment already has is left alone.
--
-- The `utf8` functions follow Lua 5.3's strict semantics: they reject overlong forms, surrogates and code points
-- above U+10FFFF (`scripts/luau.test.ts` compares them with a native Lua 5.3+ when one is installed).

local env = ...

local function define(library, name, value)
  if rawget(library, name) == nil then
    library[name] = value
  end
end

-- `typeof` is Luau's, for code written against Luau. Here it is `type`: the library modules return userdata (a JSON
-- `Object`, a `Duration`, a `TcpListener`, ...), and every one of them is `"userdata"`, because their metatables
-- are locked and name no type. Tell them apart by what they do, not by `typeof`.
define(env, "typeof", type)

local function argError(position, name, message)
  return error("bad argument #" .. position .. " to '" .. name .. "' (" .. message .. ")", 3)
end

local function checkInteger(position, name, value, default)
  if value == nil and default ~= nil then
    return default
  end
  local number = tonumber(value)
  if number == nil then
    argError(position, name, "number expected, got " .. type(value))
  end
  if math.floor(number) ~= number then
    argError(position, name, "number has no integer representation")
  end
  return number
end

-- ── utf8 ─────────────────────────────────────────────────────────────────

local utf8Library = {}

-- The bytes of "[\0-\x7F\xC2-\xF4][\x80-\xBF]*", written the Lua 5.1 way (`%z` for NUL), without byte escapes
-- that no source tool would keep intact.
utf8Library.charpattern = "[%z\1-\127"
  .. string.char(194)
  .. "-"
  .. string.char(244)
  .. "]["
  .. string.char(128)
  .. "-"
  .. string.char(191)
  .. "]*"

local MAX_CODE_POINT = 0x10FFFF
-- The smallest code point each sequence length may encode (shorter encodings of it are overlong).
local LOWEST = { [1] = 0x80, [2] = 0x800, [3] = 0x10000 }

local function isContinuation(text, position)
  local byte = string.byte(text, position)
  return byte ~= nil and byte >= 128 and byte <= 191
end

-- Decodes the code point starting at byte `position` (which must exist): its value and the position after it,
-- or nil when the bytes there are not valid UTF-8.
local function decode(text, position)
  local lead = string.byte(text, position)
  if lead < 128 then
    return lead, position + 1
  end
  local extra, code
  if lead >= 240 and lead < 248 then
    extra, code = 3, lead - 240
  elseif lead >= 224 and lead < 240 then
    extra, code = 2, lead - 224
  elseif lead >= 192 and lead < 224 then
    extra, code = 1, lead - 192
  else
    return nil, position
  end
  for step = 1, extra do
    if not isContinuation(text, position + step) then
      return nil, position
    end
    code = code * 64 + string.byte(text, position + step) - 128
  end
  if code < LOWEST[extra] or code > MAX_CODE_POINT or (code >= 0xD800 and code <= 0xDFFF) then
    return nil, position
  end
  return code, position + extra + 1
end

-- A relative position (negative counts from the end) as an absolute one; 0 when it falls off the front.
local function absolute(position, length)
  if position >= 0 then
    return position
  elseif -position > length then
    return 0
  end
  return length + position + 1
end

define(utf8Library, "len", function(text, first, last)
  local source = tostring(text)
  local length = #source
  local from = absolute(checkInteger(2, "len", first, 1), length)
  local upTo = absolute(checkInteger(3, "len", last, -1), length)
  if from < 1 or from - 1 > length then
    argError(2, "len", "initial position out of bounds")
  end
  if upTo - 1 >= length then
    argError(3, "len", "final position out of bounds")
  end
  local count = 0
  while from <= upTo do
    local code, following = decode(source, from)
    if code == nil then
      return nil, from
    end
    from = following
    count = count + 1
  end
  return count
end)

define(utf8Library, "offset", function(text, count, start)
  local source = tostring(text)
  local length = #source
  local remaining = checkInteger(2, "offset", count)
  local fallback = length + 1
  if remaining > 0 then
    fallback = 1
  end
  local position = absolute(checkInteger(3, "offset", start, fallback), length)
  if position < 1 or position - 1 > length then
    argError(3, "offset", "position out of bounds")
  end
  if remaining == 0 then
    while position > 1 and isContinuation(source, position) do
      position = position - 1
    end
    return position
  end
  if isContinuation(source, position) then
    error("initial position is a continuation byte", 2)
  end
  if remaining < 0 then
    while remaining < 0 and position > 1 do
      repeat
        position = position - 1
      until position <= 1 or not isContinuation(source, position)
      remaining = remaining + 1
    end
  else
    remaining = remaining - 1
    while remaining > 0 and position <= length do
      repeat
        position = position + 1
      until not isContinuation(source, position)
      remaining = remaining - 1
    end
  end
  if remaining == 0 then
    return position
  end
  return nil
end)

define(utf8Library, "codepoint", function(text, first, last)
  local source = tostring(text)
  local length = #source
  local from = absolute(checkInteger(2, "codepoint", first, 1), length)
  local upTo = absolute(checkInteger(3, "codepoint", last, from), length)
  if from < 1 then
    argError(2, "codepoint", "out of bounds")
  end
  if upTo > length then
    argError(3, "codepoint", "out of bounds")
  end
  local codes = {}
  while from <= upTo do
    local code, following = decode(source, from)
    if code == nil then
      error("invalid UTF-8 code", 2)
    end
    table.insert(codes, code)
    from = following
  end
  return env.unpack(codes)
end)

define(utf8Library, "codes", function(text)
  local source = tostring(text)
  local function step(subject, previous)
    local position = previous + 1
    while isContinuation(subject, position) do
      position = position + 1
    end
    if position > #subject then
      return nil, nil
    end
    local code = decode(subject, position)
    if code == nil then
      error("invalid UTF-8 code", 2)
    end
    return position, code
  end
  return step, source, 0
end)

local function encode(code)
  if code < 128 then
    return string.char(code)
  elseif code < 0x800 then
    return string.char(192 + math.floor(code / 64), 128 + code % 64)
  elseif code < 0x10000 then
    return string.char(224 + math.floor(code / 4096), 128 + math.floor(code / 64) % 64, 128 + code % 64)
  end
  return string.char(
    240 + math.floor(code / 262144),
    128 + math.floor(code / 4096) % 64,
    128 + math.floor(code / 64) % 64,
    128 + code % 64
  )
end

define(utf8Library, "char", function(...)
  local pieces = {}
  for index = 1, select("#", ...) do
    local code = checkInteger(index, "char", (select(index, ...)))
    if code < 0 or code > MAX_CODE_POINT then
      argError(index, "char", "value out of range")
    end
    pieces[index] = encode(code)
  end
  return table.concat(pieces)
end)

define(env, "utf8", utf8Library)
