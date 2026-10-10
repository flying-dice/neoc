local function value(t)
  return (function()
    local __s = t
    if __s.__tag == "Leaf" then
      local n = __s[1]
      return n
    end
    if __s.__tag == "Empty" then
      return 0
    end
  end)()
end
local function main()
  println(value({ __enum = "Tree", __tag = "Leaf", 5 }))
  println(value({ __enum = "Tree", __tag = "Empty" }))
end
