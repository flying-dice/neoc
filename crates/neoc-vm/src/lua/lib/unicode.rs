//! Unicode normalisation, case-folding, grapheme segmentation, and codepoint
//! properties — `lib:unicode`.
//!
//! Backs onto the unicode-rs crate family: `unicode-normalization`
//! (NFC/NFD/NFKC/NFKD), `caseless` (default case fold + canonical caseless
//! match), `unicode-segmentation` (extended grapheme clusters),
//! `unicode-properties` (`General_Category`), and `unicode-width` (East-Asian
//! display width — shared with `lib:text`).
//!
//! Every function is a flat, stateless transform over a UTF-8 string; there is
//! no resource to model, so the module is flat-functions-only (per the module API
//! pattern in CLAUDE.md, stateless modules need no UserData surface). All
//! arguments are taken as `String`, so non-UTF-8 input is rejected at the bridge before any body runs
//! — a Unicode module has no meaning for ill-formed bytes.

use caseless::{canonical_caseless_match_str, default_case_fold_str};
use mlua::{Lua, Table};
use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::lua::facade::Module;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:unicode` surface through the facade.
///
/// `normalize` and `category` are fallible and route through
/// [`crate::lua::facade::Module::try_func`] (the `(value, error)` channel):
/// `normalize` rejects an unknown form, `category` rejects input that is not
/// exactly one codepoint. The other seven functions are total over any UTF-8
/// string and route through [`crate::lua::facade::Module::func`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:unicode")?;

    m.try_func(
        "normalize",
        &[("s", "string"), ("form", "string")],
        "Normalise `s` to a Unicode normalisation form: \"nfc\", \"nfd\", \"nfkc\", or \"nfkd\" (case-insensitive). C-forms compose combining sequences, D-forms decompose them; the K-forms additionally fold compatibility characters (ﬁ → fi, ① → 1).",
        |_, (s, form): (String, String)| normalize_form(&s, &form),
    )?
    .func(
        "fold",
        &[("s", "string")],
        "Default Unicode case fold of `s` for caseless comparison (UAX #44): ß → \"ss\", ẞ → \"ss\", final and medial sigma → σ. Produces a comparison key — fold both sides and compare, do not display the result.",
        |_, s: String| Ok(default_case_fold_str(&s)),
    )?
    .func(
        "caseless_eq",
        &[("a", "string"), ("b", "string")],
        "Canonical caseless match of `a` and `b` (UAX #44 D145): true when they are equal ignoring case and ignoring canonical-equivalent combining-mark spellings. The robust default for comparing user-supplied text.",
        |_, (a, b): (String, String)| Ok(canonical_caseless_match_str(&a, &b)),
    )?
    .func(
        "graphemes",
        &[("s", "string")],
        "Split `s` into a 1-based array of extended grapheme clusters — user-perceived characters (UAX #29). A flag emoji or a base-plus-combining-accent is one element, not its component codepoints.",
        build_grapheme_table,
    )?
    .func(
        "grapheme_count",
        &[("s", "string")],
        "Count the extended grapheme clusters in `s` (UAX #29) without materialising them — the user-perceived length, which `#s` (bytes) and codepoint count both miss.",
        |_, s: String| Ok(s.graphemes(true).count() as i64),
    )?
    .try_func(
        "category",
        &[("ch", "string")],
        "General_Category of the single codepoint `ch` as its two-letter abbreviation (\"Lu\", \"Ll\", \"Nd\", \"Zs\", …). Errors unless `ch` is exactly one codepoint.",
        |_, ch: String| category_of(&ch),
    )?
    .func(
        "is_alphabetic",
        &[("s", "string")],
        "True when `s` is non-empty and every codepoint has the Unicode Alphabetic property — letters plus letter-numbers such as Roman numerals (broader than Python `str.isalpha`, which counts letters only). The empty string is false.",
        |_, s: String| Ok(!s.is_empty() && s.chars().all(char::is_alphabetic)),
    )?
    .func(
        "is_numeric",
        &[("s", "string")],
        "True when `s` is non-empty and every codepoint is in a Number category — Nd, Nl, or No (narrower than Python `str.isnumeric`, which also counts Numeric_Type codepoints outside those categories such as the CJK numeral 一, U+4E00, category Lo). The empty string is false.",
        |_, s: String| Ok(!s.is_empty() && s.chars().all(char::is_numeric)),
    )?
    .func(
        "width",
        &[("s", "string")],
        "Terminal display width of `s` in columns (Unicode East-Asian Width): wide/fullwidth CJK count as 2, zero-width and most combining marks as 0. Use this to align text, not `#s` (bytes) or grapheme count.",
        |_, s: String| Ok(display_width(&s)),
    )?;

    Ok(m)
}

