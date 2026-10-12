local prefix
local Greet = {}
function Greet.hi(self)
  return (prefix() .. self:name())
end
prefix = function()
  return "hi "
end
return { Greet = Greet }
