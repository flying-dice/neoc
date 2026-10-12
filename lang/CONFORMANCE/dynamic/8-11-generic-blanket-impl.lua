local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_A
function __check_A(v, path)
  if type(v) ~= "table" or v.__struct ~= "A" then __type_error(path, "A", v) end
  __check_string(v.n, path .. ".n")
  return v
end
local A = {}
A.__index = A
function A.new(fields)
  fields.__struct = "A"
  return __check_A(setmetatable(fields, A), "A")
end
A.__traits = { "Named", "Source" }
function A.name(self)
  return self.n
end
local function pull(source)
  return (source:next() + 1)
end
local function main()
  local a = A.new({ n = "a" })
  println(a:next())
  println(pull(a))
end
function A.next(self)
  return (function()
    if (self:name() == "a") then
      return 7
    else
      return 0
    end
  end)()
end
