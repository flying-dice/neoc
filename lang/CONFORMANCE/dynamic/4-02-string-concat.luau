local function greet(name: string): string
  return (("hello, " .. name) .. "!")
end
local function main()
  println(greet("world"))
end
