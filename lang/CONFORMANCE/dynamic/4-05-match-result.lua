local Ok   = function(v) return { _enum = "Result", tag = "Ok", v } end
local Err  = function(e) return { _enum = "Result", tag = "Err", e } end
local Some = function(v) return { _enum = "Option", tag = "Some", v } end
local None = { _enum = "Option", tag = "None" }

local function describe(r)
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
  print(describe(Ok(7)))
  print(describe(Err("boom")))
end
