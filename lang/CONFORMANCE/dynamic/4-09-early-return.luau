local function clamp(n: number): number
  if (n < 0) then
    return 0
  end
  if (n > 100) then
    return 100
  end
  return n
end
local function main()
  println(clamp((-5)))
  println(clamp(250))
  println(clamp(42))
end
