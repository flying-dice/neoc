local Ok   = function(v) return { __enum = "Result", __tag = "Ok", v } end
local Err  = function(e) return { __enum = "Result", __tag = "Err", e } end
local Some = function(v) return { __enum = "Option", __tag = "Some", v } end
local None = { __enum = "Option", __tag = "None" }

local function describe(o)
  return (function()
    local __s = o
    if __s.__tag == "Some" then
      local n = __s[1]
      return "some"
    end
    if __s.__tag == "None" then
      return "none"
    end
  end)()
end
local function main()
  print(describe(Some(3)))
  print(describe(None))
end
