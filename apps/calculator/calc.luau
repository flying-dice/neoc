--!strict
-- A safe arithmetic evaluator (a small shunting-yard parser: nothing is ever compiled from the text, and the
-- sandbox has no `loadstring`), `format` for results and `formatEntry` for the number being typed. The `calculate`
-- tool and the keypad both run it, so `2 + 3 * 4` is 14 for an agent and for a person.
local calc = {}

-- A failure calc raises on purpose is a plain sentence for a person ("Can't divide by zero") and for an agent, with
-- no "calc.lua:56:" in front: an error object whose `tostring` is that sentence. Anything else that goes wrong
-- (a bug) is an ordinary error with a source position, which `calc.reason` keeps from reaching the screen.
type Failure = { message: string }

local Failure = {}
Failure.__tostring = function(failure: Failure): string
  return failure.message
end

local function fail(message: string): never
  error(setmetatable({ message = message }, Failure))
end

-- The sentence to show for something `pcall` caught: a failure raised by `fail`, else a generic one.
function calc.reason(err: unknown): string
  if getmetatable(err :: any) == Failure then
    return (err :: Failure).message
  end
  return "Something went wrong"
end

-- A token is { kind = "num" | "op" | "paren", value = text, number = its value (numbers only) }.
type Token = { kind: "num" | "op" | "paren", value: string, number: number }

local PRECEDENCE: { [string]: number } = { ["+"] = 1, ["-"] = 1, ["*"] = 2, ["/"] = 2, ["%"] = 2, ["^"] = 3, neg = 4 }
local RIGHT_ASSOCIATIVE: { [string]: boolean } = { ["^"] = true, neg = true }

