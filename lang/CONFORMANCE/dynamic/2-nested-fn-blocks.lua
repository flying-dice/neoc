local function base()
  return 100
end
local function main()
  local f = function(n)
    local function twice(k)
      return (k * 2)
    end
    local r = (n + 1)
    return twice(r)
  end
  println(f(4))
  local acc = 0
  for i = 0, (5) - 1 do
    local __brk_1 = false
    repeat
      local function inc(k)
        return (k + base())
      end
      if (i == 2) then
        break
      end
      acc = (acc + inc(i))
    until true
    if __brk_1 then break end
  end
  println(acc)
  local v = (function()
    local __s = (acc > 0)
    if __s == true then
      return (function()
        local function neg(k)
          return (0 - k)
        end
        return neg(acc)
      end)()
    end
    if __s == false then
      return 0
    end
  end)()
  println(v)
end
