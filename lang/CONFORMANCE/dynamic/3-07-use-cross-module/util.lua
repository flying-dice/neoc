local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Counter
function __check_Counter(v, path)
  if type(v) ~= "table" or v.__struct ~= "Counter" then __type_error(path, "Counter", v) end
  __check_number(v.n, path .. ".n")
  return v
end
local function add(a, b)
  __check_number(a, "add.a")
  __check_number(b, "add.b")
  return (a + b)
end
local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return __check_Counter(setmetatable(fields, Counter), "Counter")
end
function Counter.bump(self)
  return (self.n + 1)
end
return { add = add, Counter = Counter }
