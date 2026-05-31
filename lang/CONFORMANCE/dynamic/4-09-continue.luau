local function main()
  local acc = 0
  for i = 0, (10) - 1 do
    local __brk_1 = false
    repeat
      if (i == 3) then
        break
      end
      if (i == 7) then
        do __brk_1 = true; break end
      end
      acc = (acc + i)
    until true
    if __brk_1 then break end
  end
  println(acc)
end
