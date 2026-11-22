--!strict
-- The keypad: one key at a time, as the iPadOS Calculator does it.
-- Pure: `press(state, key)` takes where a sum has got to and gives the next state; nothing is kept here. The client holds
-- the state and sends it with each key, so every window has a calculator of its own. The arithmetic is calc.luau.
local calc = require("./calc")

export type Op = "+" | "-" | "*" | "/"

-- `entry` is the text being typed; once a result is in (`fresh`) the number lives in `value`, so a result is never parsed
-- back from its own display text. The sum in progress is `numbers[i]` followed by `ops[i]` (always one operator per
-- number: `2 + 3 ×` is { 2, 3 } and { "+", "*" }). `waiting` is true from an operator key until a number is started:
-- the display then holds what that operator has completed, not an operand.
export type State = {
  entry: string,
  value: number,
  numbers: { number },
  ops: { Op },
  waiting: boolean,
  fresh: boolean,
  caption: string,
  err: boolean,
}

export type Screen = {
  state: State,
  display: string,
  caption: string,
  clear: "AC" | "C",
  pending: Op?,
  error: string?,
}

local keypad = {}

local MAX_DIGITS = 9

function keypad.initial(): State
  return {
    entry = "0",
    value = 0,
    numbers = {},
    ops = {},
    waiting = false,
    fresh = true,
    caption = "",
    err = false,
  }
end

-- The sum as calc.luau's flat list of terms (`{ 2, "+", 3, "*" }`).
local function termsOf(numbers: { number }, ops: { Op }): { number | string }
  local terms: { number | string } = {}
  for i, n in numbers do
    table.insert(terms, n)
    local op = ops[i]
    if op ~= nil then
      table.insert(terms, op)
    end
  end
  return terms
end

-- The number on the display: the result once one is in, else what has been typed. An entry that is not a number (a
-- state the client made up) is a failure like any other, not a crash.
local function current(s: State): number
  if s.fresh then
    return s.value
  end
  local n = tonumber(s.entry)
  if n == nil then
    error("not a number: " .. s.entry)
  end
  return n
end

-- Show a number (a result); the next digit starts a new entry.
local function show(s: State, value: number)
  s.value, s.fresh = value, true
end

local function digit(s: State, d: string)
  s.waiting = false -- a number is being started
  if s.fresh then
    s.entry, s.fresh = (d == "." and "0." or d), false
  elseif d == "." then
    if not string.find(s.entry, ".", 1, true) then
      s.entry ..= "."
    end
  elseif #string.gsub(s.entry, "[-.]", "") < MAX_DIGITS then
    s.entry = if s.entry == "0" then d else s.entry .. d
  end
end

-- An operator key adds the number on the display and the operator to the sum, and works out only what that operator
-- completes (`calc.progress`): `2 + 3 ×` still shows 3, `2 × 3 +` shows 6. Another operator before the next number
-- replaces the waiting one.
local function operator(s: State, op: Op)
  local numbers, ops = table.clone(s.numbers), table.clone(s.ops)
  if s.waiting and #ops > 0 then
    ops[#ops] = op
  else
    table.insert(numbers, current(s))
    table.insert(ops, op)
  end
  local terms = termsOf(numbers, ops)
  local reduced = calc.progress(terms) -- raises on division by zero, before anything changes
  s.numbers, s.ops, s.waiting = numbers, ops, true
  show(s, reduced)
  s.caption = calc.describe(terms) -- "9 × 3 −": the sum so far, in words as well as on the key
end

-- `=` adds the number on the display and evaluates the whole sum. With nothing in progress it does nothing; with an
-- operator waiting (`5 +`) the displayed number is the operand.
local function equals(s: State)
  if #s.numbers == 0 then
    return
  end
  local terms = termsOf(s.numbers, s.ops)
  table.insert(terms, current(s))
  local result = calc.total(terms) -- raises on division by zero and on a result too large
  s.caption = calc.describe(terms)
  s.numbers, s.ops, s.waiting = {}, {}, false
  show(s, result)
end

-- "C" clears what is being typed; "AC" (nothing typed) clears everything.
local function clear(s: State)
  if s.entry ~= "0" and not s.fresh then
    s.entry = "0"
    s.waiting = #s.numbers > 0 -- the sum goes on: the display is not an operand until a digit is typed
  else
    s.entry, s.numbers, s.ops, s.waiting, s.caption = "0", {}, {}, false, ""
  end
  s.err, s.value, s.fresh = false, 0, true
end

local function negate(s: State)
  s.waiting = false -- the number on the display is the operand now
  if s.fresh then
    s.value = if s.value == 0 then 0 else -s.value -- never "-0"
  elseif string.sub(s.entry, 1, 1) == "-" then
    s.entry = string.sub(s.entry, 2)
  elseif s.entry ~= "0" then
    s.entry = "-" .. s.entry
  end
end

local function percent(s: State)
  show(s, calc.combine(current(s), "/", 100))
  s.waiting = false
end

local function backspace(s: State)
  if not s.fresh then
    local rest = string.sub(s.entry, 1, -2)
    s.entry = if rest == "" or rest == "-" or rest == "-0" then "0" else rest
  end
end

local function apply(s: State, key: string)
  if string.find(key, "^[%d.]$") then
    digit(s, key)
  elseif key == "+" or key == "-" or key == "*" or key == "/" then
    operator(s, key :: Op)
  elseif key == "=" then
    equals(s)
  elseif key == "clear" then
    clear(s)
  elseif key == "negate" then
    negate(s)
  elseif key == "percent" then
    percent(s)
  elseif key == "backspace" then
    backspace(s)
  else
    error("unknown key " .. key)
  end
end

-- What to draw for a state. While the display shows Error, the caption is the reason, and `error` repeats it for the
-- client to announce.
function keypad.screen(s: State): Screen
  local display
  if s.err then
    display = "Error"
  elseif s.fresh then
    display = calc.format(s.value)
  else
    display = calc.formatEntry(s.entry)
  end
  return {
    state = s,
    display = display,
    caption = s.caption,
    clear = if s.entry ~= "0" and not s.fresh then "C" else "AC",
    pending = if s.waiting then s.ops[#s.ops] else nil,
    error = if s.err then s.caption else nil,
  }
end

-- Press a key. A calculation that fails (calc.luau raises a plain sentence) leaves the display at "Error" with the reason
-- under it, and the screen carries the reason for the client to announce. "AC" gets out of it, and so does a digit,
-- which starts a new sum as it does on an iPad; the other keys have nothing to work on and are ignored.
function keypad.press(given: State, key: string): Screen
  local s = table.clone(given)
  if s.err then
    if string.find(key, "^[%d.]$") then
      clear(s)
    elseif key ~= "clear" then
      return keypad.screen(s)
    end
  end
  -- `pcall` is typed for a function that returns a value: say so, to get the error as its second result
  local ok, err = pcall(apply :: (State, string) -> any, s, key)
  if not ok then
    local reason = calc.reason(err) -- an unexpected error is a generic sentence, never a source position
    local failed = keypad.initial()
    failed.err, failed.caption = true, reason
    return keypad.screen(failed)
  end
  return keypad.screen(s)
end

return keypad
