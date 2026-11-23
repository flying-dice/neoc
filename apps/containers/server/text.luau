--!strict
-- What a container wrote, made fit to show: logs are bytes, and a page shows text.

local strings = require("lib:text")

local text = {}

-- The end of `raw`, at most `limit` bytes of it, as text: from the start of a line when it was cut, valid UTF-8, and
-- without a terminal's colour sequences and control characters (line breaks and tabs stay). The second result says
-- whether it was cut.
function text.tail(raw: string, limit: number): (string, boolean)
  local cut = #raw > limit
  if cut then
    raw = string.sub(raw, #raw - limit + 1)
    -- the cut may have landed inside a line (and inside a character): start at the next whole line
    local newline = string.find(raw, "\n", 1, true)
    if newline then
      raw = string.sub(raw, newline + 1)
    end
  end
  return strings.strip_ansi(strings.to_valid_utf8(raw)), cut
end

return text
