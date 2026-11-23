-- lib:diff on neoc-vm and neoc-vm2: what both must answer the same (patches are written out as text here).
local diff = require("lib:diff")
local out = {}

local function summary(changes)
	local rows = {}
	for i, c in ipairs(changes) do
		rows[i] = c.op .. ":" .. c.value
	end
	return rows
end

out.lines = summary(diff.lines("a\nb\nc\n", "a\nB\nc\n"))
out.lines_myers = summary(diff.lines("1\n2\n3\n4\n5\n", "1\n2\nX\n4\n5\n", { algorithm = "myers" }))
out.lines_empty = { summary(diff.lines("", "a\nb\n")), summary(diff.lines("a\nb\n", "")) }
out.words = summary(diff.words("the quick brown fox", "the slow brown fox"))
out.chars = summary(diff.chars("kitten", "sitten"))

local old, new = "alpha beta gamma", "alpha BETA gamma"
out.apply_changes = {
	(diff.apply_changes(old, diff.words(old, new))),
	(diff.apply_changes("kitten", diff.chars("kitten", "sitting"))),
	(diff.apply_changes("alpha\nbravo\n", diff.lines("alpha\nbravo\n", "alpha\nBRAVO\n"))),
}
local misaligned, misaligned_err =
	diff.apply_changes("alpha\n", { { op = "equal", value = "wrong\n" }, { op = "insert", value = "x\n" } })
out.misaligned = { misaligned == nil, misaligned_err ~= nil }

local hunks_of = function(patch)
	local rows = {}
	for i, h in ipairs(patch.hunks) do
		rows[i] = { h.old_start, h.old_count, h.new_start, h.new_count, #h.lines }
	end
	return rows
end
local patch = diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "old.txt", new_label = "new.txt" })
out.unified = { patch.old_label, patch.new_label, hunks_of(patch) }
out.unified_defaults = { diff.unified("a\n", "b\n").old_label, diff.unified("a\n", "b\n").new_label }
out.unified_same = { #diff.unified("a\nb\n", "a\nb\n").hunks, #diff.unified("", "").hunks }
local ten_old = "1\n2\n3\n4\n5\nX\n7\n8\n9\n10\n"
local ten_new = "1\n2\n3\n4\n5\nY\n7\n8\n9\n10\n"
out.context = { #diff.unified(ten_old, ten_new, { context = 0 }).hunks[1].lines, #diff.unified(ten_old, ten_new, { context = 3 }).hunks[1].lines }
out.zero_context = hunks_of(diff.unified("a\nb\n", "a\nb\nc\n", { context = 0 }))
out.line_values = { diff.unified("a\nb", "a\nB").hunks[1].lines[2].value, diff.unified("a\nb\n", "a\nB\n").hunks[1].lines[2].value }

local text = "--- x.txt\n+++ y.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
local parsed = diff.parse_unified(text)
out.parse = { parsed.old_label, parsed.new_label, hunks_of(parsed), parsed.hunks[1].lines[2].value }
out.parse_no_newline = diff.parse_unified("--- a\n+++ b\n@@ -1,1 +1,1 @@\n-x\n\\ No newline at end of file\n+y\n\\ No newline at end of file\n").hunks[1].lines[1].value
out.parse_errors = {
	select(1, diff.parse_unified("just some text\nno headers\n")) == nil,
	select(1, diff.parse_unified("--- a\n+++ b\n")) == nil,
	select(1, diff.parse_unified("--- a\n+++ b\n@@ not a header @@\n")) == nil,
	select(1, diff.parse_unified("--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n")) == nil,
	select(1, diff.parse_unified("")) == nil,
}

out.apply = {
	(diff.apply("alpha\nbravo\ncharlie\n", "--- a\n+++ b\n@@ -1,3 +1,3 @@\n alpha\n-bravo\n+BRAVO\n charlie\n")),
	(diff.apply("alpha\nbravo", "--- a\n+++ b\n@@ -1,2 +1,2 @@\n alpha\n-bravo\n\\ No newline at end of file\n+BRAVO\n\\ No newline at end of file\n")),
	(diff.apply("a\nb\n", "--- a\n+++ b\n@@ -2,0 +3,1 @@\n+c\n")),
	(diff.apply("a\nb\n", "--- a\n+++ b\n@@ -0,0 +1,1 @@\n+X\n")),
}
local mismatch, mismatch_err = diff.apply("alpha\nNOT\ncharlie\n", "--- a\n+++ b\n@@ -1,3 +1,3 @@\n alpha\n-bravo\n+BRAVO\n charlie\n")
out.apply_mismatch = { mismatch == nil, mismatch_err ~= nil }
local malformed, malformed_err = diff.apply("a\n", "this isn't a patch\n")
out.apply_malformed = { malformed == nil, malformed_err ~= nil }

out.similarity = {
	diff.similarity("anything", "anything"),
	diff.similarity("", ""),
	diff.similarity("", "anything"),
	diff.similarity("anything", ""),
	diff.similarity("the quick brown fox", "the lazy red fox") == diff.similarity("the lazy red fox", "the quick brown fox"),
	diff.similarity("the quick brown fox", "the quick brown box") > 0.9,
}
return out
