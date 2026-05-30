local function grade(n)
  return (function()
    if (n >= 90) then
      return "A"
    else
      if (n >= 80) then
        return "B"
      else
        if (n >= 70) then
          return "C"
        else
          return "F"
        end
      end
    end
  end)()
end
local function main()
  println(grade(95))
  println(grade(82))
  println(grade(40))
end
