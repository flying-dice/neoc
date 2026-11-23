-- intentional: unified returns a plain Patch record and diff.format(patch) writes the text (no :format() method; it works on a patch a script built), the algorithm and change op are string unions and context an unsigned integer (refused as arguments), and parse_unified, apply and apply_changes fail with typed errors (not_unified, truncated, invalid_hunk_header, invalid_line, invalid_marker, count_mismatch; malformed_patch, starts_past_end, out_of_order, overrun, mismatch; does_not_reconstruct) that carry line and hunk numbers.
local diff = require("lib:diff")

-- format writes the text, and parse_unified reads it back
local patch = diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "old.txt", new_label = "new.txt" })
local text = diff.format(patch)
assert(text == "--- old.txt\n+++ new.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n")
assert(diff.format((assert(diff.parse_unified(text)))) == text)
assert(diff.format(diff.unified("alpha\nbravo", "alpha\nBRAVO")):find("\\ No newline at end of file", 1, true))
assert(diff.format(diff.unified("a\nb\n", "a\nb\nc\n", { context = 0 })):find("@@ -2,0 +3,1 @@", 1, true))

-- a patch a script built formats and applies
local built = { old_label = "a", new_label = "b", hunks = { { old_start = 1, old_count = 1, new_start = 1, new_count = 1, lines = { { op = "delete", value = "x\n" }, { op = "insert", value = "y\n" } } } } }
assert(assert(diff.apply("x\n", diff.format(built))) == "y\n")

-- round trips through the text
for _, case in { { "alpha\nbravo\n", "alpha\nBRAVO\n" }, { "alpha\nbravo", "alpha\nBRAVO" }, { "a", "a\nb" }, { "", "x\n" }, { "x\n", "" } } do
	assert(assert(diff.apply(case[1], diff.format(diff.unified(case[1], case[2])))) == case[2])
end

-- typed errors
local function failure(patch_text, original)
	local value, err = diff.apply(original or "a\n", patch_text)
	assert(value == nil)
	return err
end
local _, not_unified = diff.parse_unified("just some text\n")
assert(not_unified.kind == "not_unified" and tostring(not_unified):find("lib:diff.parse_unified: not_unified:", 1, true))
local _, truncated = diff.parse_unified("--- a\n+++ b\n")
assert(truncated.kind == "truncated")
local _, header = diff.parse_unified("--- a\n+++ b\n@@ not a header @@\n")
assert(header.kind == "invalid_hunk_header" and header.line == 3)
local _, counts = diff.parse_unified("--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n")
assert(counts.kind == "count_mismatch" and counts.field == "old_count" and counts.declared == 2 and counts.observed == 1)
local _, bad_line = diff.parse_unified("--- a\n+++ b\n@@ -1,1 +1,1 @@\n a\nstray")
assert(bad_line.kind == "invalid_line" and bad_line.line == 5 and bad_line.text == "stray")

local patch_text = diff.format(diff.unified("alpha\nbravo\ncharlie\n", "alpha\nBRAVO\ncharlie\n"))
local mismatch = failure(patch_text, "alpha\nNOT\ncharlie\n")
assert(mismatch.kind == "mismatch" and mismatch.hunk == 1 and mismatch.line == 2 and mismatch.change == "delete")
assert(mismatch.expected == "bravo\n" and mismatch.found == "NOT\n" and mismatch.op == "lib:diff.apply")
assert(failure(patch_text, "alpha\n").kind == "overrun")
assert(failure("--- a\n+++ b\n@@ -9,1 +9,1 @@\n-x\n+y\n").kind == "starts_past_end")
assert(failure("--- a\n+++ b\n@@ -3,1 +3,1 @@\n-c\n+C\n@@ -1,1 +1,1 @@\n-a\n+A\n", "a\nb\nc\n").kind == "out_of_order")
local malformed = failure("this isn't a patch\n")
assert(malformed.kind == "malformed_patch" and malformed.reason:find("not a unified diff", 1, true))

local _, reconstruct = diff.apply_changes("alpha\n", { { op = "equal", value = "wrong\n" } })
assert(reconstruct.kind == "does_not_reconstruct" and reconstruct.got == 6 and reconstruct.expected == 6)

-- unions and counts are refused as arguments
local ok, refused = pcall(diff.lines, "a", "b", { algorithm = "wagner" })
assert(not ok and tostring(refused):find("lib:diff.lines: argument #3 'opts'", 1, true))
local ok2, refused2 = pcall(diff.apply_changes, "x", { { op = "swap", value = "x" } })
assert(not ok2 and tostring(refused2):find("argument #2 'changes'", 1, true))
local ok3, refused3 = pcall(diff.unified, "a", "b", { context = -1 })
assert(not ok3 and tostring(refused3):find("argument #3 'opts'", 1, true))
