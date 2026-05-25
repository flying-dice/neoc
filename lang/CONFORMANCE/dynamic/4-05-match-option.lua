local Ok   = function(v) return { _enum = "Result", tag = "Ok", v } end
local Err  = function(e) return { _enum = "Result", tag = "Err", e } end
local Some = function(v) return { _enum = "Option", tag = "Some", v } end
local None = { _enum = "Option", tag = "None" }

local function describe(o)
  return (function()
    local __s = o
    if __s.tag == "Some" then
      local n = __s[1]
      return "some"
    end
    if __s.tag == "None" then
      return "none"
    end
  end)()
end
local function main()
  print(describe(Some(3)))
  print(describe(None))
end
