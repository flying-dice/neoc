local function main()
  local xs = { 1, 2, 3 }
  xs[(0) + 1] = xs[(0) + 1] + 10
  xs[(0) + 1] = xs[(0) + 1] * 2
  xs[(1) + 1] = xs[(1) + 1] * 5
  local i = 2
  xs[(i) + 1] = xs[(i) + 1] - 1
  println(xs[(0) + 1])
  println(xs[(1) + 1])
  println(xs[(2) + 1])
end
