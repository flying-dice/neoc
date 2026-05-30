local function main()
  local acc = 0
  for i = 0, (3) - 1 do
    for j = 0, (3) - 1 do
      local __brk_1 = false
      repeat
        if (j == 1) then
          break
        end
        if (j == 2) then
          do __brk_1 = true; break end
        end
        acc = ((acc + (i * 10)) + j)
      until true
      if __brk_1 then break end
    end
  end
  println(acc)
end
