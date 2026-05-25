local Ok   = function(v) return { _enum = "Result", tag = "Ok", v } end
local Err  = function(e) return { _enum = "Result", tag = "Err", e } end
local Some = function(v) return { _enum = "Option", tag = "Some", v } end
local None = { _enum = "Option", tag = "None" }

local function incr(o)
  local __try_0 = o
  if __try_0.tag == "Err" or __try_0.tag == "None" then return __try_0 end
  local n = __try_0[1]
  return Some((n + 1))
end
local function unwrap_or(o)
  return (function()
    local __s = o
    if __s.tag == "Some" then
      local n = __s[1]
      return n
    end
    if __s.tag == "None" then
      return 0
    end
  end)()
end
local function main()
  println(unwrap_or(incr(Some(41))))
  println(unwrap_or(incr(None)))
end
