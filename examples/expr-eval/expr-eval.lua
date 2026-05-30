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

local boolstr, digit, render
local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
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
local __check_Binding, __check_Env, __check_Outcome, __check_VarId
function __check_Binding(v, path)
  if type(v) ~= "table" or v.__struct ~= "Binding" then __type_error(path, "Binding", v) end
  __check_string(v.name, path .. ".name")
  __check_number(v.value, path .. ".value")
  return v
end
function __check_Env(v, path)
  if type(v) ~= "table" or v.__struct ~= "Env" then __type_error(path, "Env", v) end
  __check_list(v.bindings, path .. ".bindings", __check_Binding)
  return v
end
function __check_Outcome(v, path)
  if type(v) ~= "table" or v.__struct ~= "Outcome" then __type_error(path, "Outcome", v) end
  __check_string(v.label, path .. ".label")
  __check_number(v.value, path .. ".value")
  return v
end
function __check_VarId(v, path)
  if type(v) ~= "table" or v.__struct ~= "VarId" then __type_error(path, "VarId", v) end
  __check_string(v[1], path .. ".0")
  return v
end
local VarId = {}
VarId.__index = VarId
function VarId.new(value)
  return __check_VarId(setmetatable({ __struct = "VarId", value }, VarId), "VarId")
end
local EvalError = {}
EvalError.__index = EvalError
function EvalError.display(self)
  if self.__tag == "UnknownVar" then
    return tostring(self[1])
  end
  if self.__tag == "DivByZero" then
    return "DivByZero"
  end
end
EvalError.__traits = { "Display" }
local Binding = {}
Binding.__index = Binding
function Binding.new(fields)
  fields.__struct = "Binding"
  return __check_Binding(setmetatable(fields, Binding), "Binding")
end
function Binding.clone(self)
  return setmetatable({ __struct = "Binding", name = self.name, value = self.value }, Binding)
end
function Binding.debug(self)
  return "Binding { " .. "name = " .. tostring(self.name) .. ", " .. "value = " .. tostring(self.value) .. " }"
end
function Binding.eq(a, b)
  return a.__struct == b.__struct and a.name == b.name and a.value == b.value
end
Binding.__traits = { "Clone", "Debug", "Eq" }
local Env = {}
Env.__index = Env
function Env.new(fields)
  fields.__struct = "Env"
  return __check_Env(setmetatable(fields, Env), "Env")
end
function Env.lookup(self, name)
  for _, b in ipairs(self.bindings) do
    if (b.name == name) then
      return Some(b.value)
    end
  end
  return None
end
local function idiv(a, b)
  local q = 0
  local r = a
  while (r >= b) do
    r = (r - b)
    q = (q + 1)
  end
  return q
end
local function apply(op, a, b)
  return (function()
    local __s = op
    if __s.__tag == "Add" then
      return Ok((a + b))
    end
    if __s.__tag == "Sub" then
      return Ok((a - b))
    end
    if __s.__tag == "Mul" then
      return Ok((a * b))
    end
    if __s.__tag == "Div" then
      return (function()
        return (function()
          local __s = b
          if __s == 0 then
            return Err(setmetatable({ __enum = "EvalError", __tag = "DivByZero" }, EvalError))
          end
          if true then
            return Ok(idiv(a, b))
          end
        end)()
      end)()
    end
  end)()
end
local function evaluate(e, env)
  return (function()
    local __s = e
    if __s.__tag == "Lit" then
      local n = __s[1]
      return Ok(n)
    end
    if __s.__tag == "Var" then
      local id = __s[1]
      return (function()
        local __d = id
        local name = __d[1]
        return env:lookup(name):ok_or(setmetatable({ __enum = "EvalError", __tag = "UnknownVar", name }, EvalError))
      end)()
    end
    if __s.__tag == "Bin" then
      local op = __s.op
      local lhs = __s.lhs
      local rhs = __s.rhs
      return (function()
        local __try_0 = evaluate(lhs, env)
        if __try_0.__tag == "Err" or __try_0.__tag == "None" then return __try_0 end
        local a = __try_0[1]
        local __try_1 = evaluate(rhs, env)
        if __try_1.__tag == "Err" or __try_1.__tag == "None" then return __try_1 end
        local b = __try_1[1]
        return apply(op, a, b)
      end)()
    end
  end)()
end
local function shape(e)
  return (function()
    local __s = e
    if __s.__tag == "Bin" and __s.op.__tag == "Div" and __s.lhs.__tag == "Lit" and __s.lhs[1] == 0 then
      local rhs = __s.rhs
      return "0 / _"
    end
    if __s.__tag == "Bin" and __s.op.__tag == "Div" then
      local lhs = __s.lhs
      local rhs = __s.rhs
      return "_ / _"
    end
    if __s.__tag == "Bin" then
      local op = __s.op
      local lhs = __s.lhs
      local rhs = __s.rhs
      return "binary op"
    end
    if __s.__tag == "Lit" then
      local n = __s[1]
      return "literal"
    end
    if true then
      return "variable"
    end
  end)()