/// Normalises `s` to the requested form. `form` is matched case-insensitively
/// against the four canonical names; anything else is the `(value, error)`
/// channel.
fn normalize_form(s: &str, form: &str) -> Result<String, String> {
    match form.to_ascii_lowercase().as_str() {
        "nfc" => Ok(s.nfc().collect()),
        "nfd" => Ok(s.nfd().collect()),
        "nfkc" => Ok(s.nfkc().collect()),
        "nfkd" => Ok(s.nfkd().collect()),
        other => Err(format!(
            "unknown form {other:?}, expected one of \"nfc\", \"nfd\", \"nfkc\", \"nfkd\""
        )),
    }
}

/// Builds the 1-based grapheme-cluster array backing the `graphemes` function.
fn build_grapheme_table(lua: &Lua, s: String) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (i, g) in s.graphemes(true).enumerate() {
        t.raw_set(i + 1, g)?;
    }
    Ok(t)
}

/// Returns the two-letter `General_Category` of the single codepoint in `ch`,
/// or an error naming the codepoint count when `ch` is not exactly one.
fn category_of(ch: &str) -> Result<String, String> {
    let mut chars = ch.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(category_code(c.general_category()).to_string()),
        _ => Err(format!(
            "expected a single codepoint, got {}",
            ch.chars().count()
        )),
    }
}

/// Maps a [`GeneralCategory`] to its UAX #44 two-letter abbreviation. The
/// `unicode-properties` enum carries only long names, so the short codes — the
/// form every other runtime's API speaks — are spelled out here.
fn category_code(gc: GeneralCategory) -> &'static str {
    use GeneralCategory::{
        ClosePunctuation, ConnectorPunctuation, Control, CurrencySymbol, DashPunctuation,
        DecimalNumber, EnclosingMark, FinalPunctuation, Format, InitialPunctuation, LetterNumber,
        LineSeparator, LowercaseLetter, MathSymbol, ModifierLetter, ModifierSymbol, NonspacingMark,
        OpenPunctuation, OtherLetter, OtherNumber, OtherPunctuation, OtherSymbol,
        ParagraphSeparator, PrivateUse, SpaceSeparator, SpacingMark, Surrogate, TitlecaseLetter,
        Unassigned, UppercaseLetter,
    };
    match gc {
        UppercaseLetter => "Lu",
        LowercaseLetter => "Ll",
        TitlecaseLetter => "Lt",
        ModifierLetter => "Lm",
        OtherLetter => "Lo",
        NonspacingMark => "Mn",
        SpacingMark => "Mc",
        EnclosingMark => "Me",
        DecimalNumber => "Nd",
        LetterNumber => "Nl",
        OtherNumber => "No",
        ConnectorPunctuation => "Pc",
        DashPunctuation => "Pd",
        OpenPunctuation => "Ps",
        ClosePunctuation => "Pe",
        InitialPunctuation => "Pi",
        FinalPunctuation => "Pf",
        OtherPunctuation => "Po",
        MathSymbol => "Sm",
        CurrencySymbol => "Sc",
        ModifierSymbol => "Sk",
        OtherSymbol => "So",
        SpaceSeparator => "Zs",
        LineSeparator => "Zl",
        ParagraphSeparator => "Zp",
        Control => "Cc",
        Format => "Cf",
        Surrogate => "Cs",
        PrivateUse => "Co",
        Unassigned => "Cn",
    }
}

