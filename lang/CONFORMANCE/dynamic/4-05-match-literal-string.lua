local function code(word)
  return (function()
    local __s = word
    if __s == "yes" then
      return 1
    end
    if __s == "no" then
      return 2
    end
    if true then
      return 0
    end
  end)()
end
local function main()
  println(code("yes"))
  println(code("no"))
  println(code("maybe"))
end
