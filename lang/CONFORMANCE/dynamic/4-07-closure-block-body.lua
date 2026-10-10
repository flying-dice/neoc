local function main()
  local stepped = function(n) return (function()
    local doubled = (n * 2)
    return (doubled + 1)
  end)() end
  println(stepped(20))
  println(stepped(0))
end
