local function main()
  local xs = { 5, 6, 7, 8 }
  println(xs[(0) + 1])
  println(xs[(3) + 1])
  local i = 1
  println(xs[(i) + 1])
  println(xs[((i + 1)) + 1])
end
