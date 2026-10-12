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

local function parse(n)
  return (function()
    if (n < 0) then
      return Err("neg")
    else
      return Ok(n)
    end
  end)()
end
local function in_block(a, b)
  local __exit_1, __val_1 = (function()
    local __try_0 = parse(a)
    if __try_0.__tag == "Err" or __try_0.__tag == "None" then return 1, __try_0 end
    local x = __try_0[1]
    local __try_1 = parse(b)
    if __try_1.__tag == "Err" or __try_1.__tag == "None" then return 1, __try_1 end
    local y = __try_1[1]
    return false, (x + y)
  end)()
  if __exit_1 then
    return __val_1
  end
  local total = __val_1
  return Ok("sum")
end
local function in_match(a)
  local __exit_3, __val_3 = (function()
    local __s = a
    if __s == 0 then
      return false, "zero"
    end
    if true then
      local __exit_2, __val_2 = (function()
        local __try_2 = parse(a)
        if __try_2.__tag == "Err" or __try_2.__tag == "None" then return 1, __try_2 end
        local v = __try_2[1]
        return false, (function()
          if (v > 100) then
            return "big"
          else
            return "ok"
          end
        end)()
      end)()
      if __exit_2 then
        return 1, __val_2
      end
      return false, __val_2
    end
  end)()
  if __exit_3 then
    return __val_3
  end
  local r = __val_3
  return Ok(r)
end
local function in_closure(a)
  local f = function(y)
    local __try_3 = parse(y)
    if __try_3.__tag == "Err" or __try_3.__tag == "None" then return __try_3 end
    local z = __try_3[1]
    return Ok("closure")
  end
  local __try_4 = f(a)
  if __try_4.__tag == "Err" or __try_4.__tag == "None" then return __try_4 end
  local r = __try_4[1]
  return Ok((r .. "|after"))
end
local function in_cond(limit)
  local i = 0
  while true do
    local __try_5 = parse((limit - i))
    if __try_5.__tag == "Err" or __try_5.__tag == "None" then return __try_5 end
    if not ((__try_5[1] > 0)) then break end
    i = i + 2
  end
  return Ok("done")
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
  println(show(in_block(1, 2)))
  println(show(in_block(1, (-2))))
  println(show(in_block((-1), 2)))
  println(show(in_match(0)))
  println(show(in_match(5)))
  println(show(in_match(500)))
  println(show(in_match((-5))))
  println(show(in_closure(1)))
  println(show(in_closure((-1))))
  println(show(in_cond(3)))
  println(show(in_cond(4)))
end
