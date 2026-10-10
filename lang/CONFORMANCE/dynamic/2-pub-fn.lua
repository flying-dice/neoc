local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
function add(a, b)
  __check_number(a, "add.a")
  __check_number(b, "add.b")
  return (a + b)
end
local function main()
  println(add(2, 3))
  println(add(10, 20))
end
