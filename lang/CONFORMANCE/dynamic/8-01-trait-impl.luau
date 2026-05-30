type Dog = { __struct: "Dog", name: string }

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_Dog
function __check_Dog(v, path)
  if type(v) ~= "table" or v.__struct ~= "Dog" then __type_error(path, "Dog", v) end
  __check_string(v.name, path .. ".name")
  return v
end
local Dog = {}
Dog.__index = Dog
function Dog.new(fields)
  fields.__struct = "Dog"
  return __check_Dog(setmetatable(fields, Dog), "Dog")
end
Dog.__traits = { "Greet" }
function Dog.greet(self): string
  return (self.name .. " says woof")
end
local function main()
  local d = Dog.new({ name = "Rex" })
  println(d:greet())
end
