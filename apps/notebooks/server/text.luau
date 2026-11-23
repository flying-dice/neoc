--!strict
-- What a note's text says about it: the notes it links to (`[[Title]]`), its tags (`#tag`), and the line that stands
-- for it in a list. Everything here is a function of text alone: nothing reads the database.
--
-- A link is `[[Title]]`, `[[Title|shown text]]` or `[[Title#Heading]]`: the title is what is before the `|` and the
-- `#`. Text in a fenced block (```) or in `inline code` is code, not writing: it has no links and no tags.
local strings = require("lib:text")

local text = {}

-- the inside of a link: no bracket and no line break
local LINK = "%[%[([^%[%]\n]+)%]%]"
-- what cannot be in a title, because a link is written with it
local FORBIDDEN = "[%[%]|#%^\n]"

text.TITLE_MAX = 120
local EXCERPT_MAX = 160

-- How a title is compared: two titles that differ only in capitals and outer spaces name the same note. The key is
-- kept with each note and in each link to it, so it stays the ASCII lower case those were written with.
function text.key(title: string): string
  return string.lower(strings.trim(title))
end

-- A line as an excerpt: at most `EXCERPT_MAX` characters, the last an ellipsis when some were left out.
local function excerpt(line: string): string
  return strings.clip(line, EXCERPT_MAX, "…")
end

-- Why a note cannot be called `title` (trimmed), or nothing when it can.
function text.titleProblem(title: string): string?
  if title == "" then
    return "A note needs a title."
  elseif (utf8.len(title) or #title) > text.TITLE_MAX then
    return `A title is at most {text.TITLE_MAX} characters.`
  elseif string.find(title, FORBIDDEN) ~= nil then
    return "A title cannot have [ ] | # or ^ in it."
  end
  return nil
end

local function lines(body: string): { string }
  return string.split(body, "\n")
end

-- the lines that are writing: none of a fenced block, and no inline code
local function prose(body: string): { string }
  local kept: { string } = {}
  local fenced = false
  for _, line in lines(body) do
    if string.match(line, "^%s*```") ~= nil then
      fenced = not fenced
    elseif not fenced then
      table.insert(kept, (string.gsub(line, "`[^`]*`", "")))
    end
  end
  return kept
end

-- the title a link's inside names: what is before the `|` and the `#`
local function targetOf(inside: string): string
  return text.key(string.match(inside, "^[^|#]*") or "")
end

-- A line without the marks of its styles and its links: `**Sow** in [[March|spring]]` reads `Sow in spring`.
local function plain(line: string): string
  local unlinked = string.gsub(line, LINK, function(inside: string): string
    return strings.trim(string.match(inside, "|(.*)$") or inside)
  end)
  local unmarked = string.gsub(unlinked, "%*%*", "")
  unmarked = string.gsub(unmarked, "~~", "")
  unmarked = string.gsub(unmarked, "==", "")
  return (string.gsub(unmarked, "`", ""))
end

-- The keys of the titles a text links to, each once, in the order they are first linked.
function text.links(body: string): { string }
  local found: { string } = {}
  local seen: { [string]: boolean } = {}
  for _, line in prose(body) do
    for inside in string.gmatch(line, LINK) do
      local target = targetOf(inside)
      if target ~= "" and not seen[target] then
        seen[target] = true
        table.insert(found, target)
      end
    end
  end
  return found
end

-- The tags of a text, in lower case without the `#`, each once, in alphabetical order. A tag is a `#` at the start
-- of a word, followed by letters, digits, `_`, `-` and `/`, not all of them digits (`#1` is a number).
function text.tags(body: string): { string }
  local found: { string } = {}
  local seen: { [string]: boolean } = {}
  for _, line in prose(body) do
    -- a link's `#Heading` is not a tag
    local plain = string.gsub(line, LINK, "")
    for tag in string.gmatch(" " .. plain, "%s#([%w_/%-]+)") do
      local name = string.lower(tag)
      if string.find(name, "[^%d]") ~= nil and not seen[name] then
        seen[name] = true
        table.insert(found, name)
      end
    end
  end
  table.sort(found)
  return found
end

-- The line that stands for a text in a list: its first line with something on it that is not a heading (a note's
-- first heading is often its title again), or its first heading when it has nothing else. It is read without the
-- marks of a list, a quote, a style or a link.
function text.excerpt(body: string): string
  local heading = ""
  for _, line in lines(body) do
    local said = strings.trim(plain((string.gsub(line, "^[%s#>%-%*%+]+", ""))))
    if said ~= "" and string.match(line, "^%s*```") == nil then
      if string.match(line, "^#+%s") == nil then
        return excerpt(said)
      elseif heading == "" then
        heading = said
      end
    end
  end
  return excerpt(heading)
end

-- The first line of a text that links to the title with this key, without its marks; empty when none does.
function text.mention(body: string, key: string): string
  for _, line in prose(body) do
    for inside in string.gmatch(line, LINK) do
      if targetOf(inside) == key then
        return excerpt(strings.trim(plain(line)))
      end
    end
  end
  return ""
end

-- The first line of a text that has `words` in it, whatever their capitals, without its marks; empty when none does.
function text.line(body: string, words: string): string
  local wanted = string.lower(words)
  for _, line in lines(body) do
    if string.find(string.lower(line), wanted, 1, true) ~= nil then
      return excerpt(strings.trim(plain(line)))
    end
  end
  return ""
end

-- A text with its links to the title with this key rewritten to `title`: the shown text and the heading of each link
-- stay as they were.
function text.retarget(body: string, key: string, title: string): string
  return (
    string.gsub(body, LINK, function(inside: string): string
      if targetOf(inside) ~= key then
        return "[[" .. inside .. "]]"
      end
      return "[[" .. title .. (string.match(inside, "[|#].*$") or "") .. "]]"
    end)
  )
end

return text
