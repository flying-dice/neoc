local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local diff = require("lib:diff")

local function ops(changes)
    local out = {}
    for i, c in ipairs(changes) do
        out[i] = c.op
    end
    return out
end

describe("diff", function()

    -- ── lines ────────────────────────────────────────────────────────────────

    describe("lines", function()
        test("equal, delete, insert ordering", function()
            local cs = diff.lines("a\nb\nc\n", "a\nB\nc\n")
            local got = ops(cs)
            expect(table.concat(got, ",")).toBe("equal,delete,insert,equal")
        end)

        test("identical inputs are all equal", function()
            local cs = diff.lines("a\nb\n", "a\nb\n")
            for _, c in ipairs(cs) do
                expect(c.op).toBe("equal")
            end
        end)

        test("empty old → only inserts", function()
            local cs = diff.lines("", "x\ny\n")
            local saw_insert = false
            for _, c in ipairs(cs) do
                expect(c.op == "insert" or c.op == "equal").toBeTruthy()
                if c.op == "insert" then saw_insert = true end
            end
            expect(saw_insert).toBeTruthy()
        end)

        test("empty new → only deletes", function()
            local cs = diff.lines("x\ny\n", "")
            local saw_delete = false
            for _, c in ipairs(cs) do
                if c.op == "delete" then saw_delete = true end
            end
            expect(saw_delete).toBeTruthy()
        end)

        test("single-line files", function()
            local cs = diff.lines("only\n", "only\n")
            expect(#cs).toBe(1)
            expect(cs[1].op).toBe("equal")
        end)

        test("values preserve newlines", function()
            local cs = diff.lines("a\nb\n", "a\nc\n")
            for _, c in ipairs(cs) do
                expect(c.value:sub(-1)).toBe("\n")
            end
        end)

        test("myers algorithm produces a valid round-trip", function()
            local old = "1\n2\n3\n4\n5\n"
            local new = "1\n2\nX\n4\n5\n"
            local cs = diff.lines(old, new, { algorithm = "myers" })
            local roundtripped = diff.apply_changes(old, cs)
            expect(roundtripped).toBe(new)
        end)

        test("rejects unknown algorithm", function()
            local cs, err = diff.lines("a", "b", { algorithm = "wagner" })
            expect(cs).toBeNil()
            expect(err:find("unknown algorithm", 1, true) ~= nil).toBeTruthy()
        end)
    end)

    -- ── words ────────────────────────────────────────────────────────────────

    describe("words", function()
        test("basic word substitution", function()
            local cs = diff.words("the quick brown fox", "the slow brown fox")
            local saw_delete, saw_insert = false, false
            for _, c in ipairs(cs) do
                if c.op == "delete" and c.value:find("quick") then saw_delete = true end
                if c.op == "insert" and c.value:find("slow") then saw_insert = true end
            end
            expect(saw_delete).toBeTruthy()
            expect(saw_insert).toBeTruthy()
        end)

        test("apply_changes round-trip", function()
            local old, new = "alpha beta gamma", "alpha BETA gamma"
            local cs = diff.words(old, new)
            expect(diff.apply_changes(old, cs)).toBe(new)
        end)
    end)

    -- ── chars ────────────────────────────────────────────────────────────────

    describe("chars", function()
        test("char-level edit", function()
            local cs = diff.chars("kitten", "sitten")
            -- first char differs
            expect(cs[1].op).toBe("delete")
            expect(cs[1].value).toBe("k")
            expect(cs[2].op).toBe("insert")
            expect(cs[2].value).toBe("s")
        end)

        test("apply_changes round-trip", function()
            local old, new = "kitten", "sitting"
            local cs = diff.chars(old, new)
            expect(diff.apply_changes(old, cs)).toBe(new)
        end)

        test("very long inputs do not blow up", function()
            local a = string.rep("x", 10000)
            local b = a .. "y"
            local cs = diff.chars(a, b)
            expect(cs[#cs].op).toBe("insert")
            expect(cs[#cs].value).toBe("y")
        end)
    end)

    -- ── unified ──────────────────────────────────────────────────────────────

    describe("unified", function()
        test("returns hunks table with op/value entries", function()
            local r = diff.unified("a\nb\nc\n", "a\nB\nc\n")
            expect(#r.hunks).toBe(1)
            local h = r.hunks[1]
            expect(h.old_start).toBe(1)
            expect(h.new_start).toBe(1)
            local saw_delete, saw_insert = false, false
            for _, ln in ipairs(h.lines) do
                if ln.op == "delete" then saw_delete = true end
                if ln.op == "insert" then saw_insert = true end
            end
            expect(saw_delete).toBeTruthy()
            expect(saw_insert).toBeTruthy()
        end)

        test("format() emits unified diff string", function()
            local r = diff.unified("a\nb\nc\n", "a\nB\nc\n", {
                old_label = "old.txt",
                new_label = "new.txt",
            })
            local s = r:format()
            expect(s:sub(1, 11)).toBe("--- old.txt")
            expect(s:find("+++ new.txt", 1, true) ~= nil).toBeTruthy()
            expect(s:find("@@", 1, true) ~= nil).toBeTruthy()
            expect(s:find("-b", 1, true) ~= nil).toBeTruthy()
            expect(s:find("+B", 1, true) ~= nil).toBeTruthy()
        end)

        test("identical inputs produce zero hunks", function()
            local r = diff.unified("a\nb\n", "a\nb\n")
            expect(#r.hunks).toBe(0)
        end)

        test("custom context radius respected", function()
            local old = table.concat({
                "1","2","3","4","5","X","7","8","9","10"
            }, "\n") .. "\n"
            local new = table.concat({
                "1","2","3","4","5","Y","7","8","9","10"
            }, "\n") .. "\n"
            local r0 = diff.unified(old, new, { context = 0 })
            local r3 = diff.unified(old, new, { context = 3 })
            -- With context=0, no equal context lines flank the change.
            local equals_zero = 0
            for _, ln in ipairs(r0.hunks[1].lines) do
                if ln.op == "equal" then equals_zero += 1 end
            end
            expect(equals_zero).toBe(0)
            -- With context=3, both sides see 3 equal context lines.
            local equals_three = 0
            for _, ln in ipairs(r3.hunks[1].lines) do
                if ln.op == "equal" then equals_three += 1 end
            end
            expect(equals_three).toBe(6)
        end)
    end)

    -- ── parse_unified ────────────────────────────────────────────────────────

    describe("parse_unified", function()
        test("round-trip: parse(format(x)) ≡ x structurally", function()
            local r = diff.unified("a\nb\nc\n", "a\nB\nc\n", {
                old_label = "x.txt", new_label = "y.txt",
            })
            local s = r:format()
            local p = diff.parse_unified(s)
            expect(p).toBeTruthy()
            expect(p.old_label).toBe("x.txt")
            expect(p.new_label).toBe("y.txt")
            expect(#p.hunks).toBe(#r.hunks)
            expect(p:format()).toBe(s)
        end)

        test("not a unified diff", function()
            local r, err = diff.parse_unified("just some text\nno headers\n")
            expect(r).toBeNil()
            expect(err:find("not a unified diff", 1, true) ~= nil).toBeTruthy()
        end)

        test("truncated patch", function()
            local r, err = diff.parse_unified("--- a\n+++ b\n")
            expect(r).toBeNil()
            expect(err:find("truncated", 1, true) ~= nil).toBeTruthy()
        end)

        test("invalid hunk header reports line", function()
            local patch = "--- a\n+++ b\n@@ not a header @@\n"
            local r, err = diff.parse_unified(patch)
            expect(r).toBeNil()
            expect(err:find("line", 1, true) ~= nil).toBeTruthy()
        end)

        test("hunk count mismatch is rejected", function()
            -- Header claims old_count=2 but body only has 1 equal line.
            local patch = "--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n"
            local r, err = diff.parse_unified(patch)
            expect(r).toBeNil()
            expect(err:find("declares", 1, true) ~= nil).toBeTruthy()
        end)
    end)

    -- ── apply ────────────────────────────────────────────────────────────────

    describe("apply", function()
        test("round-trip: apply(old, unified(old, new):format()) == new", function()
            local old = "alpha\nbravo\ncharlie\n"
            local new = "alpha\nBRAVO\ncharlie\n"
            local patch = diff.unified(old, new):format()
            local out, err = diff.apply(old, patch)
            expect(err).toBeNil()
            expect(out).toBe(new)
        end)

        test("context mismatch errors with line number", function()
            local patch = diff.unified("a\nb\nc\n", "a\nB\nc\n"):format()
            local out, err = diff.apply("a\nNOT_B\nc\n", patch)
            expect(out).toBeNil()
            expect(err:find("mismatch", 1, true) ~= nil).toBeTruthy()
            expect(err:find("hunk", 1, true) ~= nil).toBeTruthy()
        end)

        test("malformed patch surfaces parse error", function()
            local out, err = diff.apply("a\n", "this isn't a patch\n")
            expect(out).toBeNil()
            expect(err:find("malformed patch", 1, true) ~= nil).toBeTruthy()
        end)

        test("no trailing newline preserved", function()
            local old = "alpha\nbravo"
            local new = "alpha\nBRAVO"
            local patch = diff.unified(old, new):format()
            expect(patch:find("No newline at end of file", 1, true) ~= nil).toBeTruthy()
            expect(diff.apply(old, patch)).toBe(new)
        end)

        test("zero-context round-trip (insert at end)", function()
            local old = "a\nb\n"
            local new = "a\nb\nZ\n"
            local patch = diff.unified(old, new, { context = 0 }):format()
            expect(diff.apply(old, patch)).toBe(new)
        end)

        test("zero-context round-trip (insert in middle)", function()
            local old = "a\nb\n"
            local new = "a\nINSERTED\nb\n"
            local patch = diff.unified(old, new, { context = 0 }):format()
            expect(diff.apply(old, patch)).toBe(new)
        end)

        test("multiple hunks apply in order", function()
            local old = table.concat({
                "A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P"
            }, "\n") .. "\n"
            local new = table.concat({
                "A","B","C","X","E","F","G","H","I","J","K","L","Y","N","O","P"
            }, "\n") .. "\n"
            local patch = diff.unified(old, new):format()
            expect(diff.apply(old, patch)).toBe(new)
        end)
    end)

    -- ── apply_changes ────────────────────────────────────────────────────────

    describe("apply_changes", function()
        test("round-trip: apply_changes(old, lines(old, new)) == new", function()
            local old = "alpha\nbravo\ncharlie\n"
            local new = "alpha\nBRAVO\ncharlie\n"
            local cs = diff.lines(old, new)
            expect(diff.apply_changes(old, cs)).toBe(new)
        end)

        test("misaligned change list rejected", function()
            local cs = {
                { op = "equal", value = "wrong\n" },
                { op = "insert", value = "anything\n" },
            }
            local out, err = diff.apply_changes("alpha\n", cs)
            expect(out).toBeNil()
            expect(err:find("reconstruct", 1, true) ~= nil).toBeTruthy()
        end)

        test("invalid op rejected", function()
            local cs = { { op = "swap", value = "x" } }
            local out, err = diff.apply_changes("x", cs)
            expect(out).toBeNil()
            expect(err:find("invalid op", 1, true) ~= nil).toBeTruthy()
        end)
    end)

    -- ── similarity ───────────────────────────────────────────────────────────

    describe("similarity", function()
        test("identical inputs return exactly 1.0", function()
            expect(diff.similarity("anything", "anything")).toBe(1.0)
            expect(diff.similarity("", "")).toBe(1.0)
        end)

        test("empty vs non-empty returns 0.0", function()
            expect(diff.similarity("", "anything")).toBe(0.0)
            expect(diff.similarity("anything", "")).toBe(0.0)
        end)

        test("symmetry holds", function()
            local a, b = "the quick brown fox", "the lazy red fox"
            local ab = diff.similarity(a, b)
            local ba = diff.similarity(b, a)
            expect(math.abs(ab - ba) < 1e-9).toBeTruthy()
        end)

        test("bounded in [0.0, 1.0]", function()
            local s = diff.similarity("hello", "world")
            expect(s >= 0.0 and s <= 1.0).toBeTruthy()
        end)

        test("highly similar inputs score near 1.0", function()
            local s = diff.similarity("the quick brown fox", "the quick brown box")
            expect(s > 0.9).toBeTruthy()
        end)
    end)

    -- ── algorithm independence ───────────────────────────────────────────────

    describe("algorithm independence", function()
        test("patience and myers both round-trip via apply", function()
            local old = "alpha\nbravo\ncharlie\ndelta\necho\n"
            local new = "alpha\nBRAVO\ncharlie\nDELTA\necho\n"
            for _, alg in ipairs({ "patience", "myers" }) do
                local patch = diff.unified(old, new, { algorithm = alg }):format()
                local out, err = diff.apply(old, patch)
                expect(err).toBeNil()
                expect(out).toBe(new)
            end
        end)
    end)

    -- ── edge cases ───────────────────────────────────────────────────────────

    describe("edge cases", function()
        test("empty old + empty new", function()
            local r = diff.unified("", "")
            expect(#r.hunks).toBe(0)
        end)

        test("only trailing newlines", function()
            local old, new = "\n\n", "\n\n\n"
            local patch = diff.unified(old, new):format()
            expect(diff.apply(old, patch)).toBe(new)
        end)

        test("very long line round-trips", function()
            local big = string.rep("x", 12000)
            local old = big .. "\n"
            local new = "y" .. big .. "\n"
            local patch = diff.unified(old, new):format()
            expect(diff.apply(old, patch)).toBe(new)
        end)
    end)
end)
