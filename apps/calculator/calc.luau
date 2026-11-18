--!strict
-- A safe arithmetic evaluator (a small shunting-yard parser: nothing is ever compiled from the text, and the
-- sandbox has no `loadstring`), `format` for results and `formatEntry` for the number being typed.
local calc = {}

type Token = { kind: string, value: string, number: number }

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
        j += 1
      end
      local text = string.sub(source, i, j - 1)
      local value = tonumber(text)
      if value == nil then
        error(`Invalid number "{text}"`)
      end
      table.insert(tokens, { kind = "num", value = text, number = value })
      i = j
    elseif string.find("+-*/%^", ch, 1, true) then
      local previous = tokens[#tokens]
      local unary = ch == "-" and (previous == nil or previous.kind == "op" or previous.value == "(")
      table.insert(tokens, { kind = "op", value = if unary then "neg" else ch, number = 0 })
      i += 1
    elseif ch == "(" or ch == ")" then
      local paren: string = ch
      table.insert(tokens, { kind = "paren", value = paren, number = 0 })
      i += 1
    else
      error(`Unexpected character "{ch}"`)
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
      error("Division by zero")
    end
    return a / b
  elseif op == "%" then
    return a % b
  elseif op == "^" then
    return a ^ b
  end
  error(`Unknown operator "{op}"`)
end

-- Round away binary noise (0.1 + 0.2): 12 significant digits, as a number.
local function tidy(value: number): number
  return tonumber(string.format("%.12g", value)) :: number
end

local function apply(op: string, stack: { number })
  if op == "neg" then
    local a = table.remove(stack)
    if a == nil then
      error("Malformed expression")
    end
    table.insert(stack, -a)
    return
  end
  local b = table.remove(stack)
  local a = table.remove(stack)
  if a == nil or b == nil then
    error("Malformed expression")
  end
  table.insert(stack, operate(op, a, b))
end

-- `a op b` on numbers, the way the keypad chains: no text round trip, so a result shown as 2e-7 or 1e21 is an
-- ordinary operand. Raises on division by zero and on a result too large to show.
function calc.combine(a: number, op: string, b: number): number
  local result = operate(op, a, b)
  if result ~= result or math.abs(result) == math.huge then
    error("Result out of range")
  end
  return tidy(result)
end

-- Evaluate an arithmetic expression; raises a descriptive error when invalid.
function calc.evaluate(expression: string): number
  local tokens = tokenize(expression)
  if #tokens == 0 then
    error("Empty expression")
  end
  local values: { number } = {}
  local ops: { string } = {}
  for _, token in tokens do
    if token.kind == "num" then
      table.insert(values, token.number)
    elseif token.value == "(" then
      table.insert(ops, "(")
    elseif token.kind == "paren" then
      while #ops > 0 and ops[#ops] ~= "(" do
        apply(table.remove(ops) :: string, values)
      end
      if table.remove(ops) ~= "(" then
        error("Mismatched parentheses")
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
          apply(table.remove(ops) :: string, values)
        else
          break
        end
      end
      table.insert(ops, token.value)
    end
  end
  while #ops > 0 do
    local op = table.remove(ops) :: string
    if op == "(" then
      error("Mismatched parentheses")
    end
    apply(op, values)
  end
  local result = values[1]
  if #values ~= 1 or result ~= result or math.abs(result) == math.huge then
    error("Malformed expression")
  end
  return tidy(result)
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
  local text = string.format(`%.{decimals}f`, value)
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
    local trimmed = string.gsub(mantissa :: string, "%.?0+$", "")
    -- "%e" pads the exponent to two digits ("e-07") and signs it ("e+12"): show "e-7" and "e12"
    local sign, digits = string.match(exponent :: string, "^([+-])(%d+)$")
    local power = `{if sign == "-" then "-" else ""}{tonumber(digits :: string) :: number}`
    return `{trimmed}e{power}`
  end
  local text = fixed(value)
  local sign, integer, fraction = string.match(text, "^(-?)(%d+)%.?(%d*)$")
  local grouped = group(integer :: string)
  return if fraction ~= "" then `{sign}{grouped}.{fraction}` else `{sign}{grouped}`
end

-- Format the number being typed: group the integer part, keep a trailing dot and trailing zeros.
function calc.formatEntry(entry: string): string
  local sign, integer, rest = string.match(entry, "^(-?)(%d*)(.*)$")
  local grouped = group(if integer == "" then "0" else integer :: string)
  return `{sign}{grouped}{rest}`
end

return calc
