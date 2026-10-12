local D = { __struct = "D" }
D.__traits = { "Make" }
local Make = {}
function Make.tag()
  return "made"
end
D.tag = Make.tag
local function main()
  println(D.tag())
end
