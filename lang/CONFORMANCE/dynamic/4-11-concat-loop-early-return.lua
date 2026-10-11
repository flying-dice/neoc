local function upto(limit)
  local s = ""
  do
    local __sb_1 = { (s .. "") }
    for i = 0, (10) - 1 do
      __sb_1[#__sb_1 + 1] = "x"
      if (i == limit) then
        return table.concat(__sb_1)
      end
    end
    s = table.concat(__sb_1)
  end
  return (s .. "|end")
end
local function verdict(limit)
  local s = ""
  do
    local __sb_2 = { (s .. "") }
    for i = 0, (10) - 1 do
      __sb_2[#__sb_2 + 1] = "y"
      if (i == limit) then
        return "early"
      end
    end
    s = table.concat(__sb_2)
  end
  return s
end
local function main()
  println(upto(2))
  println(upto(99))
  println(verdict(3))
  println(verdict(99))
end
