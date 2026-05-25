local function main()
  local x = 7
  local y = (function()
    if (x > 5) then
      return (x * 2)
    else
      return 0
    end
  end)()
  y = (y + 1)
  println(y)
end
