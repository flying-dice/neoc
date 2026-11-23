-- intentional: a parse failure is a typed error (control_byte, unclosed_section, empty_section_name, syntax) with the line as a number, stringify returns the text alone and writes sections and keys in sorted order (it was the table's own order), a comment symbol or delimiter is the whole string (it was its first character) and with symbols of the caller's choosing `;`, `#` and `=` are plain characters, and options and values are checked by their types.
local ini = require("lib:ini")

-- typed errors
local _, unclosed = ini.parse("a=1\n[bad")
assert(unclosed.kind == "unclosed_section" and unclosed.line == 2 and unclosed.op == "lib:ini.parse")
assert(tostring(unclosed) == "lib:ini.parse: unclosed_section: line 2: unclosed section header")
local _, empty = ini.parse("[]")
assert(empty.kind == "empty_section_name" and empty.line == 1)
local _, control = ini.parse("key=val\0ue")
assert(control.kind == "control_byte" and control.offset == 7 and control.byte == 0)

-- stringify: one value, sorted
local text, extra = ini.stringify({ zeta = { k = "v" }, [""] = { name = "world" }, alpha = { b = 2, a = "1", c = true } })
assert(text == "name=world\n\n[alpha]\na=1\nb=2\nc\n\n[zeta]\nk=v\n" and extra == nil)

-- symbols are whole strings, and the defaults they replace are plain characters
local doc = assert(ini.parse("// note\n/ not=a comment\n;x=1\n#y", { comment_symbols = { "//" } }))
assert(doc[""]["/ not"] == "a comment" and doc[""][";x"] == "1" and doc[""]["#y"] == true)
local arrows = assert(ini.parse("a => 1\nb: 2 => 3\nc=4", { delimiters = { "=>", ":" } }))
assert(arrows[""].a == "1" and arrows[""].b == "2 => 3" and arrows[""]["c=4"] == true)

-- arguments are checked by their types
local ok, refused = pcall(ini.parse, "a=1", { lowercase = true })
assert(not ok and tostring(refused):find("lib:ini.parse: argument #2 'opts'", 1, true))
assert(not pcall(ini.parse, "a=1", { delimiters = "=" }))
local ok2, refused2 = pcall(ini.stringify, { s = { bad = {} } })
assert(not ok2 and tostring(refused2):find("lib:ini.stringify: argument #1 'config'", 1, true))
assert(not pcall(ini.stringify, { s = { k = "v" } }, { spacing = "yes" }))
