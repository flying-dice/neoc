-- intentional: a type or extension that is not known is normal absence (a single nil, it was (nil, "unknown ...")); the only error left is text that is not a MIME type, typed as invalid_mime; the preference map is a typed { [string]: string }.
local mime = require("lib:mime")

assert(select("#", mime.from_extension(".zzzzz")) == 1 and mime.from_extension(".zzzzz") == nil)
assert(select("#", mime.detect("Makefile")) == 1 and mime.detect("Makefile") == nil)
local none, none_err = mime.to_extension("application/x-neoc-nothing")
assert(none == nil and none_err == nil)

local value, err = mime.to_extension("not-a-mime")
assert(value == nil and err.kind == "invalid_mime" and err.mime_type == "not-a-mime" and err.op == "lib:mime.to_extension")
assert(tostring(err):find("lib:mime.to_extension: invalid_mime:", 1, true))

local ok, refused = pcall(mime.to_extension, "text/html", { ["text/html"] = 1 })
assert(not ok and tostring(refused):find("argument #2 'prefs'", 1, true))
