local function main()
  local stepped = function(n)
    local doubled = (n * 2)
    return (doubled + 1)
  end
  println(stepped(20))
  println(stepped(0))
end
