type Shape = { __enum: "Shape", __tag: "Dot" } | { __enum: "Shape", __tag: "Move", [number]: number } | { __enum: "Shape", __tag: "Circle", radius: number }

local function describe(s: Shape): string
  return (function()
    local __s = s
    if __s.__tag == "Dot" then
      return "dot"
    end
    if __s.__tag == "Move" then
      local a = __s[1]
      local b = __s[2]
      return "move"
    end
    if __s.__tag == "Circle" then
      local radius = __s.radius
      return "circle"
    end
  end)()
end
local function main()
  print(describe({ __enum = "Shape", __tag = "Dot" }))
  print(describe({ __enum = "Shape", __tag = "Move", 1, 2 }))
  print(describe({ __enum = "Shape", __tag = "Circle", radius = 5 }))
end