local function tokenize(input: string): { Token }
  local source = string.gsub(input, "×", "*")
  source = string.gsub(source, "÷", "/")
  source = string.gsub(source, "−", "-")
  source = string.gsub(source, "%s+", "")
  local tokens: { Token } = {}
  local i = 1
  while i <= #source do
    local ch = string.sub(source, i, i)
    if string.find(ch, "[%d.]") then
      local j = i
      while j <= #source and string.find(string.sub(source, j, j), "[%d.]") do
        j = j + 1
      end
      local text = string.sub(source, i, j - 1)
      local value = tonumber(text)
      if value == nil then
        return fail('Invalid number "' .. text .. '"')
      end
      table.insert(tokens, { kind = "num", value = text, number = value })
      i = j
    elseif string.find("+-*/%^", ch, 1, true) then
      local previous = tokens[#tokens]
      local unary = ch == "-" and (previous == nil or previous.kind == "op" or previous.value == "(")
      table.insert(tokens, { kind = "op", value = unary and "neg" or ch, number = 0 })
      i = i + 1
    elseif ch == "(" or ch == ")" then
      local paren: string = ch
      table.insert(tokens, { kind = "paren", value = paren, number = 0 })
      i = i + 1
    else
      return fail('Unexpected character "' .. ch .. '"')
    end
  end
  return tokens
end

-- One binary operation on numbers. Raises on division by zero.
local function operate(op: string, a: number, b: number): number
  if op == "+" then
    return a + b
  elseif op == "-" then
    return a - b
  elseif op == "*" then
    return a * b
  elseif op == "/" then
    if b == 0 then
      return fail("Can't divide by zero")
    end
    return a / b
  elseif op == "%" then
    if b == 0 then
      return fail("Can't divide by zero") -- Lua's `a % 0` is nan
    end
    return a % b
  elseif op == "^" then
    return a ^ b
  end
  return fail('Unknown operator "' .. op .. '"')
end

-- Round away binary noise (0.1 + 0.2): 12 significant digits, as a number.
local function tidy(value: number): number
  return tonumber(string.format("%.12g", value)) :: number
end

-- Takes the top of the stack; an empty one is a malformed expression (`1 +`, `*`).
local function pop(stack: { number }): number
  return table.remove(stack) or fail("Malformed expression")
end

local function apply(op: string, stack: { number })
  if op == "neg" then
    table.insert(stack, -pop(stack))
    return
  end
  local b = pop(stack)
  local a = pop(stack)
  table.insert(stack, operate(op, a, b))
end

-- A result the display cannot show: infinity (too big) or nan (not a real number, such as (-8) ^ 0.5).
local function checked(result: number): number
  if result ~= result then
    fail("Not a real number")
  elseif math.abs(result) == math.huge then
    fail("Result too large")
  end
  return tidy(result)
end

-- `a op b` on numbers with no text round trip (the keypad's `%` is `value / 100`): a result shown as 2e-7 or 1e21 is
-- an ordinary operand. Raises on division by zero and on a result too large to show.
function calc.combine(a: number, op: string, b: number): number
  return checked(operate(op, a, b))
end

-- Reduce the operators waiting on the stack for as long as `ready(top)` says so.
local function reduceWhile(ops: { string }, values: { number }, ready: (top: string) -> boolean)
  while #ops > 0 and ops[#ops] ~= "(" and ready(ops[#ops]) do
    apply(table.remove(ops) :: string, values)
  end
end

-- The one evaluator: shunting-yard over tokens. With no `trailing` operator it evaluates the whole expression and
-- returns its value. With one (the operator just pressed on the keypad) it reduces only what that operator completes,
-- the waiting operators of equal or higher precedence, and returns the number on top of the stack: `2 + 3 ×` is 3,
-- `2 × 3 +` is 6, `2 + 3 × 4 +` is 14. The result is not yet `checked`.
local function run(tokens: { Token }, trailing: string?): number
  local values: { number } = {}
  local ops: { string } = {}
  for _, token in ipairs(tokens) do
    if token.kind == "num" then
      table.insert(values, token.number)
    elseif token.value == "(" then
      table.insert(ops, "(")
    elseif token.kind == "paren" then
      reduceWhile(ops, values, function()
        return true
      end)
      if table.remove(ops) ~= "(" then
        fail("Mismatched parentheses")
      end
    else
      local precedence = PRECEDENCE[token.value]
      reduceWhile(ops, values, function(top)
        local other = PRECEDENCE[top]
        return other > precedence or (other == precedence and not RIGHT_ASSOCIATIVE[token.value])
      end)
      table.insert(ops, token.value)
    end
  end
  if trailing ~= nil then
    local precedence = PRECEDENCE[trailing]
    reduceWhile(ops, values, function(top)
      return PRECEDENCE[top] >= precedence
    end)
    if #values == 0 then
      return fail("Malformed expression")
    end
    return values[#values]
  end
  while #ops > 0 do
    local op = table.remove(ops) :: string
    if op == "(" then
      fail("Mismatched parentheses")
    end
    apply(op, values)
  end
  if #values ~= 1 then
    fail("Malformed expression")
  end
  return values[1]
end

-- Evaluate an arithmetic expression; raises a descriptive error when invalid.
function calc.evaluate(expression: string): number
  local tokens = tokenize(expression)
  if #tokens == 0 then
    fail("Empty expression")
  end
  return checked(run(tokens))
end

-- The keypad's sum is a flat list of terms: numbers, and the operators `+ - * /` between them (`{ 2, "+", 3, "*" }`).
-- They become the tokens `tokenize` makes, so the keypad runs the evaluator above and not a copy of it.
local function tokensOf(terms: { number | string }): { Token }
  local tokens: { Token } = {}
  for _, term in terms do
    if type(term) == "number" then
      table.insert(tokens, { kind = "num", value = tostring(term), number = term })
    else
      table.insert(tokens, { kind = "op", value = term, number = 0 })
    end
  end
  return tokens
end

-- The value of a finished list of terms (it ends with a number): what `=` shows. Raises like `evaluate`.
function calc.total(terms: { number | string }): number
  return checked(run(tokensOf(terms)))
end

-- The number to show while the terms end with an operator that waits for its operand: the part of the sum that
-- operator completes. Raises on a division by zero in that part as soon as the operator is pressed.
function calc.progress(terms: { number | string }): number
  local tokens = tokensOf(terms)
  local trailing = table.remove(tokens)
  if trailing == nil then
    return fail("Malformed expression")
  end
  return checked(run(tokens, trailing.value))
end

local SYMBOL: { [string]: string } = { ["+"] = "+", ["-"] = "−", ["*"] = "×", ["/"] = "÷" }
local CAPTION_WIDTH = 24 -- characters: the caption is one line, 296 pt wide

-- The terms as the caption writes them (`2 + 3 ×`). A long chain keeps its last terms behind an ellipsis, starting
-- at a number (`… 3 + 4 + 5`, not `… + 4 + 5`).
function calc.describe(terms: { number | string }): string
  local words: { string } = {}
  local first: number | string | nil -- the term of words[1]
  local width = 0
  for i = #terms, 1, -1 do
    local term = terms[i]
    local word = if type(term) == "number" then calc.format(term) else SYMBOL[term]
    width += (if type(term) == "number" then #word else 1) + 1 -- an operator symbol is one character, however many bytes
    if width > CAPTION_WIDTH and #words > 0 then
      if type(first) ~= "number" then
        table.remove(words, 1)
      end
      table.insert(words, 1, "…")
      break
    end
    table.insert(words, 1, word)
    first = term
  end
  return table.concat(words, " ")
end

local function group(digits: string): string
  local spaced = string.gsub(string.reverse(digits), "(%d%d%d)", "%1,")
  return (string.gsub(string.reverse(spaced), "^,", ""))
end

-- Fixed notation with at most 9 significant digits and no trailing zeros. Not `%.9g`: that switches to
-- exponent form ("5e-05", "1e+09") between 1e-6 and 1e-4 and when rounding reaches 1e9, which has no integer
-- part to group.
local function fixed(value: number): string
  local decimals = math.max(0, 8 - math.floor(math.log10(math.abs(value))))
  local text = string.format("%." .. decimals .. "f", value)
  if string.find(text, ".", 1, true) then
    text = string.gsub(text, "0+$", "")
    text = string.gsub(text, "%.$", "")
  end
  return text
end

-- Format a result for the display: grouping, at most 9 significant digits.
function calc.format(value: number): string
  if value ~= value or math.abs(value) == math.huge then
    return "Error"
  end
  if value == 0 then
    return "0"
  end
  -- 999999999.6 is under 1e9 but shows as 1e9 once rounded to 9 digits
  local rounded = tonumber(string.format("%.9g", value)) :: number
  if math.abs(rounded) >= 1e9 or math.abs(value) < 1e-6 then
    local mantissa, exponent = string.match(string.format("%.8e", value), "^(.-)e([+-]%d+)$")
    assert(mantissa ~= nil and exponent ~= nil)
    local trimmed = string.gsub(mantissa, "%.?0+$", "")
    -- "%e" pads the exponent to two digits ("e-07") and signs it ("e+12"): show "e-7" and "e12"
    local sign, digits = string.match(exponent, "^([+-])(%d+)$")
    assert(sign ~= nil and digits ~= nil)
    local power = (sign == "-" and "-" or "") .. (tonumber(digits) :: number)
    return trimmed .. "e" .. power
  end
  local text = fixed(value)
  local sign, integer, fraction = string.match(text, "^(-?)(%d+)%.?(%d*)$")
  assert(sign ~= nil and integer ~= nil and fraction ~= nil)
  local grouped = group(integer)
  if fraction ~= "" then
    return sign .. grouped .. "." .. fraction
  end
  return sign .. grouped
end

-- Format the number being typed: group the integer part, keep a trailing dot and trailing zeros.
function calc.formatEntry(entry: string): string
  local sign, integer, rest = string.match(entry, "^(-?)(%d*)(.*)$")
  assert(sign ~= nil and integer ~= nil and rest ~= nil)
  local grouped = group(integer == "" and "0" or integer)
  return sign .. grouped .. rest
end

return calc
