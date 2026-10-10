local function main()
  local half = (function() local __r = {} for __i = 1, (4) - 1 do __r[#__r + 1] = __i end return __r end)()
  local incl = (function() local __r = {} for __i = 1, 4 do __r[#__r + 1] = __i end return __r end)()
  local sum = 0
  for _, x in ipairs(half) do
    sum = (sum + x)
  end
  for _, y in ipairs(incl) do
    sum = (sum + y)
  end
  println(sum)
end
