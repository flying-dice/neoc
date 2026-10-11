local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_User
function __check_User(v, path)
  if type(v) ~= "table" or v.__struct ~= "User" then __type_error(path, "User", v) end
  __check_string(v.name, path .. ".name")
  return v
end
local User = {}
User.__index = User
function User.new(fields)
  fields.__struct = "User"
  return __check_User(setmetatable(fields, User), "User")
end
local function main()
  local ada = User.new({ name = "ada" })
  println(ada.name)
  local bad = User.new({ name = tonumber("7") })
  println(bad.name)
end
