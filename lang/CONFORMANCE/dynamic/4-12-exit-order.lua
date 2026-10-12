local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Pair, __check_Pt
function __check_Pair(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pair" then __type_error(path, "Pair", v) end
  __check_number(v.a, path .. ".a")
  __check_number(v.b, path .. ".b")
  return v
end
function __check_Pt(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pt" then __type_error(path, "Pt", v) end
  __check_number(v.x, path .. ".x")
  __check_number(v.y, path .. ".y")
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
local Pt = {}
Pt.__index = Pt
function Pt.new(fields)
  fields.__struct = "Pt"
  return __check_Pt(setmetatable(fields, Pt), "Pt")
end
local function shorthand(stop)
  local x = 1
  local __op_5 = x
  local __exit_5, __val_5 = (function()
    if stop then
      return 1, "stopped"
    end
    x = x + 10
    return false, 2
  end)()
  if __exit_5 then
    return __val_5
  end
  local p = Pt.new({ x = __op_5, y = __val_5 })
  return (function()
    if (p.x == 1) then
      return "bound"
    else
      return "shifted"
    end
  end)()
end
local function range_bounds(stop)
  local s = ""
  do
    local __sb_1 = { (s .. "") }
    local __op_6 = mark("lo")
    local __exit_6, __val_6 = (function()
      if stop then
        return 1, "stopped"
      end
      return false, (mark("hi") + 2)
    end)()
    if __exit_6 then
      return __val_6
    end
    for i = __op_6, (__val_6) - 1 do
      __sb_1[#__sb_1 + 1] = "r"
    end
    s = table.concat(__sb_1)
  end
  return s
end
local function update(stop)
  local p = Pt.new({ x = 1, y = 2 })
  local __op_7 = mark("y")
  local __exit_7, __val_7 = (function()
    if stop then
      return 1, "stopped"
    end
    return false, p
  end)()
  if __exit_7 then
    return __val_7
  end
  local q = Pt.new((function()
    local __f0 = __op_7
    local __b = {}
    for k, v in pairs(__val_7) do __b[k] = v end
    __b.y = __f0
    return __b
  end)())
  return (function()
    if (q.x == 1) then
      return "kept"
    else
      return "?"
    end
  end)()
end
local function make()
  println("base")
  return Pt.new({ x = 0, y = 0 })
end
local function update_order()
  local q = Pt.new((function()
    local __f0 = mark("x")
    local __b = {}
    for k, v in pairs(make()) do __b[k] = v end
    __b.x = __f0
    return __b
  end)())
  return (function()
    if (q.x == 1) then
      return "set"
    else
      return "?"
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
  println(shorthand(false))
  println(shorthand(true))
  println(range_bounds(false))
  println(range_bounds(true))
  println(update(false))
  println(update(true))
  println(update_order())
end