/// Sums the East-Asian display width of every codepoint; width-less codepoints
/// (control, most combining marks) contribute 0.
fn display_width(s: &str) -> i64 {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as i64)
        .sum()
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    /// Test-only engine binding `uni = require("lib:unicode")` as a global, to
    /// keep the inline test bodies focused on Unicode behaviour rather than
    /// require's plumbing (which has its own coverage).
    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:unicode")"#).eval().unwrap();
        lua.globals().set("uni", m).unwrap();
        lua
    }

    #[test]
    fn normalize_nfc_composes() {
        let lua = engine();
        // "e" + U+0301 COMBINING ACUTE ACCENT composes to U+00E9 "é".
        let (s, err): (String, Option<String>) = lua
            .load(r#"return uni.normalize("e\u{0301}", "nfc")"#)
            .eval()
            .unwrap();
        assert!(err.is_none());
        assert_eq!(s, "é");
        assert_eq!(s.chars().count(), 1);
    }

    #[test]
    fn normalize_nfd_decomposes() {
        let lua = engine();
        let s: mlua::LuaString = lua
            .load(r#"return (uni.normalize("é", "nfd"))"#)
            .eval()
            .unwrap();
        // Precomposed "é" decomposes to "e" + combining accent: 3 bytes.
        assert_eq!(s.as_bytes().len(), 3);
    }

    #[test]
    fn normalize_nfkc_folds_compatibility() {
        let lua = engine();
        // U+FB01 LATIN SMALL LIGATURE FI → "fi" under the compatibility forms.
        let (s, _): (String, Option<String>) = lua
            .load(r#"return uni.normalize("ﬁ", "nfkc")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "fi");
    }

    #[test]
    fn normalize_form_is_case_insensitive() {
        let lua = engine();
        let (s, err): (String, Option<String>) = lua
            .load(r#"return uni.normalize("é", "NFC")"#)
            .eval()
            .unwrap();
        assert!(err.is_none());
        assert_eq!(s, "é");
    }

    #[test]
    fn normalize_unknown_form_errors_with_prefix() {
        let lua = engine();
        let (v, err): (Option<String>, Option<String>) = lua
            .load(r#"return uni.normalize("x", "nfz")"#)
            .eval()
            .unwrap();
        assert!(v.is_none());
        let err = err.expect("error string");
        assert!(err.starts_with("unicode.normalize:"), "prefix: {err}");
        assert!(err.contains("nfz"), "names the bad form: {err}");
    }

    #[test]
    fn fold_collapses_case_and_sharp_s() {
        let lua = engine();
        let mixed: String = lua.load(r#"return uni.fold("MASSE")"#).eval().unwrap();
        assert_eq!(mixed, "masse");
        // ß case-folds to "ss", so "STRASSE" and "straße" share a fold key.
        let a: String = lua.load(r#"return uni.fold("STRASSE")"#).eval().unwrap();
        let b: String = lua.load(r#"return uni.fold("straße")"#).eval().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn caseless_eq_ignores_case_and_normalisation() {
        let lua = engine();
        // NFC "é" vs NFD "e"+accent, differing case — canonical caseless match.
        let yes: bool = lua
            .load(r#"return uni.caseless_eq("É", "e\u{0301}")"#)
            .eval()
            .unwrap();
        assert!(yes);
        let no: bool = lua
            .load(r#"return uni.caseless_eq("apple", "orange")"#)
            .eval()
            .unwrap();
        assert!(!no);
    }

    #[test]
    fn graphemes_groups_combining_and_emoji() {
        let lua = engine();
        // "a" + thumbs-up with a skin-tone modifier + "b" → three graphemes.
        let n: i64 = lua.load(r#"return #uni.graphemes("a👍🏽b")"#).eval().unwrap();
        assert_eq!(n, 3);
        let first: String = lua
            .load(r#"return uni.graphemes("a👍🏽b")[1]"#)
            .eval()
            .unwrap();
        assert_eq!(first, "a");
    }

    #[test]
    fn grapheme_count_matches_array_length() {
        let lua = engine();
        let n: i64 = lua
            .load(r#"return uni.grapheme_count("a👍🏽b")"#)
            .eval()
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn category_classifies_single_codepoints() {
        let lua = engine();
        let cases = [("A", "Lu"), ("a", "Ll"), ("1", "Nd"), (" ", "Zs")];
        for (input, want) in cases {
            let (got, err): (String, Option<String>) = lua
                .load(format!(r#"return uni.category("{input}")"#))
                .eval()
                .unwrap();
            assert!(err.is_none(), "{input}: {err:?}");
            assert_eq!(got, want, "category({input:?})");
        }
    }

    #[test]
    fn category_rejects_multi_codepoint_input() {
        let lua = engine();
        let (v, err): (Option<String>, Option<String>) =
            lua.load(r#"return uni.category("ab")"#).eval().unwrap();
        assert!(v.is_none());
        assert!(err.unwrap().starts_with("unicode.category:"));
    }

    #[test]
    fn is_alphabetic_and_is_numeric_predicates() {
        let lua = engine();
        let alpha: bool = lua
            .load(r#"return uni.is_alphabetic("héllo")"#)
            .eval()
            .unwrap();
        assert!(alpha);
        let mixed: bool = lua
            .load(r#"return uni.is_alphabetic("abc1")"#)
            .eval()
            .unwrap();
        assert!(!mixed);
        let empty: bool = lua.load(r#"return uni.is_alphabetic("")"#).eval().unwrap();
        assert!(!empty, "empty string is not alphabetic");

        let num: bool = lua.load(r#"return uni.is_numeric("123")"#).eval().unwrap();
        assert!(num);
        let not_num: bool = lua.load(r#"return uni.is_numeric("12a")"#).eval().unwrap();
        assert!(!not_num);
    }

    #[test]
    fn width_counts_east_asian_columns() {
        let lua = engine();
        let ascii: i64 = lua.load(r#"return uni.width("abc")"#).eval().unwrap();
        assert_eq!(ascii, 3);
        // Two fullwidth CJK ideographs occupy four columns.
        let cjk: i64 = lua.load(r#"return uni.width("世界")"#).eval().unwrap();
        assert_eq!(cjk, 4);
        let empty: i64 = lua.load(r#"return uni.width("")"#).eval().unwrap();
        assert_eq!(empty, 0);
    }
}
