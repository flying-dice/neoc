local function classify(n)
  return (function()
    local __s = n
    if __s == 0 then
      return "zero"
    end
    if ((__s == 1) or (__s == 2) or (__s == 3)) then
      return "small"
    end
    if true then
      local x = __s
      if (x > 100) then
        return "huge"
      end
    end
    if true then
      return "other"
    end
  end)()
end
local function main()
  println(classify(0))
  println(classify(2))
  println(classify(200))
  println(classify(50))
end
