local function main()
  local s = ""
  while (s ~= "xxx") do
    s = (s .. "x")
  end
  println(s)
  local t = ""
  for i = 0, (3) - 1 do
    t = (t .. "y")
    println(t)
  end
end
