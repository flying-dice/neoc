local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Cell
function __check_Cell(v, path)
  if type(v) ~= "table" or v.__struct ~= "Cell" then __type_error(path, "Cell", v) end
  __check_number(v.value, path .. ".value")
  return v
end
local Cell = {}
Cell.__index = Cell
function Cell.new(fields)
  fields.__struct = "Cell"
  return __check_Cell(setmetatable(fields, Cell), "Cell")
end
local function main()
  local c = Cell.new({ value = 1 })
  local alias = c
  c.value = 5
  println(alias.value)
  alias.value = (c.value + 2)
  println(c.value)
end
