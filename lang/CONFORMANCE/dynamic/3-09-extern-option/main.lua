local collections = require("std:collections")

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

local function __option_to_host(v)
  if v.__tag == "Some" then return v[1] end
  return nil
end

local function __option_from_host(v)
  if v == nil then return None end
  return Some(v)
end

local function main()
  local seeded = collections.Counter.new(__option_to_host(Some(5)))
  local bare = collections.Counter.new(__option_to_host(None))
  println(seeded:get())
  println(bare:get())
  local m = collections.Map.new()
  m:set("a", "x")
  println(__option_from_host(m:get("a")):unwrap_or("none"))
  println(__option_from_host(m:get("z")):unwrap_or("none"))
  println(__option_from_host(m:get("z")):is_none())
  local flags = collections.Map.new()
  flags:set("off", false)
  println(__option_from_host(flags:get("off")):is_some())
  println(__option_from_host(flags:get("off")):unwrap_or(true))
end
return { main = main }
