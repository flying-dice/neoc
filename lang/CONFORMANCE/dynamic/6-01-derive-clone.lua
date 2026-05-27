local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Counter
function __check_Counter(v, path)
  if type(v) ~= "table" or v.__struct ~= "Counter" then __type_error(path, "Counter", v) end
  __check_number(v.value, path .. ".value")
  return v
end
local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return __check_Counter(setmetatable(fields, Counter), "Counter")
end
function Counter.clone(self)
  return setmetatable({ __struct = "Counter", value = self.value }, Counter)
end
Counter.__traits = { "Clone" }
local function main()
  local a = Counter.new({ value = 5 })
  local b = a:clone()
  println(a.value)
  println(b.value)
end
