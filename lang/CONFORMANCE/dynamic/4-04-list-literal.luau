local function main()
  local xs = { 10, 20, 30 }
  println(xs[(0) + 1])
  println(xs[(1) + 1])
  println(xs[(2) + 1])
  local base = 5
  local ys = { base, (base + 1), (base * 2) }
  println(ys[(0) + 1])
  println(ys[(1) + 1])
  println(ys[(2) + 1])
end
