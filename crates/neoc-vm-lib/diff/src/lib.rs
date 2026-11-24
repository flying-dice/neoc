//! Text differencing: `lib:diff`.
//!
//! Backed by the `similar` crate for the patience / Myers algorithms. Formatting, parsing and applying the unified
//! diff text is hand-rolled, so the round trips are ours to keep:
//!
//! ```text
//! diff.apply(old, diff.format(diff.unified(old, new))) == new
//! diff.format(diff.parse_unified(text)) == text      (for text that `format` produced)
//! ```
//!
//! "No newline at end of file" follows GNU diff: a line that does not end in `\n` is written with the
//! `\ No newline at end of file` marker after it, and the parser takes the marker back out.
//!
//! The algorithm is a `"patience" | "myers"` union and the change `op` a `"equal" | "delete" | "insert"` union
//! (anything else is refused as an argument, in a change list too), `context` is an unsigned integer, `unified` returns
//! a plain typed `Patch` record (`diff.format(patch)` writes the text, and works on a patch a script built), a hunk
//! line's `value` is the line as it is in the text (with its `\n` when it has one), and `parse_unified`, `apply` and
//! `apply_changes` fail with typed errors that carry the line and hunk numbers.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use similar::Algorithm as SimilarAlgorithm;

const DEFAULT_CONTEXT: u32 = 3;
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file";

/// The most context lines a hunk can use. Larger values mean "the whole file" for any input a script can build, and
/// `similar` computes `radius * 2`, which overflows (a panic) for the huge ones a Lua number can hold (`1e300`).
const MAX_CONTEXT: u32 = 1 << 31;

