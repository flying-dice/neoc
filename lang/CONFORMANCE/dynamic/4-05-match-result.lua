local Ok   = function(v) return { __enum = "Result", __tag = "Ok", v } end
local Err  = function(e) return { __enum = "Result", __tag = "Err", e } end
local Some = function(v) return { __enum = "Option", __tag = "Some", v } end
local None = { __enum = "Option", __tag = "None" }

local function describe(r)
  return (function()
    local __s = r
    if __s.__tag == "Ok" then
      local n = __s[1]
      return "ok"
    end
    if __s.__tag == "Err" then
      local e = __s[1]
      return e
    end
  end)()
end
local function main()
  print(describe(Ok(7)))
  print(describe(Err("boom")))
end
