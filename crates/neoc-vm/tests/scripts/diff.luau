--!strict
-- lib:diff as a script sees it: its answers (diff.json), then what it asserts.
local diff = require("lib:diff")
local out = {}

local function summary(changes: { lib_diff_Change })
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
out.unified = { patch.old_label, patch.new_label, hunks_of(patch) } :: { any }
out.unified_defaults = { diff.unified("a\n", "b\n").old_label, diff.unified("a\n", "b\n").new_label }
out.unified_same = { #diff.unified("a\nb\n", "a\nb\n").hunks, #diff.unified("", "").hunks }
local ten_old = "1\n2\n3\n4\n5\nX\n7\n8\n9\n10\n"
local ten_new = "1\n2\n3\n4\n5\nY\n7\n8\n9\n10\n"
out.context = {
  #diff.unified(ten_old, ten_new, { context = 0 }).hunks[1].lines,
  #diff.unified(ten_old, ten_new, { context = 3 }).hunks[1].lines,
}
out.zero_context = hunks_of(diff.unified("a\nb\n", "a\nb\nc\n", { context = 0 }))
out.line_values =
  { diff.unified("a\nb", "a\nB").hunks[1].lines[2].value, diff.unified("a\nb\n", "a\nB\n").hunks[1].lines[2].value }

local text = "--- x.txt\n+++ y.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"
local parsed = assert(diff.parse_unified(text))
out.parse = { parsed.old_label, parsed.new_label, hunks_of(parsed), parsed.hunks[1].lines[2].value } :: { any }
out.parse_no_newline = assert(
  diff.parse_unified(
    "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-x\n\\ No newline at end of file\n+y\n\\ No newline at end of file\n"
  )
).hunks[1].lines[1].value
out.parse_errors = {
  select(1, diff.parse_unified("just some text\nno headers\n")) == nil,
  select(1, diff.parse_unified("--- a\n+++ b\n")) == nil,
  select(1, diff.parse_unified("--- a\n+++ b\n@@ not a header @@\n")) == nil,
  select(1, diff.parse_unified("--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n")) == nil,
  select(1, diff.parse_unified("")) == nil,
}

out.apply = {
  (diff.apply("alpha\nbravo\ncharlie\n", "--- a\n+++ b\n@@ -1,3 +1,3 @@\n alpha\n-bravo\n+BRAVO\n charlie\n")),
  (
    diff.apply(
      "alpha\nbravo",
      "--- a\n+++ b\n@@ -1,2 +1,2 @@\n alpha\n-bravo\n\\ No newline at end of file\n+BRAVO\n\\ No newline at end of file\n"
    )
  ),
  (diff.apply("a\nb\n", "--- a\n+++ b\n@@ -2,0 +3,1 @@\n+c\n")),
  (diff.apply("a\nb\n", "--- a\n+++ b\n@@ -0,0 +1,1 @@\n+X\n")),
}
local mismatch, mismatch_err =
  diff.apply("alpha\nNOT\ncharlie\n", "--- a\n+++ b\n@@ -1,3 +1,3 @@\n alpha\n-bravo\n+BRAVO\n charlie\n")
out.apply_mismatch = { mismatch == nil, mismatch_err ~= nil }
local malformed, malformed_err = diff.apply("a\n", "this isn't a patch\n")
out.apply_malformed = { malformed == nil, malformed_err ~= nil }

out.similarity = {
  diff.similarity("anything", "anything"),
  diff.similarity("", ""),
  diff.similarity("", "anything"),
  diff.similarity("anything", ""),
  diff.similarity("the quick brown fox", "the lazy red fox")
    == diff.similarity("the lazy red fox", "the quick brown fox"),
  diff.similarity("the quick brown fox", "the quick brown box") > 0.9,
} :: { any }

-- what it asserts
do
  -- format writes the text, and parse_unified reads it back
  local patch = diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "old.txt", new_label = "new.txt" })
  local text = diff.format(patch)
  assert(text == "--- old.txt\n+++ new.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n")
  assert(diff.format((assert(diff.parse_unified(text)))) == text)
  assert(diff.format(diff.unified("alpha\nbravo", "alpha\nBRAVO")):find("\\ No newline at end of file", 1, true))
  assert(diff.format(diff.unified("a\nb\n", "a\nb\nc\n", { context = 0 })):find("@@ -2,0 +3,1 @@", 1, true))

  -- a patch a script built formats and applies
  local built: lib_diff_Patch = {
    old_label = "a",
    new_label = "b",
    hunks = {
      {
        old_start = 1,
        old_count = 1,
        new_start = 1,
        new_count = 1,
        lines = { { op = "delete", value = "x\n" }, { op = "insert", value = "y\n" } },
      },
    },
  }
  assert(assert(diff.apply("x\n", diff.format(built))) == "y\n")

  -- round trips through the text
  for _, case in
    {
      { "alpha\nbravo\n", "alpha\nBRAVO\n" },
      { "alpha\nbravo", "alpha\nBRAVO" },
      { "a", "a\nb" },
      { "", "x\n" },
      { "x\n", "" },
    }
  do
    assert(assert(diff.apply(case[1], diff.format(diff.unified(case[1], case[2])))) == case[2])
  end

  -- typed errors
  local function failure(patch_text: string, original: string?): lib_diff_ApplyError
    local value, err = diff.apply(original or "a\n", patch_text)
    assert(value == nil and err ~= nil)
    return err
  end
  local _, not_unified = diff.parse_unified("just some text\n")
  assert(
    not_unified ~= nil
      and not_unified.kind == "not_unified"
      and tostring(not_unified):find("lib:diff.parse_unified: not_unified:", 1, true)
  )
  local _, truncated = diff.parse_unified("--- a\n+++ b\n")
  assert(truncated ~= nil and truncated.kind == "truncated")
  local _, header = diff.parse_unified("--- a\n+++ b\n@@ not a header @@\n")
  assert(header ~= nil and header.kind == "invalid_hunk_header" and header.line == 3)
  local _, counts = diff.parse_unified("--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n")
  assert(
    counts ~= nil
      and counts.kind == "count_mismatch"
      and counts.field == "old_count"
      and counts.declared == 2
      and counts.observed == 1
  )
  local _, bad_line = diff.parse_unified("--- a\n+++ b\n@@ -1,1 +1,1 @@\n a\nstray")
  assert(bad_line ~= nil and bad_line.kind == "invalid_line" and bad_line.line == 5 and bad_line.text == "stray")

  local patch_text = diff.format(diff.unified("alpha\nbravo\ncharlie\n", "alpha\nBRAVO\ncharlie\n"))
  local mismatch = failure(patch_text, "alpha\nNOT\ncharlie\n")
  assert(mismatch.kind == "mismatch" and mismatch.hunk == 1 and mismatch.line == 2 and mismatch.change == "delete")
  assert(mismatch.expected == "bravo\n" and mismatch.found == "NOT\n" and mismatch.op == "lib:diff.apply")
  assert(failure(patch_text, "alpha\n").kind == "overrun")
  assert(failure("--- a\n+++ b\n@@ -9,1 +9,1 @@\n-x\n+y\n").kind == "starts_past_end")
  assert(
    failure("--- a\n+++ b\n@@ -3,1 +3,1 @@\n-c\n+C\n@@ -1,1 +1,1 @@\n-a\n+A\n", "a\nb\nc\n").kind == "out_of_order"
  )
  local malformed = failure("this isn't a patch\n")
  assert(malformed.kind == "malformed_patch" and malformed.reason:find("not a unified diff", 1, true))

  local _, reconstruct = diff.apply_changes("alpha\n", { { op = "equal", value = "wrong\n" } })
  assert(
    reconstruct ~= nil
      and reconstruct.kind == "does_not_reconstruct"
      and reconstruct.got == 6
      and reconstruct.expected == 6
  )

  -- unions and counts are refused as arguments
  -- deliberate refusal tests: an algorithm and an op that do not exist
  local ok, refused = pcall(diff.lines, "a", "b", { algorithm = "wagner" :: any })
  assert(not ok and tostring(refused):find("lib:diff.lines: argument #3 'opts'", 1, true))
  local ok2, refused2 = pcall(function()
    diff.apply_changes("x", { { op = "swap" :: any, value = "x" } })
  end)
  assert(not ok2 and tostring(refused2):find("argument #2 'changes'", 1, true))
  local ok3, refused3 = pcall(diff.unified, "a", "b", { context = -1 })
  assert(not ok3 and tostring(refused3):find("argument #3 'opts'", 1, true))
end

return out