/// A count that fits the declared integer: lines past 4 Gi do not exist in a text a script can hold.
fn wide(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[mlua_ir::module("lib:diff")]
pub mod diff {
    use mlua_ir::{LuaError, Options, Shape};
    use serde::{Deserialize, Serialize};
    use similar::{ChangeTag, TextDiff};

    use super::{DEFAULT_CONTEXT, MAX_CONTEXT, NO_NEWLINE_MARKER, SimilarAlgorithm, wide};

    /// What a change does to the old text.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Op {
        /// The text is in both.
        Equal,
        /// The text is in the old only.
        Delete,
        /// The text is in the new only.
        Insert,
    }

    /// How the differences are found.
    #[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Algorithm {
        /// Patience diff: anchors on lines that occur once, which reads best for code.
        #[default]
        Patience,
        /// Myers' shortest edit script.
        Myers,
    }

    impl From<Algorithm> for SimilarAlgorithm {
        fn from(a: Algorithm) -> Self {
            match a {
                Algorithm::Patience => SimilarAlgorithm::Patience,
                Algorithm::Myers => SimilarAlgorithm::Myers,
            }
        }
    }

    /// How `lines`, `words` and `chars` compare.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, Options)]
    pub struct ChangeOptions {
        /// The algorithm; patience when not given.
        pub algorithm: Algorithm,
    }

    /// How `unified` compares and labels.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct UnifiedOptions {
        /// The algorithm; patience when not given.
        pub algorithm: Algorithm,
        /// The equal lines kept around each change (3 when not given). A value beyond any text is the whole text.
        pub context: u32,
        /// The name of the old text in the `---` header.
        pub old_label: String,
        /// The name of the new text in the `+++` header.
        pub new_label: String,
    }

    impl Default for UnifiedOptions {
        fn default() -> Self {
            Self {
                algorithm: Algorithm::Patience,
                context: DEFAULT_CONTEXT,
                old_label: "a".to_owned(),
                new_label: "b".to_owned(),
            }
        }
    }

    /// One piece of a change list.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    pub struct Change {
        /// What it does.
        pub op: Op,
        /// The text, with its line break when it ends a line.
        pub value: String,
    }

    /// One line of a hunk.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    pub struct HunkLine {
        /// Context (`equal`), a removed line or an added line.
        pub op: Op,
        /// The line as it is in the text: with its `\n`, or without it for a last line that has none.
        pub value: String,
    }

    /// A run of changes with its context, and where it is in both texts.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    pub struct Hunk {
        /// The first line in the old text, counting from 1; when `old_count` is 0, the line before the gap (0 for
        /// before the first line), as GNU diff writes it.
        pub old_start: u32,
        /// The number of old lines the hunk covers.
        pub old_count: u32,
        /// The first line in the new text; the line before the gap when `new_count` is 0.
        pub new_start: u32,
        /// The number of new lines the hunk covers.
        pub new_count: u32,
        /// The lines in order.
        pub lines: Vec<HunkLine>,
    }

    /// A unified diff: the two labels and the hunks. No hunks means the texts are the same.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    pub struct Patch {
        /// The name of the old text.
        pub old_label: String,
        /// The name of the new text.
        pub new_label: String,
        /// The hunks in order.
        pub hunks: Vec<Hunk>,
    }

    /// Which count of a hunk header disagrees with its body.
    #[derive(Debug, Clone, Copy, Serialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Count {
        /// The number of old lines (context and removed).
        OldCount,
        /// The number of new lines (context and added).
        NewCount,
    }

    /// Why text is not a unified diff.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ParseError {
        /// The text has no `---`, `+++` or `@@` line at its start.
        #[error("not a unified diff")]
        NotUnified,
        /// There are file headers but no hunk after them (or the hunks are empty).
        #[error("truncated patch: no hunks")]
        Truncated,
        /// A line starting with `@@` is not `@@ -a,b +c,d @@`.
        #[error("invalid hunk header at line {line}: {text:?}")]
        InvalidHunkHeader {
            /// The line, counting from 1.
            line: u32,
            /// The line.
            text: String,
        },
        /// A hunk body line that starts with none of ` `, `-`, `+` and `\`.
        #[error("invalid line {line} in hunk: {text:?}")]
        InvalidLine {
            /// The line, counting from 1.
            line: u32,
            /// The line.
            text: String,
        },
        /// A `\ No newline at end of file` marker with no line before it.
        #[error("invalid marker at line {line}: no preceding line")]
        InvalidMarker {
            /// The line, counting from 1.
            line: u32,
        },
        /// A hunk header counts lines its body does not have.
        #[error("hunk header at line {line} declares {field:?}={declared} but body has {observed}")]
        CountMismatch {
            /// The line of the header, counting from 1.
            line: u32,
            /// Which count.
            field: Count,
            /// What the header says.
            declared: u32,
            /// What the body has.
            observed: u32,
        },
    }

    /// Why a patch does not apply to a text.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ApplyError {
        /// The patch is not a unified diff (what `parse_unified` would say).
        #[error("malformed patch: {reason}")]
        MalformedPatch {
            /// `parse_unified`'s explanation.
            reason: String,
        },
        /// A hunk starts after the end of the text.
        #[error("hunk {hunk} starts past end of original (line {line} of {total} total)")]
        StartsPastEnd {
            /// The hunk, counting from 1.
            hunk: u32,
            /// The line it starts at.
            line: u32,
            /// The number of lines in the text.
            total: u32,
        },
        /// A hunk starts before the end of the previous one.
        #[error("hunk {hunk} starts at line {line} but the previous hunk reached line {cursor}")]
        OutOfOrder {
            /// The hunk, counting from 1.
            hunk: u32,
            /// The line it starts at.
            line: u32,
            /// The line the previous hunk reached.
            cursor: u32,
        },
        /// A context or removed line is past the end of the text.
        #[error("hunk {hunk} {change:?} overruns end of original at line {line}")]
        Overrun {
            /// The hunk, counting from 1.
            hunk: u32,
            /// The line of the text.
            line: u32,
            /// `equal` for context, `delete` for a removed line.
            change: Op,
        },
        /// A context or removed line is not the line the text has.
        #[error("hunk {hunk} {change:?} mismatch at line {line}: expected {expected:?}, found {found:?}")]
        Mismatch {
            /// The hunk, counting from 1.
            hunk: u32,
            /// The line of the text.
            line: u32,
            /// `equal` for context, `delete` for a removed line.
            change: Op,
            /// The line the patch expects.
            expected: String,
            /// The line the text has.
            found: String,
        },
    }

    /// Why a change list cannot be applied.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ChangesError {
        /// The `equal` and `delete` pieces do not add up to the original text.
        #[error("change list does not reconstruct original (got {got} bytes, expected {expected})")]
        DoesNotReconstruct {
            /// The bytes the `equal` and `delete` pieces make.
            got: u64,
            /// The bytes of the original.
            expected: u64,
        },
    }

    // Change lists.

    fn compute(text: TextDiff<'_, '_, str>) -> Vec<Change> {
        text.iter_all_changes().map(|c| Change { op: op_of(c.tag()), value: c.value().to_owned() }).collect()
    }

    fn op_of(tag: ChangeTag) -> Op {
        match tag {
            ChangeTag::Equal => Op::Equal,
            ChangeTag::Delete => Op::Delete,
            ChangeTag::Insert => Op::Insert,
        }
    }

    /// The line-level change list between `old` and `new`.
    ///
    /// ```luau
    /// local changes = diff.lines("a\nb\nc\n", "a\nB\nc\n")
    /// assert(#changes == 4 and changes[2].op == "delete" and changes[3].value == "B\n")
    /// ```
    pub fn lines(old: String, new: String, opts: ChangeOptions) -> Vec<Change> {
        compute(TextDiff::configure().algorithm(opts.algorithm.into()).diff_lines(&old, &new))
    }

    /// The word-level change list, tokenising on white space.
    pub fn words(old: String, new: String, opts: ChangeOptions) -> Vec<Change> {
        compute(TextDiff::configure().algorithm(opts.algorithm.into()).diff_words(&old, &new))
    }

    /// The character-level change list, over Unicode codepoints.
    pub fn chars(old: String, new: String, opts: ChangeOptions) -> Vec<Change> {
        compute(TextDiff::configure().algorithm(opts.algorithm.into()).diff_chars(&old, &new))
    }

    // Unified diffs.

    /// The `start` a hunk header carries: GNU's convention is that an empty range names the line BEFORE the gap,
    /// and a non-empty one its first line (counting from 1). [`decode_anchor`] is the inverse.
    fn encode_anchor(start_zero_indexed: usize, count: usize) -> usize {
        if count == 0 { start_zero_indexed } else { start_zero_indexed + 1 }
    }

    fn decode_anchor(start: usize, count: usize) -> usize {
        if count == 0 { start } else { start.saturating_sub(1) }
    }

    fn build_hunks(alg: SimilarAlgorithm, old: &str, new: &str, context: usize) -> Vec<Hunk> {
        let text = TextDiff::configure().algorithm(alg).diff_lines(old, new);
        let mut hunks = Vec::new();
        for group in text.grouped_ops(context) {
            let (Some(first), Some(last)) = (group.first(), group.last()) else {
                continue;
            };
            let (old_range, new_range) = (
                first.old_range().start..last.old_range().end,
                first.new_range().start..last.new_range().end,
            );
            let mut lines = Vec::new();
            for op in &group {
                for change in text.iter_changes(op) {
                    lines.push(HunkLine { op: op_of(change.tag()), value: change.value().to_owned() });
                }
            }
            let (old_count, new_count) = (old_range.len(), new_range.len());
            hunks.push(Hunk {
                old_start: wide(encode_anchor(old_range.start, old_count)),
                old_count: wide(old_count),
                new_start: wide(encode_anchor(new_range.start, new_count)),
                new_count: wide(new_count),
                lines,
            });
        }
        hunks
    }

    /// Compute a unified diff of `old` and `new`, by lines.
    ///
    /// ```luau
    /// local patch = diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "old.txt", new_label = "new.txt" })
    /// assert(#patch.hunks == 1 and patch.hunks[1].old_start == 1)
    /// assert(diff.format(patch):sub(1, 11) == "--- old.txt")
    /// ```
    pub fn unified(old: String, new: String, opts: UnifiedOptions) -> Patch {
        let context = opts.context.min(MAX_CONTEXT) as usize;
        Patch {
            old_label: opts.old_label,
            new_label: opts.new_label,
            hunks: build_hunks(opts.algorithm.into(), &old, &new, context),
        }
    }

    /// Write a patch as unified diff text, the form `patch` and `git apply` read. A line without its `\n` gets the
    /// `\ No newline at end of file` marker.
    ///
    /// ```luau
    /// local patch = diff.unified("alpha\nbravo\n", "alpha\nBRAVO\n")
    /// assert(diff.apply("alpha\nbravo\n", diff.format(patch)) == "alpha\nBRAVO\n")
    /// ```
    pub fn format(patch: Patch) -> String {
        let mut out = format!("--- {}\n+++ {}\n", patch.old_label, patch.new_label);
        for hunk in &patch.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ));
            for line in &hunk.lines {
                out.push(match line.op {
                    Op::Equal => ' ',
                    Op::Delete => '-',
                    Op::Insert => '+',
                });
                out.push_str(line.value.strip_suffix('\n').unwrap_or(&line.value));
                out.push('\n');
                if !line.value.ends_with('\n') {
                    out.push_str(NO_NEWLINE_MARKER);
                    out.push('\n');
                }
            }
        }
        out
    }

    // Parsing.

    type Lines<'a> = std::iter::Peekable<std::iter::Enumerate<std::str::Split<'a, char>>>;

    struct Header {
        old_start: u32,
        old_count: u32,
        new_start: u32,
        new_count: u32,
    }

    fn parse_range(part: &str) -> Option<(u32, u32)> {
        let (start, count) = part.split_once(',').unwrap_or((part, "1"));
        Some((start.parse().ok()?, count.parse().ok()?))
    }

    /// `@@ -A,B +C,D @@` (B or D left out means 1).
    fn parse_header(line: &str, number: u32) -> Result<Header, ParseError> {
        let bad = || ParseError::InvalidHunkHeader { line: number, text: line.to_owned() };
        let body = line.strip_prefix("@@").and_then(|s| s.trim_start().strip_prefix('-')).ok_or_else(bad)?;
        let (old_part, rest) = body.split_once(" +").ok_or_else(bad)?;
        let new_part = rest.split_once("@@").map(|(a, _)| a.trim()).ok_or_else(bad)?;
        let (old_start, old_count) = parse_range(old_part.trim()).ok_or_else(bad)?;
        let (new_start, new_count) = parse_range(new_part.trim()).ok_or_else(bad)?;
        Ok(Header { old_start, old_count, new_start, new_count })
    }

    fn parse_body(iter: &mut Lines<'_>) -> Result<(Vec<HunkLine>, u32, u32), ParseError> {
        let mut lines: Vec<HunkLine> = Vec::new();
        let (mut old, mut new) = (0u32, 0u32);
        while let Some(&(at, text)) = iter.peek() {
            if text.starts_with("@@") {
                break;
            }
            if text.is_empty() {
                iter.next();
                if iter.peek().is_some() {
                    // An empty line inside a hunk is an empty context line.
                    lines.push(HunkLine { op: Op::Equal, value: "\n".to_owned() });
                    old += 1;
                    new += 1;
                    continue;
                }
                break; // the end of the patch: its last line break leaves an empty piece
            }
            let number = wide(at + 1);
            let rest = &text[1..];
            match text.as_bytes()[0] {
                b' ' => {
                    lines.push(HunkLine { op: Op::Equal, value: format!("{rest}\n") });
                    old += 1;
                    new += 1;
                }
                b'-' => {
                    lines.push(HunkLine { op: Op::Delete, value: format!("{rest}\n") });
                    old += 1;
                }
                b'+' => {
                    lines.push(HunkLine { op: Op::Insert, value: format!("{rest}\n") });
                    new += 1;
                }
                b'\\' => {
                    let Some(last) = lines.last_mut() else {
                        return Err(ParseError::InvalidMarker { line: number });
                    };
                    // `\ No newline at end of file`: the line before it has no line break.
                    last.value.pop();
                }
                _ => return Err(ParseError::InvalidLine { line: number, text: text.to_owned() }),
            }
            iter.next();
        }
        Ok((lines, old, new))
    }

    fn parse(patch: &str) -> Result<Patch, ParseError> {
        if patch.is_empty() {
            return Err(ParseError::NotUnified);
        }
        let mut iter: Lines<'_> = patch.split('\n').enumerate().peekable();
        let (mut old_label, mut new_label) = ("a".to_owned(), "b".to_owned());
        let (mut saw_headers, mut found_hunk) = (false, false);
        while let Some(&(_, line)) = iter.peek() {
            if let Some(rest) = line.strip_prefix("--- ") {
                old_label = rest.to_owned();
                saw_headers = true;
            } else if let Some(rest) = line.strip_prefix("+++ ") {
                new_label = rest.to_owned();
                saw_headers = true;
            } else if line.starts_with("@@") {
                found_hunk = true;
                break;
            } else if !line.is_empty() {
                break;
            }
            iter.next();
        }
        if !found_hunk {
            return Err(if saw_headers { ParseError::Truncated } else { ParseError::NotUnified });
        }

        let mut hunks = Vec::new();
        while let Some(&(at, line)) = iter.peek() {
            let number = wide(at + 1);
            if line.starts_with("@@") {
                iter.next();
                let header = parse_header(line, number)?;
                let (lines, old, new) = parse_body(&mut iter)?;
                for (field, declared, observed) in
                    [(Count::OldCount, header.old_count, old), (Count::NewCount, header.new_count, new)]
                {
                    if declared != observed {
                        return Err(ParseError::CountMismatch { line: number, field, declared, observed });
                    }
                }
                hunks.push(Hunk {
                    old_start: header.old_start,
                    old_count: header.old_count,
                    new_start: header.new_start,
                    new_count: header.new_count,
                    lines,
                });
            } else {
                // Only empty lines can be left here: a hunk body takes every other line.
                iter.next();
            }
        }
        if hunks.is_empty() {
            return Err(ParseError::Truncated);
        }
        Ok(Patch { old_label, new_label, hunks })
    }

    /// Parse unified diff text into the same `Patch` record `unified` returns.
    ///
    /// ```luau
    /// local patch = assert(diff.parse_unified("--- a\n+++ b\n@@ -1,1 +1,1 @@\n-x\n+y\n"))
    /// assert(patch.hunks[1].lines[2].value == "y\n")
    /// local _, err = diff.parse_unified("just some text\n")
    /// assert(err ~= nil and err.kind == "not_unified")
    /// ```
    pub fn parse_unified(patch: String) -> Result<Patch, ParseError> {
        parse(&patch)
    }

    // Applying.

    fn verify(original: &[&str], cursor: usize, line: &HunkLine, hunk: u32) -> Result<(), ApplyError> {
        let at = wide(cursor + 1);
        let Some(&found) = original.get(cursor) else {
            return Err(ApplyError::Overrun { hunk, line: at, change: line.op });
        };
        if found != line.value {
            return Err(ApplyError::Mismatch {
                hunk,
                line: at,
                change: line.op,
                expected: line.value.clone(),
                found: found.to_owned(),
            });
        }
        Ok(())
    }

    fn apply_hunks(original: &[&str], hunks: &[Hunk]) -> Result<String, ApplyError> {
        let mut out: Vec<&str> = Vec::new();
        let mut cursor = 0usize;
        for (index, hunk) in hunks.iter().enumerate() {
            let number = wide(index + 1);
            let anchor = decode_anchor(hunk.old_start as usize, hunk.old_count as usize);
            if anchor > original.len() {
                return Err(ApplyError::StartsPastEnd {
                    hunk: number,
                    line: hunk.old_start,
                    total: wide(original.len()),
                });
            }
            if anchor < cursor {
                return Err(ApplyError::OutOfOrder {
                    hunk: number,
                    line: hunk.old_start,
                    cursor: wide(cursor + 1),
                });
            }
            out.extend_from_slice(&original[cursor..anchor]);
            cursor = anchor;
            for line in &hunk.lines {
                match line.op {
                    Op::Equal => {
                        verify(original, cursor, line, number)?;
                        out.push(&line.value);
                        cursor += 1;
                    }
                    Op::Delete => {
                        verify(original, cursor, line, number)?;
                        cursor += 1;
                    }
                    Op::Insert => out.push(&line.value),
                }
            }
        }
        out.extend_from_slice(&original[cursor..]);

        // Only the very last line may lack its line break.
        let mut text = String::new();
        for (i, piece) in out.iter().enumerate() {
            text.push_str(piece);
            if i + 1 < out.len() && !piece.ends_with('\n') {
                text.push('\n');
            }
        }
        Ok(text)
    }

    /// Apply unified diff text to `original`. The context and removed lines must match `original` exactly.
    ///
    /// ```luau
    /// local old, new = "alpha\nbravo\n", "alpha\nBRAVO\n"
    /// local patched = assert(diff.apply(old, diff.format(diff.unified(old, new))))
    /// assert(patched == new)
    /// local _, err = diff.apply("alpha\nOTHER\n", diff.format(diff.unified(old, new)))
    /// assert(err ~= nil and err.kind == "mismatch" and err.hunk == 1)
    /// ```
    pub fn apply(original: String, patch: String) -> Result<String, ApplyError> {
        let parsed = parse(&patch).map_err(|e| ApplyError::MalformedPatch { reason: e.to_string() })?;
        let lines: Vec<&str> = original.split_inclusive('\n').collect();
        apply_hunks(&lines, &parsed.hunks)
    }

    /// Apply a change list: `original` is checked against its `equal` and `delete` pieces first, and the result is
    /// its `equal` and `insert` pieces.
    ///
    /// ```luau
    /// local old, new = "alpha beta", "alpha BETA"
    /// assert(assert(diff.apply_changes(old, diff.words(old, new))) == new)
    /// ```
    pub fn apply_changes(original: String, changes: Vec<Change>) -> Result<String, ChangesError> {
        let kept: String = changes.iter().filter(|c| c.op != Op::Insert).map(|c| c.value.as_str()).collect();
        if kept != original {
            return Err(ChangesError::DoesNotReconstruct {
                got: kept.len() as u64,
                expected: original.len() as u64,
            });
        }
        Ok(changes.iter().filter(|c| c.op != Op::Delete).map(|c| c.value.as_str()).collect())
    }

    /// Edit-ratio similarity in `[0, 1]` from a character-level patience diff: 1 for equal texts (two empty ones
    /// too), 0 when only one is empty.
    ///
    /// ```luau
    /// assert(diff.similarity("abc", "abc") == 1)
    /// assert(diff.similarity("the quick brown fox", "the quick brown box") > 0.9)
    /// ```
    pub fn similarity(a: String, b: String) -> f64 {
        if a == b {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        f64::from(TextDiff::configure().algorithm(SimilarAlgorithm::Patience).diff_chars(&a, &b).ratio())
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:diff")"#).eval().unwrap();
        lua.globals().set("diff", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    /// `old` patched with `unified(old, new, opts)` through the text form.
    fn round_trip(old: &str, new: &str, opts: &str) -> String {
        eval(&format!(
            r#"
            local old, new = "{old}", "{new}"
            return assert(diff.apply(old, diff.format(diff.unified(old, new, {opts}))))
        "#
        ))
    }

    #[test]
    fn lines_emit_equal_delete_insert_in_order() {
        let ops: String = eval(
            r#"
            local out = {}
            for i, c in diff.lines("a\nb\nc\n", "a\nB\nc\n") do out[i] = c.op end
            return table.concat(out, ",")
        "#,
        );
        assert_eq!(ops, "equal,delete,insert,equal");
    }

    #[test]
    fn identical_lines_are_all_equal() {
        assert!(eval::<bool>(
            r#"
            for _, c in diff.lines("a\nb\n", "a\nb\n") do if c.op ~= "equal" then return false end end
            return true
        "#
        ));
        assert_eq!(eval::<usize>(r#"return #diff.lines("only\n", "only\n")"#), 1);
    }

    #[test]
    fn empty_sides_give_only_inserts_or_deletes() {
        assert!(eval::<bool>(
            r#"
            local saw = false
            for _, c in diff.lines("", "a\nb\n") do
                if c.op == "insert" then saw = true elseif c.op ~= "equal" then return false end
            end
            return saw
        "#
        ));
        assert!(eval::<bool>(
            r#"
            local saw = false
            for _, c in diff.lines("a\nb\n", "") do if c.op == "delete" then saw = true end end
            return saw
        "#
        ));
    }

    #[test]
    fn values_keep_their_line_breaks() {
        assert!(eval::<bool>(
            r#"
            for _, c in diff.lines("a\nb\n", "a\nc\n") do if c.value:sub(-1) ~= "\n" then return false end end
            return true
        "#
        ));
    }

    #[test]
    fn the_algorithm_is_a_union() {
        assert_eq!(
            eval::<String>(
                r#"
                local old, new = "1\n2\n3\n4\n5\n", "1\n2\nX\n4\n5\n"
                return assert(diff.apply_changes(old, diff.lines(old, new, { algorithm = "myers" })))
            "#
            ),
            "1\n2\nX\n4\n5\n"
        );
        let err = refused(r#"return diff.lines("a", "b", { algorithm = "wagner" })"#);
        assert!(err.contains("lib:diff.lines: argument #3 'opts'") && err.contains("wagner"), "{err}");
    }

    #[test]
    fn words_and_chars() {
        let (deleted, inserted): (bool, bool) = eval(
            r#"
            local d, i = false, false
            for _, c in diff.words("the quick brown fox", "the slow brown fox") do
                if c.op == "delete" and c.value:find("quick") then d = true end
                if c.op == "insert" and c.value:find("slow") then i = true end
            end
            return d, i
        "#,
        );
        assert!(deleted && inserted);
        let (op1, v1, op2, v2): (String, String, String, String) = eval(
            r#"local cs = diff.chars("kitten", "sitten") return cs[1].op, cs[1].value, cs[2].op, cs[2].value"#,
        );
        assert_eq!((op1.as_str(), v1.as_str(), op2.as_str(), v2.as_str()), ("delete", "k", "insert", "s"));
        for (old, new) in [("alpha beta gamma", "alpha BETA gamma"), ("kitten", "sitting")] {
            for f in ["words", "chars"] {
                let applied: String = eval(&format!(
                    r#"local cs = diff.{f}("{old}", "{new}") return assert(diff.apply_changes("{old}", cs))"#
                ));
                assert_eq!(applied, new);
            }
        }
    }

    #[test]
    fn a_very_long_line_does_not_blow_up() {
        let (op, value): (String, String) = eval(
            r#"
            local a = string.rep("x", 20000)
            local cs = diff.chars(a, a .. "y")
            return cs[#cs].op, cs[#cs].value
        "#,
        );
        assert_eq!((op.as_str(), value.as_str()), ("insert", "y"));
    }

    #[test]
    fn unified_describes_the_hunk() {
        let (hunks, old_start, new_start, old_count, lines): (usize, u32, u32, u32, usize) = eval(
            r#"
            local p = diff.unified("a\nb\nc\n", "a\nB\nc\n")
            local h = p.hunks[1]
            return #p.hunks, h.old_start, h.new_start, h.old_count, #h.lines
        "#,
        );
        assert_eq!((hunks, old_start, new_start, old_count, lines), (1, 1, 1, 3, 4));
        assert_eq!(eval::<usize>(r#"return #diff.unified("a\nb\n", "a\nb\n").hunks"#), 0);
        assert_eq!(eval::<usize>(r#"return #diff.unified("", "").hunks"#), 0);
    }

    #[test]
    fn format_writes_the_unified_text() {
        let text: String = eval(
            r#"return diff.format(diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "old.txt", new_label = "new.txt" }))"#,
        );
        assert_eq!(text, "--- old.txt\n+++ new.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
    }

    #[test]
    fn labels_default_to_a_and_b() {
        let (old, new): (String, String) =
            eval(r#"local p = diff.unified("a\n", "b\n") return p.old_label, p.new_label"#);
        assert_eq!((old.as_str(), new.as_str()), ("a", "b"));
    }

    #[test]
    fn context_radius_is_respected() {
        let (zero, three): (usize, usize) = eval(
            r#"
            local old = "1\n2\n3\n4\n5\nX\n7\n8\n9\n10\n"
            local new = "1\n2\n3\n4\n5\nY\n7\n8\n9\n10\n"
            local function equals(p)
                local n = 0
                for _, l in p.hunks[1].lines do if l.op == "equal" then n += 1 end end
                return n
            end
            return equals(diff.unified(old, new, { context = 0 })), equals(diff.unified(old, new, { context = 3 }))
        "#,
        );
        assert_eq!((zero, three), (0, 6));
    }

    #[test]
    fn a_huge_context_is_clamped_not_an_overflow_panic() {
        for src in ["4294967295", "2^31", "2^31 + 1"] {
            let hunks: usize = eval(&format!(
                r#"return #diff.unified("a\nb\nc\n", "a\nB\nc\n", {{ context = {src} }}).hunks"#
            ));
            assert_eq!(hunks, 1, "context = {src}");
        }
        for src in ["-1", "1e300", "1.5", "2^32"] {
            let err = refused(&format!(r#"return diff.unified("a", "b", {{ context = {src} }})"#));
            assert!(err.contains("argument #3 'opts'"), "context = {src}: {err}");
        }
    }

    #[test]
    fn a_hunk_line_value_carries_its_line_break() {
        let (a, b): (String, String) = eval(
            r#"
            local p = diff.unified("a\nb", "a\nB")
            local h = p.hunks[1]
            return h.lines[2].value, h.lines[3].value
        "#,
        );
        assert_eq!((a.as_str(), b.as_str()), ("b", "B"));
        assert_eq!(
            eval::<String>(r#"return diff.unified("a\nb\n", "a\nB\n").hunks[1].lines[2].value"#),
            "b\n"
        );
    }

    #[test]
    fn parse_and_format_round_trip() {
        let (same, old, new, hunks): (bool, String, String, usize) = eval(
            r#"
            local text = diff.format(diff.unified("a\nb\nc\n", "a\nB\nc\n", { old_label = "x.txt", new_label = "y.txt" }))
            local p = assert(diff.parse_unified(text))
            return diff.format(p) == text, p.old_label, p.new_label, #p.hunks
        "#,
        );
        assert!(same);
        assert_eq!((old.as_str(), new.as_str(), hunks), ("x.txt", "y.txt", 1));
    }

    #[test]
    fn parse_equals_what_unified_computed() {
        assert!(eval::<bool>(
            r#"
            local made = diff.unified("a\nb\nc\nd", "a\nB\nc\nd\ne")
            local parsed = assert(diff.parse_unified(diff.format(made)))
            return diff.format(parsed) == diff.format(made)
                and parsed.hunks[1].lines[#parsed.hunks[1].lines].value == made.hunks[1].lines[#made.hunks[1].lines].value
        "#
        ));
    }

    fn parse_error(patch: &str) -> (String, String) {
        eval(&format!(r#"local _, e = diff.parse_unified("{patch}") return e.kind, tostring(e)"#))
    }

    #[test]
    fn parse_errors_are_typed() {
        let (kind, text) = parse_error("just some text\\nno headers\\n");
        assert_eq!(kind, "not_unified");
        assert!(text.starts_with("lib:diff.parse_unified: not_unified:"), "{text}");
        assert_eq!(parse_error("--- a\\n+++ b\\n").0, "truncated");
        assert_eq!(parse_error("").0, "not_unified");
        assert_eq!(parse_error("--- a\\n+++ b\\n@@ not a header @@\\n").0, "invalid_hunk_header");
        assert_eq!(parse_error("--- a\\n+++ b\\n@@@ -1 +1 @@\\n").0, "invalid_hunk_header");
        assert_eq!(parse_error("--- a\\n+++ b\\n@@ -1,1 +1,1 @@\\n?x\\n").0, "invalid_line");
        assert_eq!(
            parse_error("--- a\\n+++ b\\n@@ -1,0 +1,0 @@\\n\\\\ No newline at end of file\\n").0,
            "invalid_marker"
        );
    }

    #[test]
    fn a_count_mismatch_says_which_and_by_how_much() {
        let (kind, line, field, declared, observed): (String, u32, String, u32, u32) = eval(
            r#"
            local _, e = diff.parse_unified("--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n+b\n")
            return e.kind, e.line, e.field, e.declared, e.observed
        "#,
        );
        assert_eq!(
            (kind.as_str(), line, field.as_str(), declared, observed),
            ("count_mismatch", 3, "old_count", 2, 1)
        );
    }

    #[test]
    fn a_line_that_is_no_hunk_line_is_a_typed_error_with_its_place() {
        let (kind, line, text): (String, u32, String) = eval(
            r#"
            local _, e = diff.parse_unified("--- a\n+++ b\n@@ -1,1 +1,1 @@\n a\nstray")
            return e.kind, e.line, e.text
        "#,
        );
        assert_eq!((kind.as_str(), line, text.as_str()), ("invalid_line", 5, "stray"));
    }

    #[test]
    fn apply_round_trips() {
        assert_eq!(
            round_trip("alpha\\nbravo\\ncharlie\\n", "alpha\\nBRAVO\\ncharlie\\n", "nil"),
            "alpha\nBRAVO\ncharlie\n"
        );
        assert_eq!(round_trip("alpha\\nbravo", "alpha\\nBRAVO", "nil"), "alpha\nBRAVO");
        assert_eq!(round_trip("\\n\\n", "\\n\\n\\n", "nil"), "\n\n\n");
        assert_eq!(round_trip("a", "a\\nb", "nil"), "a\nb");
        assert_eq!(round_trip("a\\nb", "a", "nil"), "a");
        assert_eq!(round_trip("", "x\\n", "nil"), "x\n");
        assert_eq!(round_trip("x\\n", "", "nil"), "");
    }

    #[test]
    fn a_missing_final_newline_gets_the_marker() {
        let text: String = eval(r#"return diff.format(diff.unified("alpha\nbravo", "alpha\nBRAVO"))"#);
        assert!(text.contains("\\ No newline at end of file"), "{text}");
        assert!(eval::<bool>(
            r#"
            local text = diff.format(diff.unified("alpha\nbravo", "alpha\nBRAVO"))
            local p = assert(diff.parse_unified(text))
            return p.hunks[1].lines[2].value == "bravo" and p.hunks[1].lines[3].value == "BRAVO"
        "#
        ));
    }

    #[test]
    fn both_algorithms_round_trip() {
        for alg in ["patience", "myers"] {
            assert_eq!(
                round_trip(
                    "alpha\\nbravo\\ncharlie\\ndelta\\necho\\n",
                    "alpha\\nBRAVO\\ncharlie\\nDELTA\\necho\\n",
                    &format!(r#"{{ algorithm = "{alg}" }}"#)
                ),
                "alpha\nBRAVO\ncharlie\nDELTA\necho\n"
            );
        }
    }

    #[test]
    fn zero_context_pure_insert_hunks() {
        // The GNU "line before the gap" anchor: at the start, in the middle and at the end.
        let ctx = "{ context = 0 }";
        assert_eq!(round_trip("a\\nb\\n", "X\\na\\nb\\n", ctx), "X\na\nb\n");
        assert_eq!(round_trip("a\\nb\\n", "a\\nINSERTED\\nb\\n", ctx), "a\nINSERTED\nb\n");
        assert_eq!(round_trip("a\\nb\\n", "a\\nb\\nZ\\n", ctx), "a\nb\nZ\n");
        let header: String = eval(
            r#"return diff.format(diff.unified("a\nb\n", "a\nb\nc\n", { context = 0 })):match("@@[^\n]*")"#,
        );
        assert_eq!(header, "@@ -2,0 +3,1 @@");
    }

    #[test]
    fn multiple_hunks_apply_in_order() {
        let old = "A\\nB\\nC\\nD\\nE\\nF\\nG\\nH\\nI\\nJ\\nK\\nL\\nM\\nN\\nO\\nP\\n";
        let new = "A\\nB\\nC\\nX\\nE\\nF\\nG\\nH\\nI\\nJ\\nK\\nL\\nY\\nN\\nO\\nP\\n";
        assert_eq!(eval::<usize>(&format!(r#"return #diff.unified("{old}", "{new}").hunks"#)), 2);
        assert_eq!(round_trip(old, new, "nil"), new.replace("\\n", "\n"));
    }

    #[test]
    fn a_very_long_line_round_trips() {
        let applied: bool = eval(
            r#"
            local big = string.rep("x", 12000)
            local old, new = big .. "\n", "y" .. big .. "\n"
            return assert(diff.apply(old, diff.format(diff.unified(old, new)))) == new
        "#,
        );
        assert!(applied);
    }

    #[test]
    fn apply_context_mismatch_is_a_typed_error() {
        let (kind, hunk, line, op, expected, found, text): (
            String,
            u32,
            u32,
            String,
            String,
            String,
            String,
        ) = eval(
            r#"
            local patch = diff.format(diff.unified("alpha\nbravo\ncharlie\n", "alpha\nBRAVO\ncharlie\n"))
            local out, e = diff.apply("alpha\nBRAVO_NOT\ncharlie\n", patch)
            assert(out == nil)
            return e.kind, e.hunk, e.line, e.change, e.expected, e.found, tostring(e)
        "#,
        );
        assert_eq!((kind.as_str(), hunk, line, op.as_str()), ("mismatch", 1, 2, "delete"));
        assert_eq!((expected.as_str(), found.as_str()), ("bravo\n", "BRAVO_NOT\n"));
        assert!(text.starts_with("lib:diff.apply: mismatch:"), "{text}");
    }

    #[test]
    fn apply_overrun_and_start_past_end() {
        let kind: String = eval(
            r#"
            local patch = diff.format(diff.unified("a\nb\nc\n", "a\nB\nc\n"))
            local _, e = diff.apply("a\n", patch)
            return e.kind
        "#,
        );
        assert_eq!(kind, "overrun");
        let (kind, total): (String, u32) = eval(
            r#"
            local _, e = diff.apply("a\n", "--- a\n+++ b\n@@ -9,1 +9,1 @@\n-x\n+y\n")
            return e.kind, e.total
        "#,
        );
        assert_eq!((kind.as_str(), total), ("starts_past_end", 1));
    }

    #[test]
    fn apply_hunks_out_of_order() {
        let kind: String = eval(
            r#"
            local patch = "--- a\n+++ b\n@@ -3,1 +3,1 @@\n-c\n+C\n@@ -1,1 +1,1 @@\n-a\n+A\n"
            local _, e = diff.apply("a\nb\nc\n", patch)
            return e.kind
        "#,
        );
        assert_eq!(kind, "out_of_order");
    }

    #[test]
    fn apply_a_malformed_patch() {
        let (kind, message, text): (String, String, String) = eval(
            r#"local _, e = diff.apply("a\n", "this isn't a patch\n") return e.kind, e.reason, tostring(e)"#,
        );
        assert_eq!(kind, "malformed_patch");
        assert!(message.contains("not a unified diff"), "{message}");
        assert!(text.starts_with("lib:diff.apply: malformed_patch:"), "{text}");
    }

    #[test]
    fn apply_changes_round_trip_and_validation() {
        assert_eq!(
            eval::<String>(
                r#"
                local old, new = "alpha\nbravo\ncharlie\n", "alpha\nBRAVO\ncharlie\n"
                return assert(diff.apply_changes(old, diff.lines(old, new)))
            "#
            ),
            "alpha\nBRAVO\ncharlie\n"
        );
        let (kind, got, expected): (String, u64, u64) = eval(
            r#"
            local cs = { { op = "equal", value = "wrong\n" }, { op = "insert", value = "anything\n" } }
            local out, e = diff.apply_changes("alpha\n", cs)
            assert(out == nil)
            return e.kind, e.got, e.expected
        "#,
        );
        assert_eq!((kind.as_str(), got, expected), ("does_not_reconstruct", 6, 6));
    }

    #[test]
    fn an_invalid_op_is_refused_as_an_argument() {
        let err = refused(r#"return diff.apply_changes("x", { { op = "swap", value = "x" } })"#);
        assert!(
            err.contains("lib:diff.apply_changes: argument #2 'changes'") && err.contains("swap"),
            "{err}"
        );
        let err = refused(r#"return diff.apply_changes("x", { { op = "equal" } })"#);
        assert!(err.contains("argument #2 'changes'"), "{err}");
    }

    #[test]
    fn similarity_pins() {
        assert_eq!(eval::<f64>(r#"return diff.similarity("anything", "anything")"#), 1.0);
        assert_eq!(eval::<f64>(r#"return diff.similarity("", "")"#), 1.0);
        assert_eq!(eval::<f64>(r#"return diff.similarity("", "anything")"#), 0.0);
        assert_eq!(eval::<f64>(r#"return diff.similarity("anything", "")"#), 0.0);
        let (ab, ba): (f64, f64) = eval(
            r#"
            local a, b = "the quick brown fox", "the lazy red fox"
            return diff.similarity(a, b), diff.similarity(b, a)
        "#,
        );
        assert!((ab - ba).abs() < 1e-9, "ab={ab} ba={ba}");
        let hello: f64 = eval(r#"return diff.similarity("hello", "world")"#);
        assert!((0.0..=1.0).contains(&hello));
        assert!(eval::<f64>(r#"return diff.similarity("the quick brown fox", "the quick brown box")"#) > 0.9);
    }

    #[test]
    fn a_forged_length_does_not_size_a_buffer() {
        // A table whose `#` is huge must not make `apply_changes` allocate from it.
        let ok: bool = eval(
            r#"
            local forged = setmetatable({ { op = "equal", value = "x" } }, { __len = function() return 2^30 end })
            local ok = pcall(diff.apply_changes, "x", forged)
            return true
        "#,
        );
        assert!(ok);
    }
}
