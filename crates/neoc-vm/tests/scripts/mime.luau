--!strict
-- lib:mime as a script sees it: its answers (mime.json), then what it asserts.
local mime = require("lib:mime")
local out = {}

out.from_extension = {
  (mime.from_extension(".json")),
  (mime.from_extension("html")),
  (mime.from_extension(".css")),
  (mime.from_extension(".png")),
}
out.unknown_extension = {
  (mime.from_extension(".zzzzz")) == nil,
  (mime.from_extension("")) == nil,
  (mime.from_extension(".")) == nil,
}

out.to_extension = {
  (mime.to_extension("text/html")),
  (mime.to_extension("image/jpeg")),
  (mime.to_extension("application/json")),
  (mime.to_extension("image/svg+xml")),
  (mime.to_extension("text/javascript")),
}
local prefs = { ["text/html"] = ".xhtml" }
out.prefs = {
  (mime.to_extension("text/html", prefs)),
  (mime.to_extension("application/json", prefs)),
  (mime.to_extension("image/jpeg", prefs)),
  (mime.to_extension("text/css", prefs)),
}
out.no_extension = (mime.to_extension("application/x-neoc-nothing")) == nil

local invalid = {}
for i, text in { "not-a-mime", "", "." } do
  local value, err = mime.to_extension(text)
  invalid[i] = { value == nil, err ~= nil }
end
out.invalid = invalid

out.detect = {
  (mime.detect("style.css")),
  (mime.detect("archive.tar.gz")),
  (mime.detect("assets/images/logo.png")),
}
out.undetected = { (mime.detect("Makefile")) == nil, (mime.detect("")) == nil, (mime.detect(".")) == nil }

-- what it asserts
do
  assert(select("#", mime.from_extension(".zzzzz")) == 1 and mime.from_extension(".zzzzz") == nil)
  assert(select("#", mime.detect("Makefile")) == 1 and mime.detect("Makefile") == nil)
  local none, none_err = mime.to_extension("application/x-neoc-nothing")
  assert(none == nil and none_err == nil)

  local value, err = mime.to_extension("not-a-mime")
  assert(value == nil and err ~= nil)
  assert(err.kind == "invalid_mime" and err.mime_type == "not-a-mime" and err.op == "lib:mime.to_extension")
  assert(tostring(err):find("lib:mime.to_extension: invalid_mime:", 1, true))

  local ok, refused = pcall(function()
    mime.to_extension("text/html", { ["text/html"] = 1 } :: any) -- deliberate refusal test: a preference is text
  end)
  assert(not ok and tostring(refused):find("argument #2 'prefs'", 1, true))
end

return out
