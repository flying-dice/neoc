-- lib:csv on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Only rows as lists are shared: records, streams and the writing functions were reshaped (see csv.vm2.luau).
local csv = require("lib:csv")
local out = {}

local rows, err = csv.parse("a,b,c\n1,2,3\n4,5,6")
out.parse = { rows, err == nil }
out.trailing_newline = csv.parse("a,b\n1,2\n")
out.crlf = csv.parse("a,b\r\n1,2\r\n")
out.empty_fields = csv.parse("a,,c\n,,")
out.empty_text = csv.parse("")
out.blank_lines = csv.parse("a,b\n\n1,2\n")
out.quoted = csv.parse('"a,b","say ""hi""","two\nlines"\nx,y,z')
out.spaces = csv.parse(" a , b \n c,d ")
out.unicode = csv.parse("名前,値\nキー,\"quoted 値\"")

out.tsv = csv.parse("a\tb\nx\ty", { delimiter = "\t" })
out.semicolons = csv.parse("'a;b';c\nd;e", { delimiter = ";", quote = "'" })
out.escape = csv.parse('"a\\"b",c', { escape = "\\" })
out.comment = csv.parse("# note\na,b\n# more\n1,2", { comment = "#" })
out.no_comment_by_default = csv.parse("#a,b\n1,2")

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	more_fields = "a,b\n1,2,3",
	fewer_fields = "a,b\n1",
} do
	local value, why = csv.parse(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out
