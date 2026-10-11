local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local __check_Table
function __check_Table(v, path)
  if type(v) ~= "table" or v.__struct ~= "Table" then __type_error(path, "Table", v) end
  return v
end
local Table = {}
Table.__index = Table
function Table.new(fields)
  fields.__struct = "Table"
  return __check_Table(setmetatable(fields, Table), "Table")
end
local function first(items)
  return items[(0) + 1]
end
local function main()
  local names = { "ada", "grace" }
  local grid = { { 1, 2 }, { 3, 4 } }
  local table = Table.new({ rows = grid })
  println(first(first(table.rows)))
  println(first(table.rows[(1) + 1]))
end
