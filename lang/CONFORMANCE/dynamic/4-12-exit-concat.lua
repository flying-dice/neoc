local function append_break(n)
  local s = ""
  do
    local __sb_1 = { (s .. "") }
    for i = 0, (n) - 1 do
      local __exit_1, __val_1 = (function()
        if (i == 1) then
          return 2
        end
        return false, "b"
      end)()
      if __exit_1 then
        break
      end
      __sb_1[#__sb_1 + 1] = "a"
      __sb_1[#__sb_1 + 1] = (__val_1 .. "")
    end
    s = table.concat(__sb_1)
  end
  return s
end
local function append_continue(n)
  local s = ""
  do
    local __sb_2 = { (s .. "") }
    for i = 0, (n) - 1 do
      local __brk_1 = false
      repeat
        local __exit_2, __val_2 = (function()
          if (i == 1) then
            return 3
          end
          return false, "b"
        end)()
        if __exit_2 then
          break
        end
        __sb_2[#__sb_2 + 1] = "a"
        __sb_2[#__sb_2 + 1] = (__val_2 .. "")
      until true
      if __brk_1 then break end
    end
    s = table.concat(__sb_2)
  end
  return s
end
local function main()
  println(append_break(3))
  println(append_continue(3))
end