end
local function band(n)
  return (function()
    local __s = n
    if __s == 0 then
      return "zero"
    end
    if ((__s == 1) or (__s == 2) or (__s == 3)) then
      return "low"
    end
    if true then
      local n = __s
      if (n > 100) then
        return "high"
      end
    end
    if true then
      return "mid"
    end
  end)()
end
local Outcome = {}
Outcome.__index = Outcome
function Outcome.new(fields)
  fields.__struct = "Outcome"
  return __check_Outcome(setmetatable(fields, Outcome), "Outcome")
end
function Outcome.display(self)
  return tostring(self.label) .. ", " .. tostring(self.value)
end
Outcome.__traits = { "Display", "Report" }
function Outcome.outcome(self)
  return ((self.label .. " = ") .. render(self.value))
end
function Outcome.describe(self)
  return ("result: " .. self:outcome())
end
local function first(a, b)
  return a
end
render = function(n)
  return (function()
    if (n == 0) then
      return "0"
    else
      local neg = (n < 0)
      local m = (function()
        if neg then
          return (0 - n)
        else
          return n
        end
      end)()
      local out = ""
      while (m > 0) do
        local d = (m % 10)
        out = (digit(d) .. out)
        m = idiv(m, 10)
      end
      return (function()
        if neg then
          return ("-" .. out)
        else
          return out
        end
      end)()
    end
  end)()
end
digit = function(d)
  return (function()
    local __s = d
    if __s == 0 then
      return "0"
    end
    if __s == 1 then
      return "1"
    end
    if __s == 2 then
      return "2"
    end
    if __s == 3 then
      return "3"
    end
    if __s == 4 then
      return "4"
    end
    if __s == 5 then
      return "5"
    end
    if __s == 6 then
      return "6"
    end
    if __s == 7 then
      return "7"
    end
    if __s == 8 then
      return "8"
    end
    if __s == 9 then
      return "9"
    end
    if true then
      return "?"
    end
  end)()
end
local function run(label, e, env)
  return (function()
    local __s = evaluate(e, env)
    if __s.__tag == "Ok" then
      local v = __s[1]
      return (function()
        local o = Outcome.new({ label = label, value = v })
        return o:describe()
      end)()
    end
    if __s.__tag == "Err" then
      local err = __s[1]
      return ((label .. " failed: ") .. err:display())
    end
  end)()
end
local function main()
  local env = Env.new({ bindings = { Binding.new({ name = "x", value = 6 }), Binding.new({ name = "y", value = 7 }), Binding.new({ name = "z", value = 0 }) } })
  local e1 = { __enum = "Expr", __tag = "Bin", op = { __enum = "Op", __tag = "Sub" }, lhs = { __enum = "Expr", __tag = "Bin", op = { __enum = "Op", __tag = "Mul" }, lhs = { __enum = "Expr", __tag = "Var", VarId.new("x") }, rhs = { __enum = "Expr", __tag = "Var", VarId.new("y") } }, rhs = { __enum = "Expr", __tag = "Lit", 2 } }
  local e2 = { __enum = "Expr", __tag = "Bin", op = { __enum = "Op", __tag = "Div" }, lhs = { __enum = "Expr", __tag = "Var", VarId.new("x") }, rhs = { __enum = "Expr", __tag = "Var", VarId.new("z") } }
  local e3 = { __enum = "Expr", __tag = "Bin", op = { __enum = "Op", __tag = "Add" }, lhs = { __enum = "Expr", __tag = "Var", VarId.new("w") }, rhs = { __enum = "Expr", __tag = "Lit", 1 } }
  println(run("(x * y) - 2", e1, env))
  println(run("x / z", e2, env))
  println(run("w + 1", e3, env))
  local names = { "x", "y", "missing" }
  local probe = function(name) return env:lookup(name):map(function(v) return (v + 100) end):unwrap_or(0) end
  for _, name in ipairs(names) do
    println(((name .. " -> ") .. render(probe(name))))
  end
  local zero_div = { __enum = "Expr", __tag = "Bin", op = { __enum = "Op", __tag = "Div" }, lhs = { __enum = "Expr", __tag = "Lit", 0 }, rhs = { __enum = "Expr", __tag = "Var", VarId.new("x") } }
  println(("shape(0 / x) = " .. shape(zero_div)))
  println(("band(2) = " .. band(2)))
  println(("band(250) = " .. band(250)))
  local picked = first(11, 22)
  local parity = (function()
    if ((picked % 2) == 0) then
      return "even"
    else
      return "odd"
    end
  end)()
  println((((("first(11, 22) = " .. render(picked)) .. " (") .. parity) .. ")"))
  local a = Binding.new({ name = "x", value = 6 })
  local b = a:clone()
  println(("clone eq: " .. boolstr(a:eq(b))))
  println(("debug: " .. b:debug()))
end
boolstr = function(t)
  return (function()
    if t then
      return "true"
    else
      return "false"
    end
  end)()
end
--# sourceMappingURL=expr-eval.lua.map
