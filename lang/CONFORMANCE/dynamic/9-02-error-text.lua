local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local function __check_list(v, path, elem)
  if type(v) ~= "table" or v.__struct or v.__enum then __type_error(path, "list", v) end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  for i = 1, n do
    if v[i] == nil then __type_error(path, "list", v) end
    elem(v[i], path .. "[" .. (i - 1) .. "]")
  end
  return v
end
local __check_Tags
function __check_Tags(v, path)
  if type(v) ~= "table" or v.__struct ~= "Tags" then __type_error(path, "Tags", v) end
  __check_list(v.names, path .. ".names", __check_string)
  return v
end
local Tags = {}
Tags.__index = Tags
function Tags.new(fields)
  fields.__struct = "Tags"
  return __check_Tags(setmetatable(fields, Tags), "Tags")
end
local function main()
  local ok = Tags.new({ names = { "a", "b" } })
  println(ok.names[(1) + 1])
  local bad = Tags.new({ names = { "a", tonumber("9") } })
  println(bad.names[(0) + 1])
end
