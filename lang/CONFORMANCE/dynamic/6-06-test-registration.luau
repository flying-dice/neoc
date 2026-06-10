local __tests = {}

local function checks_alpha()
  println("alpha-ran")
end
__tests["checks_alpha"] = checks_alpha
local function checks_beta()
  println("beta-ran")
end
__tests["checks_beta"] = checks_beta
local function main()
  checks_alpha()
  checks_beta()
end
