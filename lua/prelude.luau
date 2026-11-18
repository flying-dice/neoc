--!strict
-- The Lua 5.1 prelude. Apps and `@agentos/ui` are authored in typed Luau and shipped as plain Lua 5.1 (see
-- `.darklua.json`), so the few Luau standard-library members they use that Lua 5.1 lacks are provided here:
-- `typeof`, `table.clone`, `table.move`, `table.pack`, `table.unpack` and the `utf8` library.
--
-- The host runs this file once, in the globals table every app chunk and `@agentos/ui` then run in, before any
-- of them (`types/generated/prelude.lua`, built by `bun run codegen`). Nothing is overwritten: a member the
-- interpreter already has is left alone, so this is a no-op on Luau and on Lua 5.3+.
--
-- The `utf8` functions follow Lua 5.3's strict semantics (which Luau shares): they reject overlong forms,
-- surrogates and code points above U+10FFFF. Apps must not use Luau-only members that are not listed here
-- (`table.find`, `math.clamp`, `bit32`, `buffer`, ...): `scripts/lua51.test.ts` fails when one appears.

local env: { [string]: any } = getfenv()

local function define(library: { [string]: any }, name: string, value: any)
  if rawget(library, name) == nil then
    library[name] = value
  end
end

-- Luau's `typeof` tells userdata types apart; the host hands apps none, so it is `type`.
define(env, "typeof", type)

local function argError(position: number, name: string, message: string): never
  return error(`bad argument #{position} to '{name}' ({message})`, 3)
end

local function checkInteger(position: number, name: string, value: any, default: number?): number
  if value == nil and default ~= nil then
    return default
  end
  local number = tonumber(value)
  if number == nil then
    argError(position, name, `number expected, got {type(value)}`)
  end
  local integer = number :: number
  if math.floor(integer) ~= integer then
    argError(position, name, "number has no integer representation")
  end
  return integer
end

-- ── table ────────────────────────────────────────────────────────────────

local tableLibrary: { [string]: any } = env.table

define(tableLibrary, "unpack", env.unpack)

define(tableLibrary, "pack", function(...: any): { [any]: any }
  return { n = select("#", ...), ... }
end)

-- A shallow copy that keeps the metatable.
define(tableLibrary, "clone", function(source: any): any
  if type(source) ~= "table" then
    argError(1, "clone", `table expected, got {type(source)}`)
  end
  local copy: { [any]: any } = {}
  for key, value in pairs(source) do
    copy[key] = value
  end
  local metatable = getmetatable(source)
  if type(metatable) == "table" then
    setmetatable(copy, metatable)
  end
  return copy
end)

-- Copies `source[first..last]` to `target[to..]` (Lua 5.3 `table.move`; the ranges may overlap).
define(tableLibrary, "move", function(source: any, first: any, last: any, to: any, target: any?): any
  local from = checkInteger(2, "move", first)
  local upTo = checkInteger(3, "move", last)
  local destination = checkInteger(4, "move", to)
  local into: any = if target == nil then source else target
  if upTo >= from then
    if destination > upTo or destination <= from or into ~= source then
      for offset = 0, upTo - from do
        into[destination + offset] = source[from + offset]
      end
    else
      for offset = upTo - from, 0, -1 do
        into[destination + offset] = source[from + offset]
      end
    end
  end
  return into
end)

-- ── utf8 ─────────────────────────────────────────────────────────────────

local utf8Library: { [string]: any } = {}

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

local function isContinuation(text: string, position: number): boolean
  local byte = string.byte(text, position)
  return byte ~= nil and byte >= 128 and byte <= 191
end

-- Decodes the code point starting at byte `position` (which must exist): its value and the position after it,
-- or nil when the bytes there are not valid UTF-8.
local function decode(text: string, position: number): (number?, number)
  local lead = string.byte(text, position) :: number
  if lead < 128 then
    return lead, position + 1
  end
  local extra: number
  local code: number
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
    code = code * 64 + (string.byte(text, position + step) :: number) - 128
  end
  if code < LOWEST[extra] or code > MAX_CODE_POINT or (code >= 0xD800 and code <= 0xDFFF) then
    return nil, position
  end
  return code, position + extra + 1
end

-- A relative position (negative counts from the end) as an absolute one; 0 when it falls off the front.
local function absolute(position: number, length: number): number
  if position >= 0 then
    return position
  elseif -position > length then
    return 0
  end
  return length + position + 1
end

define(utf8Library, "len", function(text: any, first: any, last: any): (number?, number?)
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
    count += 1
  end
  return count
end)

define(utf8Library, "offset", function(text: any, count: any, start: any): number?
  local source = tostring(text)
  local length = #source
  local remaining = checkInteger(2, "offset", count)
  local fallback = if remaining > 0 then 1 else length + 1
  local position = absolute(checkInteger(3, "offset", start, fallback), length)
  if position < 1 or position - 1 > length then
    argError(3, "offset", "position out of bounds")
  end
  if remaining == 0 then
    while position > 1 and isContinuation(source, position) do
      position -= 1
    end
    return position
  end
  if isContinuation(source, position) then
    error("initial position is a continuation byte", 2)
  end
  if remaining < 0 then
    while remaining < 0 and position > 1 do
      repeat
        position -= 1
      until position <= 1 or not isContinuation(source, position)
      remaining += 1
    end
  else
    remaining -= 1
    while remaining > 0 and position <= length do
      repeat
        position += 1
      until not isContinuation(source, position)
      remaining -= 1
    end
  end
  if remaining == 0 then
    return position
  end
  return nil
end)

define(utf8Library, "codepoint", function(text: any, first: any, last: any): ...number
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
  local codes: { number } = {}
  while from <= upTo do
    local code, following = decode(source, from)
    if code == nil then
      error("invalid UTF-8 code", 2)
    end
    table.insert(codes, code :: number)
    from = following
  end
  return env.unpack(codes)
end)

define(utf8Library, "codes", function(text: any): (any, string, number)
  local source = tostring(text)
  local function step(subject: string, previous: number): (number?, number?)
    local position = previous + 1
    while isContinuation(subject, position) do
      position += 1
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

local function encode(code: number): string
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

define(utf8Library, "char", function(...: any): string
  local pieces: { string } = {}
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
