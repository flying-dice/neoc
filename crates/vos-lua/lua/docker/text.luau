--!strict
-- What the server wrote, made fit to show: logs are bytes, and a page shows text.

local text = {}

-- a colour or cursor sequence of a terminal: ESC [ ... letter
local ANSI = "\27%[[%d;?]*%a"

-- `raw` as valid UTF-8: a byte that is not part of a character becomes U+FFFD
local function valid(raw: string): string
  if utf8.len(raw) then
    return raw
  end
  local out: { string } = {}
  local at = 1
  while at <= #raw do
    local length, bad = utf8.len(raw, at)
    if length then
      table.insert(out, string.sub(raw, at))
      break
    end
    local stop = bad :: number
    table.insert(out, string.sub(raw, at, stop - 1))
    table.insert(out, "\u{FFFD}")
    at = stop + 1
  end
  return table.concat(out)
end

-- The end of `raw`, at most `limit` bytes of it, as text: from the start of a line when it was cut, without a
-- terminal's colour sequences and control characters (line breaks and tabs stay), and valid UTF-8. The second result
-- says whether it was cut.
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
  raw = string.gsub(raw, ANSI, "")
  raw = string.gsub(raw, "\r\n", "\n")
  raw = string.gsub(raw, "[%z\1-\8\11-\31\127]", "")
  return valid(raw), cut
end

return text
