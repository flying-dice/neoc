//! Pure three-way text merge for two-way bound text.
//!
//! The host holds `base` (the value it last sent the view), `theirs` (the value now in the database,
//! usually an agent's write) and `mine` (what the human typed). The merge works on *tokens* (a word, a run
//! of blanks, or one punctuation or newline character), not lines and not bytes, so that "milk" becoming
//! "Milk" composes with an agent appending a line, while two different rewrites of the same word do not
//! silently interleave into a third word.
//!
//! Rules:
//! - edits that touch different tokens of `base` compose;
//! - two pure insertions at the same point compose, **theirs first**;
//! - an insertion at the edge of another edit composes (it is outside the replaced tokens);
//! - overlapping edits are a conflict and the human (**mine**) wins that region; the caller keeps
//!   [`Merge::lost`] (the agent's whole value) as a revision so nothing is silently lost;
//! - identical edits on both sides are not a conflict.

use std::cmp::Ordering;

/// Past this many edits between the compared texts the diff gives up and treats the whole differing middle
/// as one edit (which then conflicts if both sides changed it). Bounds time and memory on pathological input.
const MAX_EDIT_DISTANCE: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge {
    /// The merged text.
    pub text: String,
    /// At least one region was edited on both sides differently; it resolved to `mine`.
    pub conflict: bool,
    /// On a conflict, the value that lost (`theirs`): the caller records it as a revision.
    pub lost: Option<String>,
}

/// Merge `mine` and `theirs`, both derived from `base`.
#[must_use]
#[allow(clippy::many_single_char_names)] // b/t/m: base, theirs, mine, as in diff3
pub fn merge3(base: &str, theirs: &str, mine: &str) -> Merge {
    let clean = |text: &str| Merge { text: text.to_owned(), conflict: false, lost: None };
    if mine == theirs || theirs == base {
        return clean(mine);
    }
    if mine == base {
        return clean(theirs);
    }
    let (b, t, m) = (tokenize(base), tokenize(theirs), tokenize(mine));
    let mut their_hunks = diff(&b, &t).into_iter().peekable();
    let mut my_hunks = diff(&b, &m).into_iter().peekable();

    let mut out = String::with_capacity(base.len().max(theirs.len()).max(mine.len()));
    let mut pos = 0; // next base token not yet emitted
    let mut conflict = false;
    loop {
        let (side, hunk) = match (their_hunks.peek(), my_hunks.peek()) {
            (None, None) => break,
            (Some(h), None) => (Side::Theirs, h.clone()),
            (None, Some(h)) => (Side::Mine, h.clone()),
            (Some(th), Some(mh)) => match order(th, mh) {
                Ordering::Greater => (Side::Mine, mh.clone()),
                _ => (Side::Theirs, th.clone()),
            },
        };
        out.push_str(&b[pos..hunk.a_start].concat());

        // is the next hunk of the OTHER side overlapping this one? then resolve the whole cluster
        let other_overlaps = match side {
            Side::Theirs => my_hunks.peek().is_some_and(|o| overlaps(&hunk, o)),
            Side::Mine => their_hunks.peek().is_some_and(|o| overlaps(&hunk, o)),
        };
        if !other_overlaps {
            out.push_str(&hunk.replacement(&t, &m, side));
            pos = hunk.a_end;
            match side {
                Side::Theirs => their_hunks.next(),
                Side::Mine => my_hunks.next(),
            };
            continue;
        }

        // cluster: grow [lo, hi) while any next hunk (either side) overlaps it
        let (lo, mut hi) = (hunk.a_start, hunk.a_end);
        let mut theirs_in = Vec::new();
        let mut mine_in = Vec::new();
        match side {
            Side::Theirs => theirs_in.extend(their_hunks.next()),
            Side::Mine => mine_in.extend(my_hunks.next()),
        }
        let union = |hi: usize| Hunk { a_start: lo, a_end: hi, b_start: 0, b_end: 0 };
        loop {
            let mut grew = false;
            while let Some(h) = their_hunks.peek() {
                if !overlaps(&union(hi), h) {
                    break;
                }
                hi = hi.max(h.a_end);
                theirs_in.extend(their_hunks.next());
                grew = true;
            }
            while let Some(h) = my_hunks.peek() {
                if !overlaps(&union(hi), h) {
                    break;
                }
                hi = hi.max(h.a_end);
                mine_in.extend(my_hunks.next());
                grew = true;
            }
            if !grew {
                break;
            }
        }
        let theirs_region = apply(&b, lo, hi, &theirs_in, &t);
        let mine_region = apply(&b, lo, hi, &mine_in, &m);
        if theirs_region != mine_region {
            conflict = true;
        }
        out.push_str(&mine_region); // the human wins
        pos = hi;
    }
    out.push_str(&b[pos..].concat());
    let lost = conflict.then(|| theirs.to_owned());
    Merge { text: out, conflict, lost }
}

