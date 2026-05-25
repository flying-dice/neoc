local function describe(s)
  return (function()
    local __s = s
    if __s.tag == "Dot" then
      return "dot"
    end
    if __s.tag == "Move" then
      local a = __s[1]
      local b = __s[2]
      return "move"
    end
    if __s.tag == "Circle" then
      local radius = __s.radius
      return "circle"
    end
  end)()
end
local function main()
  print(describe({ _enum = "Shape", tag = "Dot" }))
  print(describe({ _enum = "Shape", tag = "Move", 1, 2 }))
  print(describe({ _enum = "Shape", tag = "Circle", radius = 5 }))
end
