local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local function create(store, body)
  __check_string(body, "create.body")
  return (function()
    if (body == "") then
      return "error: body must not be empty"
    else
      local id = store:add(body)
      return ("added: " .. body)
    end
  end)()
end
local function list(store)
  local out = ""
  local i = 0
  local n = store:count()
  do
    local __sb_1 = { (out .. "") }
    while (i < n) do
      __sb_1[#__sb_1 + 1] = (store:at(i) .. "")
      __sb_1[#__sb_1 + 1] = " | "
      i = (i + 1)
    end
    out = table.concat(__sb_1)
  end
  return out
end
return { create = create, list = list }