#[derive(Clone, Copy)]
enum Side {
    Theirs,
    Mine,
}

/// A replacement of `base[a_start..a_end]` by `other[b_start..b_end]` (token indexes).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hunk {
    a_start: usize,
    a_end: usize,
    b_start: usize,
    b_end: usize,
}

impl Hunk {
    fn replacement(&self, theirs: &[&str], mine: &[&str], side: Side) -> String {
        let tokens = match side {
            Side::Theirs => theirs,
            Side::Mine => mine,
        };
        tokens[self.b_start..self.b_end].concat()
    }
    fn is_insertion(&self) -> bool {
        self.a_start == self.a_end
    }
}

/// Processing order: by position in `base`; at the same position an insertion goes before a replacement;
/// otherwise theirs goes first.
fn order(theirs: &Hunk, mine: &Hunk) -> Ordering {
    theirs.a_start.cmp(&mine.a_start).then_with(|| mine.is_insertion().cmp(&theirs.is_insertion()))
}

/// Do two edits share a base token (or does one insert strictly inside the other)? Edits that merely touch
/// do not overlap.
fn overlaps(a: &Hunk, b: &Hunk) -> bool {
    a.a_start < b.a_end && b.a_start < a.a_end
}

/// `base[lo..hi]` with `hunks` (all inside `[lo, hi)`, ordered, from one side) applied.
fn apply(base: &[&str], lo: usize, hi: usize, hunks: &[Hunk], other: &[&str]) -> String {
    let mut out = String::new();
    let mut pos = lo;
    for h in hunks {
        out.push_str(&base[pos..h.a_start].concat());
        out.push_str(&other[h.b_start..h.b_end].concat());
        pos = h.a_end;
    }
    out.push_str(&base[pos..hi].concat());
    out
}

/// Split into tokens: a run of alphanumerics/underscores, a run of spaces/tabs, or one other character.
fn tokenize(s: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Blank,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c == ' ' || c == '\t' {
            Class::Blank
        } else {
            Class::Other
        }
    };
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut prev: Option<Class> = None;
    for (i, c) in s.char_indices() {
        let cls = class(c);
        let continues = matches!(prev, Some(p) if p == cls && cls != Class::Other);
        if !continues && i > start {
            tokens.push(&s[start..i]);
            start = i;
        }
        prev = Some(cls);
    }
    if start < s.len() {
        tokens.push(&s[start..]);
    }
    tokens
}

/// The edits turning `a` into `b`, as ordered non-adjacent hunks.
fn diff(a: &[&str], b: &[&str]) -> Vec<Hunk> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if a_mid.is_empty() && b_mid.is_empty() {
        return vec![];
    }
    let mut hunks = match myers(a_mid, b_mid) {
        Some(h) => h,
        None => vec![Hunk { a_start: 0, a_end: a_mid.len(), b_start: 0, b_end: b_mid.len() }],
    };
    for h in &mut hunks {
        h.a_start += prefix;
        h.a_end += prefix;
        h.b_start += prefix;
        h.b_end += prefix;
    }
    hunks
}

