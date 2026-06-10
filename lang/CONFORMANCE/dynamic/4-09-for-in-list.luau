local function main()
  local xs = { 2, 4, 6, 8 }
  local sum = 0
  for _, x in ipairs(xs) do
    sum = (sum + x)
  end
  println(sum)
  local acc = 0
  for _, x in ipairs(xs) do
    acc = ((acc * 10) + x)
  end
  println(acc)
end
