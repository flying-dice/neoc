local function loud(tag: string): boolean
  println(tag)
  return true
end
local function main()
  local _ = (false and loud("and-skipped"))
  local _ = (true or loud("or-skipped"))
  local _ = (true and loud("and-run"))
  local _ = (false or loud("or-run"))
end
