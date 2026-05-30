type Shape = { __enum: "Shape", __tag: "Dot" } | { __enum: "Shape", __tag: "Move", [number]: number } | { __enum: "Shape", __tag: "Box", w: number, h: number }

local function area(s: Shape): number
  return (function()
    local __s = s
    if __s.__tag == "Move" then
      local dx = __s[1]
      local dy = __s[2]
      return (dx + dy)
    end
    if __s.__tag == "Box" then
      local w = __s.w
      local h = __s.h
      return (w * h)
    end
    if true then
      return 0
    end
  end)()
end
local function main()
  println(area({ __enum = "Shape", __tag = "Move", 3, 4 }))
  println(area({ __enum = "Shape", __tag = "Box", w = 5, h = 6 }))
  println(area({ __enum = "Shape", __tag = "Dot" }))
end
