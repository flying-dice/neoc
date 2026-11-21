-- A safe arithmetic evaluator (a small shunting-yard parser: nothing is ever compiled from the text, and the
-- sandbox has no `loadstring`), `format` for results and `formatEntry` for the number being typed.
local calc = {}

-- A failure calc raises on purpose is a plain sentence for a person ("Can't divide by zero") and for an agent, with
-- no "calc.lua:56:" in front: an error object whose `tostring` is that sentence. Anything else that goes wrong
-- (a bug) is an ordinary error with a source position, which `calc.reason` keeps from reaching the screen.
local Failure = {}
Failure.__tostring = function(failure)
  return failure.message
end

local function fail(message)
  error(setmetatable({ message = message }, Failure))
end

-- The sentence to show for something `pcall` caught: a failure raised by `fail`, else a generic one.
function calc.reason(err)
  if getmetatable(err) == Failure then
    return err.message
  end
  return "Something went wrong"
end

-- A token is { kind = "num" | "op" | "paren", value = text, number = its value (numbers only) }.

local PRECEDENCE = { ["+"] = 1, ["-"] = 1, ["*"] = 2, ["/"] = 2, ["%"] = 2, ["^"] = 3, neg = 4 }
local RIGHT_ASSOCIATIVE = { ["^"] = true, neg = true }

local function tokenize(input)
  local source = string.gsub(input, "×", "*")
  source = string.gsub(source, "÷", "/")
  source = string.gsub(source, "−", "-")
  source = string.gsub(source, "%s+", "")
  local tokens = {}
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
        fail('Invalid number "' .. text .. '"')
      end
      table.insert(tokens, { kind = "num", value = text, number = value })
      i = j
    elseif string.find("+-*/%^", ch, 1, true) then
      local previous = tokens[#tokens]
      local unary = ch == "-" and (previous == nil or previous.kind == "op" or previous.value == "(")
      table.insert(tokens, { kind = "op", value = unary and "neg" or ch, number = 0 })
      i = i + 1
    elseif ch == "(" or ch == ")" then
      table.insert(tokens, { kind = "paren", value = ch, number = 0 })
      i = i + 1
    else
      fail('Unexpected character "' .. ch .. '"')
    end
  end
  return tokens
end

-- One binary operation on numbers. Raises on division by zero.
local function operate(op, a, b)
  if op == "+" then
    return a + b
  elseif op == "-" then
    return a - b
  elseif op == "*" then
    return a * b
  elseif op == "/" then
    if b == 0 then
      fail("Can't divide by zero")
    end
    return a / b
  elseif op == "%" then
    if b == 0 then
      fail("Can't divide by zero") -- Lua's `a % 0` is nan
    end
    return a % b
  elseif op == "^" then
    return a ^ b
  end
  fail('Unknown operator "' .. op .. '"')
end

-- Round away binary noise (0.1 + 0.2): 12 significant digits, as a number.
local function tidy(value)
  return tonumber(string.format("%.12g", value))
end

local function apply(op, stack)
  if op == "neg" then
    local a = table.remove(stack)
    if a == nil then
      fail("Malformed expression")
    end
    table.insert(stack, -a)
    return
  end
  local b = table.remove(stack)
  local a = table.remove(stack)
  if a == nil or b == nil then
    fail("Malformed expression")
  end
  table.insert(stack, operate(op, a, b))
end

-- A result the display cannot show: infinity (too big) or nan (not a real number, such as (-8) ^ 0.5).
local function checked(result)
  if result ~= result then
    fail("Not a real number")
  elseif math.abs(result) == math.huge then
    fail("Result too large")
  end
  return tidy(result)
end

-- `a op b` on numbers, the way the keypad chains: no text round trip, so a result shown as 2e-7 or 1e21 is an
-- ordinary operand. Raises on division by zero and on a result too large to show.
function calc.combine(a, op, b)
  return checked(operate(op, a, b))
end

-- Evaluate an arithmetic expression; raises a descriptive error when invalid.
function calc.evaluate(expression)
  local tokens = tokenize(expression)
  if #tokens == 0 then
    fail("Empty expression")
  end
  local values = {}
  local ops = {}
  for _, token in ipairs(tokens) do
    if token.kind == "num" then
      table.insert(values, token.number)
    elseif token.value == "(" then
      table.insert(ops, "(")
    elseif token.kind == "paren" then
      while #ops > 0 and ops[#ops] ~= "(" do
        apply(table.remove(ops), values)
      end
      if table.remove(ops) ~= "(" then
        fail("Mismatched parentheses")
      end
    else
      local precedence = PRECEDENCE[token.value]
      while #ops > 0 do
        local top = ops[#ops]
        if top == "(" then
          break
        end
        local other = PRECEDENCE[top]
        if other > precedence or (other == precedence and not RIGHT_ASSOCIATIVE[token.value]) then
          apply(table.remove(ops), values)
        else
          break
        end
      end
      table.insert(ops, token.value)
    end
  end
  while #ops > 0 do
    local op = table.remove(ops)
    if op == "(" then
      fail("Mismatched parentheses")
    end
    apply(op, values)
  end
  if #values ~= 1 then
    fail("Malformed expression")
  end
  return checked(values[1])
end

local function group(digits)
  local spaced = string.gsub(string.reverse(digits), "(%d%d%d)", "%1,")
  return (string.gsub(string.reverse(spaced), "^,", ""))
end

-- Fixed notation with at most 9 significant digits and no trailing zeros. Not `%.9g`: that switches to
-- exponent form ("5e-05", "1e+09") between 1e-6 and 1e-4 and when rounding reaches 1e9, which has no integer
-- part to group.
local function fixed(value)
  local decimals = math.max(0, 8 - math.floor(math.log10(math.abs(value))))
  local text = string.format("%." .. decimals .. "f", value)
  if string.find(text, ".", 1, true) then
    text = string.gsub(text, "0+$", "")
    text = string.gsub(text, "%.$", "")
  end
  return text
end

-- Format a result for the display: grouping, at most 9 significant digits.
function calc.format(value)
  if value ~= value or math.abs(value) == math.huge then
    return "Error"
  end
  if value == 0 then
    return "0"
  end
  -- 999999999.6 is under 1e9 but shows as 1e9 once rounded to 9 digits
  local rounded = tonumber(string.format("%.9g", value))
  ---@cast rounded number
  if math.abs(rounded) >= 1e9 or math.abs(value) < 1e-6 then
    local mantissa, exponent = string.match(string.format("%.8e", value), "^(.-)e([+-]%d+)$")
    local trimmed = string.gsub(mantissa, "%.?0+$", "")
    -- "%e" pads the exponent to two digits ("e-07") and signs it ("e+12"): show "e-7" and "e12"
    local sign, digits = string.match(exponent, "^([+-])(%d+)$")
    local power = (sign == "-" and "-" or "") .. tonumber(digits)
    return trimmed .. "e" .. power
  end
  local text = fixed(value)
  local sign, integer, fraction = string.match(text, "^(-?)(%d+)%.?(%d*)$")
  local grouped = group(integer)
  if fraction ~= "" then
    return sign .. grouped .. "." .. fraction
  end
  return sign .. grouped
end

-- Format the number being typed: group the integer part, keep a trailing dot and trailing zeros.
function calc.formatEntry(entry)
  local sign, integer, rest = string.match(entry, "^(-?)(%d*)(.*)$")
  local grouped = group(integer == "" and "0" or integer)
  return sign .. grouped .. rest
end

return calc
