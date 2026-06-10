local function classify(n: number)
  return (function()
    local __s = n
    if __s == 0 then
      return println("zero")
    end
    if __s == 1 then
      return println("one")
    end
    if true then
      return println("many")
    end
  end)()
end
local function main()
  classify(0)
  classify(1)
  classify(7)
end
