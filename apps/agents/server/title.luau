--!strict
-- A chat's title: the first line of its first message, cut to 48 characters on a character boundary (pure).
local TITLE_MAX = 48

local M = {}

M.NEW = "New chat"

function M.of(text: string): string
  local line = string.gsub(string.match(text, "^%s*([^\n]*)") or "", "%s+$", "")
  local length = utf8.len(line)
  if length == nil or length <= TITLE_MAX then
    if line == "" then
      return M.NEW
    end
    return line
  end
  local cut = (utf8.offset(line, TITLE_MAX) :: number) - 1
  return string.sub(line, 1, cut) .. "…"
end

return M
