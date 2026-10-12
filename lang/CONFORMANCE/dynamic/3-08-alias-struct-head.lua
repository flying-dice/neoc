local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_P, __check_W
function __check_P(v, path)
  if type(v) ~= "table" or v.__struct ~= "P" then __type_error(path, "P", v) end
  __check_number(v.x, path .. ".x")
  return v
end
function __check_W(v, path)
  if type(v) ~= "table" or v.__struct ~= "W" then __type_error(path, "W", v) end
  return v
end
local P = {}
P.__index = P
function P.new(fields)
  fields.__struct = "P"
  return __check_P(setmetatable(fields, P), "P")
end
P.__traits = { "Show" }
function P.mk()
  return P.new({ x = 7 })
end
function P.make(x)
  return P.new({ x = x })
end
function P.doubled(self)
  return (self.x * 2)
end
function P.show(self)
  return (self.x + 1)
end
local W = {}
W.__index = W
function W.new(fields)
  fields.__struct = "W"
  return __check_W(setmetatable(fields, W), "W")
end
function W.three()
  return W.new({ v = 3 })
end
local function pick(c)
  return (function()
    local __s = c
    if __s.__tag == "Red" then
      return 0
    end
    if __s.__tag == "Green" then
      local n = __s[1]
      return n
    end
  end)()
end
local function unwrap(a)
  local __d = a
  local x = __d.x
  return x
end
local function main()
  local a = P.new({ x = 4 })
  local b = P.new({ x = 5 })
  println(a.x)
  println(b:doubled())
  println(P.mk().x)
  println(P.make(6):doubled())
  println(a:show())
  println(W.three().v)
  println(W.new({ v = 8 }).v)
  println(pick({ __enum = "Color", __tag = "Green", 9 }))
  println(pick({ __enum = "Color", __tag = "Red" }))
  println(unwrap(P.new({ x = 2 })))
end
