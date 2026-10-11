local Option = {}
Option.__index = Option
local Result = {}
Result.__index = Result
local Ok   = function(v) return setmetatable({ __enum = "Result", __tag = "Ok", v }, Result) end
local Err  = function(e) return setmetatable({ __enum = "Result", __tag = "Err", e }, Result) end
local Some = function(v) return setmetatable({ __enum = "Option", __tag = "Some", v }, Option) end
local None = setmetatable({ __enum = "Option", __tag = "None" }, Option)
function Option.is_some(self) return self.__tag == "Some" end
function Option.is_none(self) return self.__tag == "None" end
function Option.unwrap(self)
  if self.__tag == "Some" then return self[1] end
  error("called unwrap on a None value", 0)
end
function Option.unwrap_or(self, default)
  if self.__tag == "Some" then return self[1] else return default end
end
function Option.map(self, f)
  if self.__tag == "Some" then return Some(f(self[1])) else return self end
end
function Option.ok_or(self, err)
  if self.__tag == "Some" then return Ok(self[1]) else return Err(err) end
end
function Result.is_ok(self) return self.__tag == "Ok" end
function Result.is_err(self) return self.__tag == "Err" end
function Result.unwrap(self)
  if self.__tag == "Ok" then return self[1] end
  error("called unwrap on an Err value", 0)
end
function Result.unwrap_err(self)
  if self.__tag == "Err" then return self[1] end
  error("called unwrap_err on an Ok value", 0)
end
function Result.map(self, f)
  if self.__tag == "Ok" then return Ok(f(self[1])) else return self end
end
function Result.map_err(self, f)
  if self.__tag == "Err" then return Err(f(self[1])) else return self end
end

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local function __check_list(v, path, elem)
  if type(v) ~= "table" or v.__struct or v.__enum then __type_error(path, "list", v) end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  for i = 1, n do
    if v[i] == nil then __type_error(path, "list", v) end
    elem(v[i], path .. "[" .. (i - 1) .. "]")
  end
  return v
end
local function __check_option(v, path, some)
  if type(v) ~= "table" or v.__enum ~= "Option" then __type_error(path, "Option", v) end
  if v.__tag == "Some" then some(v[1], path .. ".0")
  elseif v.__tag ~= "None" then __type_error(path, "Option", v) end
  return v
end
local __check_Address, __check_User
function __check_Address(v, path)
  if type(v) ~= "table" or v.__struct ~= "Address" then __type_error(path, "Address", v) end
  __check_string(v.zip, path .. ".zip")
  __check_option(v.resident, path .. ".resident", __check_User)
  return v
end
function __check_User(v, path)
  if type(v) ~= "table" or v.__struct ~= "User" then __type_error(path, "User", v) end
  __check_string(v.name, path .. ".name")
  __check_list(v.addresses, path .. ".addresses", __check_Address)
  return v
end
local User = {}
User.__index = User
function User.new(fields)
  fields.__struct = "User"
  return __check_User(setmetatable(fields, User), "User")
end
local Address = {}
Address.__index = Address
function Address.new(fields)
  fields.__struct = "Address"
  return __check_Address(setmetatable(fields, Address), "Address")
end
local function main()
  local home = Address.new({ zip = "10001", resident = None })
  local ada = User.new({ name = "ada", addresses = { home } })
  local work = Address.new({ zip = "94105", resident = Some(ada) })
  local grace = User.new({ name = "grace", addresses = { work } })
  do
    local __s = grace.addresses[(0) + 1].resident
    if __s.__tag == "Some" then
      local u = __s[1]
      println(u.name)
    end
    if __s.__tag == "None" then
      println("none")
    end
  end
  println(grace.addresses[(0) + 1].zip)
end
