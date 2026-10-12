local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Pair
function __check_Pair(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pair" then __type_error(path, "Pair", v) end
  __check_number(v.a, path .. ".a")
  __check_number(v.b, path .. ".b")
  return v
end
local Pair = {}
Pair.__index = Pair
function Pair.new(fields)
  fields.__struct = "Pair"
  return __check_Pair(setmetatable(fields, Pair), "Pair")
end
local function mark(s)
  println(s)
  return 1
end
local function add(a, b)
  return (a + b)
end
local function call_args(stop)
  local __op_1 = mark("left")
  local __exit_1, __val_1 = (function()
    if stop then
      return 1, "stopped"
    end
    return false, mark("right")
  end)()
  if __exit_1 then
    return __val_1
  end
  local n = add(__op_1, __val_1)
  return (function()
    if (n == 2) then
      return "sum"
    else
      return "?"
    end
  end)()
end
local function fields(stop)
  local __op_2 = mark("a")
  local __exit_2, __val_2 = (function()
    if stop then
      return 1, "stopped"
    end
    return false, mark("b")
  end)()
  if __exit_2 then
    return __val_2
  end
  local p = Pair.new({ a = __op_2, b = __val_2 })
  return (function()
    if ((p.a + p.b) == 2) then
      return "pair"
    else
      return "?"
    end
  end)()
end
local function lazy_and(a)
  local __op_3 = a
  if __op_3 then
    local __exit_3, __val_3 = (function()
      if a then
        return 1, "rhs ran"
      end
      return false, true
    end)()
    if __exit_3 then
      return __val_3
    end
    __op_3 = __val_3
  end
  local ok = __op_3
  return (function()
    if ok then
      return "true"
    else
      return "false"
    end
  end)()
end
local function lazy_or(a)
  local __op_4 = a
  if not __op_4 then
    local __exit_4, __val_4 = (function()
      if (not a) then
        return 1, "rhs ran"
      end
      return false, true
    end)()
    if __exit_4 then
      return __val_4
    end
    __op_4 = __val_4
  end
  local ok = __op_4
  return (function()
    if ok then
      return "true"
    else
      return "false"
    end
  end)()
end
local function main()
  println(call_args(false))
  println(call_args(true))
  println(fields(false))
  println(fields(true))
  println(lazy_and(false))
  println(lazy_and(true))
  println(lazy_or(true))
  println(lazy_or(false))
end