/// Myers' O(ND) diff. `None` when the edit distance exceeds [`MAX_EDIT_DISTANCE`].
#[allow(clippy::many_single_char_names)] // the textbook variable names (n, m, d, k, x, y, v)
fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Hunk>> {
    let (n, m) = (a.len(), b.len());
    let max = (n + m).min(MAX_EDIT_DISTANCE);
    let offset = max + 1;
    let mut v = vec![0usize; 2 * max + 3];
    let mut trace: Vec<Vec<usize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=max {
        trace.push(v.clone());
        let d_i = isize::try_from(d).ok()?;
        let mut k = -d_i;
        while k <= d_i {
            let idx = |k: isize| usize::try_from(isize::try_from(offset).ok()? + k).ok();
            let down = k == -d_i || (k != d_i && v[idx(k - 1)?] < v[idx(k + 1)?]);
            let mut x = if down { v[idx(k + 1)?] } else { v[idx(k - 1)?] + 1 };
            let mut y = usize::try_from(isize::try_from(x).ok()? - k).ok()?;
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            v[idx(k)?] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
            k += 2;
        }
    }
    let d_total = found?;
    // backtrack into per-token edits, then group into hunks
    let mut edits: Vec<(usize, usize, bool)> = Vec::new(); // (x, y, is_delete): a deletion at a[x] or insertion of b[y]
    let (mut x, mut y) = (n, m);
    for d in (1..=d_total).rev() {
        let v = &trace[d];
        let d_i = isize::try_from(d).ok()?;
        let k = isize::try_from(x).ok()? - isize::try_from(y).ok()?;
        let idx = |k: isize| usize::try_from(isize::try_from(offset).ok()? + k).ok();
        let down = k == -d_i || (k != d_i && v[idx(k - 1)?] < v[idx(k + 1)?]);
        let prev_k = if down { k + 1 } else { k - 1 };
        let prev_x = v[idx(prev_k)?];
        let prev_y = usize::try_from(isize::try_from(prev_x).ok()? - prev_k).ok()?;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
        }
        if down {
            edits.push((x, y - 1, false)); // insertion of b[y-1] at a-position x
        } else {
            edits.push((x - 1, y, true)); // deletion of a[x-1]
        }
        x = prev_x;
        y = prev_y;
    }
    edits.reverse();
    Some(group(&edits))
}

