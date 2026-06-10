local function classify(n: number): number
  return (function()
    local __s = n
    if __s == 0 then
      return 100
    end
    if __s == 1 then
      return 200
    end
    if true then
      return 999
    end
  end)()
end
local function main()
  println(classify(0))
  println(classify(1))
  println(classify(7))
end
