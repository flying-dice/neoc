local function name(x)
  return (function()
    local __s = x
    if __s == 0.5 then
      return "half"
    end
    if __s == 2.25 then
      return "two-and-a-quarter"
    end
    if true then
      return "other"
    end
  end)()
end
local function main()
  println(name(0.5))
  println(name(2.25))
  println(name(3.0))
end
