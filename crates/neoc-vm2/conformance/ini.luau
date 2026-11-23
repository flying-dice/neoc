-- lib:ini on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
local ini = require("lib:ini")
local out = {}

local doc, err = ini.parse("name=world\n; a comment\n# another\n\n[server]\nhost = localhost\nport=5432\nverbose\nempty=\n")
out.parse = { doc, err == nil }

out.duplicates = ini.parse("[s]\nk=a\nk=b\n[t]\nx=1\n[s]\nother=2")
out.no_inline_comments = ini.parse("[s]\nkey=value ; not a comment")
out.empty_section = ini.parse("[empty]\n[other]\nk=v")
out.unicode = ini.parse("[日本語]\nキー=値")
out.colons = ini.parse("[a:b]\nurl=http://host:80/x\nbare:key")
out.quoted = ini.parse('[s]\na="quoted value"\nb=\'single\'\nc=plain "inner" text')
out.whitespace = ini.parse("  [ s ]  \n   key   =   spaced value   \n\tflag\t")
out.crlf = ini.parse("[s]\r\nkey=value\r\nflag\r\n")
out.nothing = { ini.parse(""), ini.parse("; comment\n# another") }

out.lowercase = ini.parse("[Section]\nKey=Val\nFLAG", { lowercase_keys = true })
out.folded = ini.parse("[Foo]\na=1\n[FOO]\nB=2", { lowercase_keys = true })
out.comment_symbols = ini.parse("!ignored\nkey=val", { comment_symbols = { "!" } })
out.delimiters = ini.parse("[s]\nkey:val\nother: x:y", { delimiters = { ":" } })
out.both_delimiters = ini.parse("a=1:2\nb:3=4", { delimiters = { "=", ":" } })

-- a failure is `(nil, err)`; what `err` is differs (see ini.vm2.luau)
for name, text in {
	unclosed = "[bad",
	empty_name = "[]",
	control = "key=val\0ue",
	sentinel = "[s]\nkey\2=value",
} do
	local value, why = ini.parse(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

-- one key per section: neoc-vm writes keys in the table's own order
out.stringify = {
	ini.stringify({ section = { key = "value" } }),
	ini.stringify({ section = { flag = true } }),
	ini.stringify({ section = { off = false } }),
	ini.stringify({ section = { n = 42 } }),
	ini.stringify({ section = { n = 1.5 } }),
	ini.stringify({ [""] = { name = "world" }, section = { k = "v" } }),
	ini.stringify({ section = { key = "value" } }, { spacing = true }),
	ini.stringify({ section = { key = "value" } }, { spacing = false }),
	ini.stringify({}),
}
out.round_trip = ini.stringify((ini.parse("top=1\n\n[db]\nhost=localhost\n")))
out.stringify_refuses = { (pcall(ini.stringify, { s = { bad = {} } })), (pcall(ini.stringify, { s = { bad = print } })) }

return out
