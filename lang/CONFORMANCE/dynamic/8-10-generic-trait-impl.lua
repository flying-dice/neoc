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
local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return __check_Counter(setmetatable(fields, Counter), "Counter")
end
Counter.__traits = { "Source" }
function Counter.next(self)
  return self.n
end
function Counter.feed(self, item)
  return (self.n + item)
end
local function pull(source)
  return (source:next() + source:feed(10))
end
local function main()
  local c = Counter.new({ n = 4 })
  local a = c:next()
  println(a)
  println(c:feed(1))
  println(pull(c))
end