/// Group single-token edits into hunks (consecutive in both sequences).
fn group(edits: &[(usize, usize, bool)]) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    for &(x, y, is_delete) in edits {
        let (a_lo, a_hi, b_lo, b_hi) = if is_delete { (x, x + 1, y, y) } else { (x, x, y, y + 1) };
        match hunks.last_mut() {
            Some(h) if h.a_end == a_lo && h.b_end == b_lo => {
                h.a_end = a_hi.max(h.a_end);
                h.b_end = b_hi.max(h.b_end);
            }
            _ => hunks.push(Hunk { a_start: a_lo, a_end: a_hi, b_start: b_lo, b_end: b_hi }),
        }
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m3(base: &str, theirs: &str, mine: &str) -> (String, bool) {
        let r = merge3(base, theirs, mine);
        (r.text, r.conflict)
    }

    #[test]
    fn the_spec_example_composes() {
        // the user capitalised "milk" while an agent appended a line
        let r = merge3("milk", "milk\n- buy oat milk", "Milk");
        assert_eq!(r.text, "Milk\n- buy oat milk");
        assert!(!r.conflict && r.lost.is_none());
    }

    #[test]
    fn trivial_cases() {
        assert_eq!(m3("a b", "a b", "a b"), ("a b".into(), false));
        assert_eq!(m3("a b", "a c", "a b"), ("a c".into(), false), "only they edited");
        assert_eq!(m3("a b", "a b", "a d"), ("a d".into(), false), "only I edited");
        assert_eq!(m3("a b", "a x", "a x"), ("a x".into(), false), "identical edits are not a conflict");
        assert_eq!(m3("", "", ""), (String::new(), false));
        assert_eq!(
            m3("", "theirs", "mine"),
            ("theirsmine".into(), false),
            "two insertions at one point compose"
        );
    }

    #[test]
    fn edits_in_different_places_compose() {
        let base = "one\ntwo\nthree\nfour\n";
        let theirs = "one\ntwo\nthree\nfour\nfive\n"; // agent appended
        let mine = "ONE\ntwo\nthree\nfour\n"; // human edited the first line
        assert_eq!(m3(base, theirs, mine), ("ONE\ntwo\nthree\nfour\nfive\n".into(), false));
        assert_eq!(m3(base, mine, theirs), ("ONE\ntwo\nthree\nfour\nfive\n".into(), false), "symmetric");
    }

    #[test]
    fn same_point_insertions_compose_theirs_first() {
        let r = merge3("a,b", "a,X,b", "a,Y,b");
        assert_eq!((r.text.as_str(), r.conflict), ("a,X,Y,b", false));
        let r = merge3("ab", "aXb", "aYb");
        assert!(r.conflict, "a word rewritten two ways is one edit of one token, not two insertions");
        let r = merge3("start end", "start AGENT end", "start HUMAN end");
        assert_eq!(r.text, "start AGENT HUMAN end", "blanks around the insertion are shared tokens");
        assert!(!r.conflict);
    }

    #[test]
    fn an_insertion_at_the_edge_of_an_edit_composes() {
        // I replace "milk", they append right after it
        assert_eq!(m3("buy milk now", "buy milk! now", "buy oat now"), ("buy oat! now".into(), false));
    }

    #[test]
    fn overlapping_edits_conflict_and_the_human_wins() {
        let r = merge3("buy milk today", "buy bread today", "buy cheese today");
        assert_eq!(r.text, "buy cheese today");
        assert!(r.conflict);
        assert_eq!(r.lost.as_deref(), Some("buy bread today"), "their whole value is kept as a revision");
    }

    #[test]
    fn a_deletion_overlapping_an_edit_conflicts() {
        let r = merge3("a b c", "a c", "a B c");
        assert!(r.conflict);
        assert_eq!(r.text, "a B c");
    }

    #[test]
    fn non_overlapping_hunks_still_compose_next_to_a_conflict() {
        let base = "alpha beta\n\ngamma delta\n";
        let theirs = "alpha BETA\n\ngamma delta\nepsilon\n"; // edits beta, appends
        let mine = "alpha Beta\n\ngamma delta\n"; // edits beta differently
        let r = merge3(base, theirs, mine);
        assert!(r.conflict);
        assert_eq!(r.text, "alpha Beta\n\ngamma delta\nepsilon\n");
    }

    #[test]
    fn a_deletion_of_theirs_composes_with_my_edit_elsewhere() {
        let base = "keep this\nremove me\nand this\n";
        let theirs = "keep this\nand this\n";
        let mine = "keep that\nremove me\nand this\n";
        assert_eq!(m3(base, theirs, mine), ("keep that\nand this\n".into(), false));
    }

    #[test]
    fn unicode_and_emoji_are_never_split() {
        let r = merge3("café ☕ au lait", "café ☕ au lait 🥛", "Café ☕ au lait");
        assert_eq!(r.text, "Café ☕ au lait 🥛");
        assert!(!r.conflict);
    }

    #[test]
    fn pathological_input_degrades_to_a_whole_text_edit_not_a_hang() {
        let a: String = (0..6_000).fold(String::new(), |s, i| s + &format!("a{i} "));
        let b: String = (0..6_000).fold(String::new(), |s, i| s + &format!("b{i} "));
        let c: String = (0..6_000).fold(String::new(), |s, i| s + &format!("c{i} "));
        let r = merge3(&a, &b, &c);
        assert!(r.conflict);
        assert_eq!(r.text, c);
    }

    // ---- property-style tests: a tiny deterministic generator, no extra dependency -------------------

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % n as u64).unwrap()
        }
    }
    const WORDS: &[&str] = &["milk", "eggs", "Δelta", "x", "bread", "tea", "☕", "ok", "a_b", "42"];
    fn word(r: &mut Rng) -> String {
        WORDS[r.below(WORDS.len())].to_owned()
    }
    fn line(r: &mut Rng) -> String {
        (0..=r.below(4)).map(|_| word(r)).collect::<Vec<_>>().join(" ")
    }
    /// A text of `n` lines separated by an untouchable blank line, so edits to distinct lines never touch.
    fn doc(r: &mut Rng, n: usize) -> Vec<String> {
        (0..n).map(|_| line(r)).collect()
    }
    fn join(lines: &[String]) -> String {
        lines.join("\n\n")
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn property_edits_to_disjoint_lines_always_compose_and_commute() {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..300 {
            let n = 3 + r.below(8);
            let base = doc(&mut r, n);
            let (mut theirs, mut mine) = (base.clone(), base.clone());
            let mut expected = base.clone();
            for i in 0..n {
                match r.below(4) {
                    0 => {
                        let edit = format!("{} T", base[i]);
                        theirs[i] = edit.clone();
                        expected[i] = edit;
                    }
                    1 => {
                        let edit = format!("M {}", base[i]);
                        mine[i] = edit.clone();
                        expected[i] = edit;
                    }
                    _ => {}
                }
            }
            let (b, t, m) = (join(&base), join(&theirs), join(&mine));
            let r1 = merge3(&b, &t, &m);
            assert!(!r1.conflict, "disjoint edits must not conflict\nbase={b:?}\ntheirs={t:?}\nmine={m:?}");
            assert_eq!(r1.text, join(&expected));
            let r2 = merge3(&b, &m, &t);
            assert_eq!(r2.text, r1.text, "composition is symmetric");
        }
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn property_overlapping_edits_resolve_to_mine_and_keep_theirs_as_lost() {
        let mut r = Rng(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..300 {
            let n = 2 + r.below(6);
            let base = doc(&mut r, n);
            let i = r.below(n);
            let (mut theirs, mut mine) = (base.clone(), base.clone());
            // every word of the line replaced, differently on each side
            let rewrite = |tag: &str| {
                base[i]
                    .split(' ')
                    .enumerate()
                    .map(|(n, _)| format!("{tag}{n}x"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            theirs[i] = rewrite("t");
            mine[i] = rewrite("m");
            let (b, t, m) = (join(&base), join(&theirs), join(&mine));
            let res = merge3(&b, &t, &m);
            assert!(res.conflict, "same line rewritten two ways\nbase={b:?}");
            assert_eq!(res.text, m, "everything else is unchanged, so mine wins outright");
            assert_eq!(res.lost.as_deref(), Some(t.as_str()));
        }
    }

    #[test]
    fn property_identities_and_robustness_on_random_text() {
        let mut r = Rng(0x1234_5678_9ABC_DEF1);
        let noisy = |r: &mut Rng, from: &str| -> String {
            // random token-level damage: drop, duplicate or inject words
            let mut words: Vec<String> = from.split(' ').map(str::to_owned).collect();
            for _ in 0..r.below(4) {
                let at = r.below(words.len() + 1);
                match r.below(3) {
                    0 if at < words.len() => {
                        words.remove(at);
                    }
                    1 => words.insert(at, word(r)),
                    _ if at < words.len() => words[at] = word(r),
                    _ => {}
                }
            }
            words.join(" ")
        };
        for _ in 0..500 {
            let base = line(&mut r) + " " + &line(&mut r) + " " + &line(&mut r);
            let theirs = noisy(&mut r, &base);
            let mine = noisy(&mut r, &base);
            assert_eq!(merge3(&base, &base, &base).text, base);
            assert_eq!(merge3(&base, &theirs, &base).text, theirs);
            assert_eq!(merge3(&base, &base, &mine).text, mine);
            assert_eq!(merge3(&base, &theirs, &theirs).text, theirs);
            let res = merge3(&base, &theirs, &mine);
            assert_eq!(res.conflict, res.lost.is_some());
            if !res.conflict {
                // a clean merge contains every token both sides added that the other did not remove
                let again = merge3(&base, &mine, &theirs);
                assert!(!again.conflict, "conflict-ness is symmetric");
            }
        }
    }

    #[test]
    fn tokenizer_round_trips() {
        for s in ["", "a", "hello  world\n", "x_1+y", "  \t ", "日本語 text ☕☕"] {
            assert_eq!(tokenize(s).concat(), s);
        }
        assert_eq!(tokenize("ab  c\n\nd"), vec!["ab", "  ", "c", "\n", "\n", "d"]);
    }

    #[test]
    fn diff_hunks_apply_back() {
        let a = tokenize("the quick brown fox jumps");
        let b = tokenize("the slow brown dog jumps over");
        let hunks = diff(&a, &b);
        let mut out = String::new();
        let mut pos = 0;
        for h in &hunks {
            out.push_str(&a[pos..h.a_start].concat());
            out.push_str(&b[h.b_start..h.b_end].concat());
            pos = h.a_end;
        }
        out.push_str(&a[pos..].concat());
        assert_eq!(out, b.concat());
    }
}
