local function yes_no(b)
  return (function()
    local __s = b
    if __s == true then
      return "yes"
    end
    if __s == false then
      return "no"
    end
  end)()
end
local function main()
  println(yes_no(true))
  println(yes_no(false))
end
