local Ok   = function(v) return { _enum = "Result", tag = "Ok", v } end
local Err  = function(e) return { _enum = "Result", tag = "Err", e } end
local Some = function(v) return { _enum = "Option", tag = "Some", v } end
local None = { _enum = "Option", tag = "None" }

local function parse(ok)
  return (function()
    if ok then
      return Ok(10)
    else
      return Err("bad")
    end
  end)()
end
local function doubled(ok)
  local __try_0 = parse(ok)
  if __try_0.tag == "Err" or __try_0.tag == "None" then return __try_0 end
  local n = __try_0[1]
  return Ok((n + n))
end
local function show(r)
  return (function()
    local __s = r
    if __s.tag == "Ok" then
      local n = __s[1]
      return "ok"
    end
    if __s.tag == "Err" then
      local e = __s[1]
      return e
    end
  end)()
end
local function main()
  println(show(doubled(true)))
  println(show(doubled(false)))
end
