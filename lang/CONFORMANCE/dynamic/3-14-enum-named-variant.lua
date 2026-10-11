local function area(s)
  return (function()
    local __s = s
    if __s.__tag == "Rect" then
      local w = __s.w
      local h = __s.h
      return (w * h)
    end
    if __s.__tag == "Dot" then
      return 0
    end
  end)()
end
local function main()
  println(area({ __enum = "Shape", __tag = "Rect", w = 3, h = 4 }))
  println(area({ __enum = "Shape", __tag = "Dot" }))
end
