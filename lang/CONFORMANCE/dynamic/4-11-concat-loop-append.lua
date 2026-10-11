local function digit(i)
  return (function()
    if (i == 0) then
      return "a"
    else
      if (i == 1) then
        return "b"
      else
        return "c"
      end
    end
  end)()
end
local function main()
  local s = "start:"
  do
    local __sb_1 = { (s .. "") }
    for i = 0, (5) - 1 do
      __sb_1[#__sb_1 + 1] = (digit((i % 3)) .. "")
      __sb_1[#__sb_1 + 1] = "-"
      __sb_1[#__sb_1 + 1] = "("
      __sb_1[#__sb_1 + 1] = (digit((i % 2)) .. "")
      __sb_1[#__sb_1 + 1] = ")"
    end
    s = table.concat(__sb_1)
  end
  println(s)
  local t = "same"
  do
    local __sb_2 = { (t .. "") }
    for i = 0, (0) - 1 do
      __sb_2[#__sb_2 + 1] = "x"
    end
    t = table.concat(__sb_2)
  end
  println(t)
end
