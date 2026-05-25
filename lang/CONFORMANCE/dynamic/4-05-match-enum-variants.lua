local function area(s)
  return (function()
    local __s = s
    if __s.tag == "Move" then
      local dx = __s[1]
      local dy = __s[2]
      return (dx + dy)
    end
    if __s.tag == "Box" then
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
  println(area({ _enum = "Shape", tag = "Move", 3, 4 }))
  println(area({ _enum = "Shape", tag = "Box", w = 5, h = 6 }))
  println(area({ _enum = "Shape", tag = "Dot" }))
end
