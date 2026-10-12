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

local function in_block(limit)
  local s = ""
  local __exit_1, __val_1 = (function()
    for i = 0, (5) - 1 do
      s = (s .. "x")
      if (i == limit) then
        return 1, "early"
      end
    end
    return false, "done"
  end)()
  if __exit_1 then
    return __val_1
  end
  local tag = __val_1
  return ((s .. "|") .. tag)
end
local function in_if(early)
  local __exit_2, __val_2 = (function()
    if early then
      return 1, "early"
    else
      return false, "done"
    end
  end)()
  if __exit_2 then
    return __val_2
  end
  local tag = __val_2
  return ("|" .. tag)
end
local function in_match(r)
  local __exit_3, __val_3 = (function()
    local __s = r
    if __s.__tag == "Ok" then
      local v = __s[1]
      return false, v
    end
    if __s.__tag == "Err" then
      local e = __s[1]
      return 1, Err(e)
    end
  end)()
  if __exit_3 then
    return __val_3
  end
  local v = __val_3
  return Ok(("|" .. v))
end
local function nested(n)
  local __exit_5, __val_5 = (function()
    if (n > 0) then
      local __exit_4, __val_4 = (function()
        if (n > 5) then
          return 1, "big"
        end
        return false, "small"
      end)()
      if __exit_4 then
        return 1, __val_4
      end
      return false, __val_4
    else
      return false, "none"
    end
  end)()
  if __exit_5 then
    return __val_5
  end
  local r = __val_5
  return ("|" .. r)
end
local function closure_return(n)
  local f = function(y)
    if (y > 0) then
      return "pos"
    end
    return "other"
  end
  return ((f(n) .. "|") .. "after")
end
local function show(r)
  return (function()
    local __s = r
    if __s.__tag == "Ok" then
      local v = __s[1]
      return v
    end
    if __s.__tag == "Err" then
      local e = __s[1]
      return e
    end
  end)()
end
local function main()
  println(in_block(1))
  println(in_block(9))
  println(in_if(true))
  println(in_if(false))
  println(show(in_match(Err("bad"))))
  println(show(in_match(Ok("good"))))
  println(nested(9))
  println(nested(2))
  println(nested(0))
  println(closure_return(1))
  println(closure_return(0))
end
