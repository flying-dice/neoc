local function apply(f, x)
  return f(x)
end
local function main()
  local double = function(n) return (n * 2) end
  println(apply(double, 21))
end
