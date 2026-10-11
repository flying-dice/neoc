local function main()
  local s = ""
  local i = 0
  do
    local __sb_1 = { (s .. "") }
    while true do
      local __brk_1 = false
      repeat
        i = (i + 1)
        if (i == 2) then
          break
        end
        __sb_1[#__sb_1 + 1] = "x"
        if (i == 5) then
          do __brk_1 = true; break end
        end
      until true
      if __brk_1 then break end
    end
    s = table.concat(__sb_1)
  end
  println(s)
  local w = "w"
  local n = 0
  do
    local __sb_2 = { (w .. "") }
    while (n < 3) do
      n = (n + 1)
      __sb_2[#__sb_2 + 1] = "!"
    end
    w = table.concat(__sb_2)
  end
  println(w)
end
