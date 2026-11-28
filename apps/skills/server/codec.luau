--!strict
-- JSON text to a plain Luau value and back. The files of a skill besides its SKILL.md are kept as JSON text in the
-- database (skills.luau).
local json = require("vnd:serde_json")

local codec = {}

-- A value as JSON text. A value that cannot be JSON (a function in it) is `null`.
function codec.encode(value: any): string
  if value == nil then
    return "null"
  end
  return json.to_string(value) or "null"
end

-- JSON text as a plain value: an object or array is a table, `null` is nil. Text that is not JSON is nil.
function codec.decode(text: string): any
  local value = json.from_str(text)
  if type(value) == "userdata" then
    -- an object or an array is a handle with `to_table`; the `null` sentinel is a handle without it
    local ok, plain = pcall(function(): any
      return (value :: any):to_table()
    end)
    return if ok then plain else nil
  end
  return value
end

return codec
