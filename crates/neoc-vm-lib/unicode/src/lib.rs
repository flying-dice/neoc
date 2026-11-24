//! Unicode normalisation, case-folding, grapheme segmentation and codepoint properties: `lib:unicode`.
//!
//! Backed by the unicode-rs family: `unicode-normalization` (NFC/NFD/NFKC/NFKD), `caseless` (default case fold and
//! canonical caseless match), `unicode-segmentation` (extended grapheme clusters), `unicode-properties`
//! (`General_Category`) and `unicode-width` (East-Asian display width). Every function is a stateless transform of
//! a UTF-8 string; text that is not valid UTF-8 is refused as an argument.
//!
//! The normalisation form is a `"nfc" | "nfd" | "nfkc" | "nfkd"` union and `normalize` cannot fail; the codepoint of
//! `category` is a validated type (exactly one codepoint, refused as an argument) and its result is a union of the
//! thirty two-letter categories; counts are unsigned integers.

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

use unicode_properties::GeneralCategory as Gc;

fn display_width(s: &str) -> u64 {
    use unicode_width::UnicodeWidthChar;
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as u64).sum()
}

fn one_codepoint(text: &str) -> Result<(), String> {
    match text.chars().count() {
        1 => Ok(()),
        n => Err(format!("expected a single codepoint, got {n}")),
    }
}

#[mlua_ir::module("lib:unicode")]
pub mod unicode {
    use caseless::{canonical_caseless_match_str, default_case_fold_str};
    use mlua_ir::{Shape, Text};
    use serde::{Deserialize, Serialize};
    use unicode_normalization::UnicodeNormalization;
    use unicode_properties::UnicodeGeneralCategory;
    use unicode_segmentation::UnicodeSegmentation;

    use super::{Gc, display_width, one_codepoint};

    /// A Unicode normalisation form.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Form {
        /// Canonical composition: combining sequences are composed (`e` + U+0301 becomes `é`).
        Nfc,
        /// Canonical decomposition: composed characters are split into base and combining marks.
        Nfd,
        /// Compatibility composition: like `nfc`, and compatibility characters are folded too (`ﬁ` to `fi`).
        Nfkc,
        /// Compatibility decomposition.
        Nfkd,
    }

    /// The `General_Category` of a codepoint (UAX #44), by its two-letter abbreviation.
    #[derive(Debug, Clone, Copy, Serialize, Shape)]
    pub enum Category {
        /// Uppercase letter.
        #[serde(rename = "Lu")]
        UppercaseLetter,
        /// Lowercase letter.
        #[serde(rename = "Ll")]
        LowercaseLetter,
        /// Titlecase letter.
        #[serde(rename = "Lt")]
        TitlecaseLetter,
        /// Modifier letter.
        #[serde(rename = "Lm")]
        ModifierLetter,
        /// Other letter.
        #[serde(rename = "Lo")]
        OtherLetter,
        /// Nonspacing mark.
        #[serde(rename = "Mn")]
        NonspacingMark,
        /// Spacing mark.
        #[serde(rename = "Mc")]
        SpacingMark,
        /// Enclosing mark.
        #[serde(rename = "Me")]
        EnclosingMark,
        /// Decimal digit.
        #[serde(rename = "Nd")]
        DecimalNumber,
        /// Letter number (Roman numerals).
        #[serde(rename = "Nl")]
        LetterNumber,
        /// Other number.
        #[serde(rename = "No")]
        OtherNumber,
        /// Connector punctuation.
        #[serde(rename = "Pc")]
        ConnectorPunctuation,
        /// Dash punctuation.
        #[serde(rename = "Pd")]
        DashPunctuation,
        /// Opening punctuation.
        #[serde(rename = "Ps")]
        OpenPunctuation,
        /// Closing punctuation.
        #[serde(rename = "Pe")]
        ClosePunctuation,
        /// Initial quote.
        #[serde(rename = "Pi")]
        InitialPunctuation,
        /// Final quote.
        #[serde(rename = "Pf")]
        FinalPunctuation,
        /// Other punctuation.
        #[serde(rename = "Po")]
        OtherPunctuation,
        /// Math symbol.
        #[serde(rename = "Sm")]
        MathSymbol,
        /// Currency symbol.
        #[serde(rename = "Sc")]
        CurrencySymbol,
        /// Modifier symbol.
        #[serde(rename = "Sk")]
        ModifierSymbol,
        /// Other symbol.
        #[serde(rename = "So")]
        OtherSymbol,
        /// Space separator.
        #[serde(rename = "Zs")]
        SpaceSeparator,
        /// Line separator.
        #[serde(rename = "Zl")]
        LineSeparator,
        /// Paragraph separator.
        #[serde(rename = "Zp")]
        ParagraphSeparator,
        /// Control character.
        #[serde(rename = "Cc")]
        Control,
        /// Format character.
        #[serde(rename = "Cf")]
        Format,
        /// Surrogate.
        #[serde(rename = "Cs")]
        Surrogate,
        /// Private use.
        #[serde(rename = "Co")]
        PrivateUse,
        /// Not assigned.
        #[serde(rename = "Cn")]
        Unassigned,
    }

    impl From<Gc> for Category {
        fn from(gc: Gc) -> Self {
            match gc {
                Gc::UppercaseLetter => Category::UppercaseLetter,
                Gc::LowercaseLetter => Category::LowercaseLetter,
                Gc::TitlecaseLetter => Category::TitlecaseLetter,
                Gc::ModifierLetter => Category::ModifierLetter,
                Gc::OtherLetter => Category::OtherLetter,
                Gc::NonspacingMark => Category::NonspacingMark,
                Gc::SpacingMark => Category::SpacingMark,
                Gc::EnclosingMark => Category::EnclosingMark,
                Gc::DecimalNumber => Category::DecimalNumber,
                Gc::LetterNumber => Category::LetterNumber,
                Gc::OtherNumber => Category::OtherNumber,
                Gc::ConnectorPunctuation => Category::ConnectorPunctuation,
                Gc::DashPunctuation => Category::DashPunctuation,
                Gc::OpenPunctuation => Category::OpenPunctuation,
                Gc::ClosePunctuation => Category::ClosePunctuation,
                Gc::InitialPunctuation => Category::InitialPunctuation,
                Gc::FinalPunctuation => Category::FinalPunctuation,
                Gc::OtherPunctuation => Category::OtherPunctuation,
                Gc::MathSymbol => Category::MathSymbol,
                Gc::CurrencySymbol => Category::CurrencySymbol,
                Gc::ModifierSymbol => Category::ModifierSymbol,
                Gc::OtherSymbol => Category::OtherSymbol,
                Gc::SpaceSeparator => Category::SpaceSeparator,
                Gc::LineSeparator => Category::LineSeparator,
                Gc::ParagraphSeparator => Category::ParagraphSeparator,
                Gc::Control => Category::Control,
                Gc::Format => Category::Format,
                Gc::Surrogate => Category::Surrogate,
                Gc::PrivateUse => Category::PrivateUse,
                Gc::Unassigned => Category::Unassigned,
            }
        }
    }

    /// Exactly one codepoint (not one grapheme: `é` written as `e` and a combining accent is two).
    #[derive(Debug, Clone, Text)]
    #[text(validate = "one_codepoint")]
    pub struct Codepoint(pub String);

    /// Normalise `s` to a Unicode normalisation form. C-forms compose combining sequences, D-forms decompose them;
    /// the K-forms also fold compatibility characters (`ﬁ` to `fi`, `①` to `1`).
    ///
    /// ```luau
    /// assert(unicode.normalize("e\u{0301}", "nfc") == "\u{e9}")
    /// assert(unicode.normalize("\u{FB01}", "nfkc") == "fi")
    /// ```
    pub fn normalize(s: String, form: Form) -> String {
        match form {
            Form::Nfc => s.nfc().collect(),
            Form::Nfd => s.nfd().collect(),
            Form::Nfkc => s.nfkc().collect(),
            Form::Nfkd => s.nfkd().collect(),
        }
    }

    /// Default Unicode case fold of `s` for caseless comparison (UAX #44): `ß` becomes `ss`, final and medial sigma
    /// become `σ`. A comparison key: fold both sides and compare, do not display the result.
    ///
    /// ```luau
    /// assert(unicode.fold("STRASSE") == unicode.fold("stra\u{df}e"))
    /// ```
    pub fn fold(s: String) -> String {
        default_case_fold_str(&s)
    }

    /// Canonical caseless match of `a` and `b` (UAX #44 D145): equal ignoring case and ignoring
    /// canonical-equivalent spellings of combining marks. The robust default for comparing user-supplied text.
    pub fn caseless_eq(a: String, b: String) -> bool {
        canonical_caseless_match_str(&a, &b)
    }

    /// Split `s` into extended grapheme clusters, the user-perceived characters (UAX #29). A flag emoji or a base
    /// letter with its combining accent is one element, not its component codepoints.
    ///
    /// ```luau
    /// assert(#unicode.graphemes("a\u{1F44D}\u{1F3FD}b") == 3)
    /// ```
    pub fn graphemes(s: String) -> Vec<String> {
        s.graphemes(true).map(str::to_owned).collect()
    }

    /// Count the extended grapheme clusters in `s` (UAX #29) without building them: the user-perceived length,
    /// which `#s` (bytes) and the codepoint count both miss.
    pub fn grapheme_count(s: String) -> u64 {
        s.graphemes(true).count() as u64
    }

    /// The `General_Category` of the codepoint `ch`.
    ///
    /// ```luau
    /// assert(unicode.category("A") == "Lu")
    /// assert(unicode.category("1") == "Nd")
    /// ```
    pub fn category(ch: Codepoint) -> Category {
        ch.0.chars().next().map_or(Category::Unassigned, |c| c.general_category().into())
    }

    /// `true` when `s` is not empty and every codepoint has the Unicode Alphabetic property: letters plus
    /// letter-numbers such as Roman numerals (broader than Python's `isalpha`). The empty string is `false`.
    pub fn is_alphabetic(s: String) -> bool {
        !s.is_empty() && s.chars().all(char::is_alphabetic)
    }

    /// `true` when `s` is not empty and every codepoint is in a Number category (Nd, Nl or No). Narrower than
    /// Python's `isnumeric`: the CJK numeral 一 is a letter (Lo). The empty string is `false`.
    pub fn is_numeric(s: String) -> bool {
        !s.is_empty() && s.chars().all(char::is_numeric)
    }

    /// Terminal display width of `s` in columns (Unicode East-Asian Width): wide and fullwidth CJK count as 2,
    /// zero-width and most combining marks as 0. Use it to align text, not `#s` or the grapheme count.
    ///
    /// ```luau
    /// assert(unicode.width("\u{4E16}\u{754C}") == 4)
    /// ```
    pub fn width(s: String) -> u64 {
        display_width(&s)
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:unicode")"#).eval().unwrap();
        lua.globals().set("uni", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    #[test]
    fn normalize_nfc_composes() {
        // "e" + U+0301 COMBINING ACUTE ACCENT composes to U+00E9.
        let s: String = eval(r#"return uni.normalize("e\u{0301}", "nfc")"#);
        assert_eq!(s, "é");
        assert_eq!(s.chars().count(), 1);
    }

    #[test]
    fn normalize_nfd_decomposes() {
        let n: usize = eval(r#"return #uni.normalize("é", "nfd")"#);
        // "e" + the combining accent: 3 bytes.
        assert_eq!(n, 3);
    }

    #[test]
    fn normalize_nfkc_folds_compatibility() {
        assert_eq!(eval::<String>(r#"return uni.normalize("ﬁ", "nfkc")"#), "fi");
        assert_eq!(eval::<String>(r#"return uni.normalize("①", "nfkd")"#), "1");
    }

    #[test]
    fn normalize_form_is_a_declared_union() {
        // Not case-insensitive any more, and an unknown form is refused as an argument.
        let nope = refused(r#"return uni.normalize("x", "nfz")"#);
        assert!(nope.contains("argument #2 'form'"), "{nope}");
        assert!(refused(r#"return uni.normalize("x", "NFC")"#).contains("argument #2 'form'"));
    }

    #[test]
    fn fold_collapses_case_and_sharp_s() {
        assert_eq!(eval::<String>(r#"return uni.fold("MASSE")"#), "masse");
        let same: bool = eval(r#"return uni.fold("STRASSE") == uni.fold("straße")"#);
        assert!(same);
    }

    #[test]
    fn caseless_eq_ignores_case_and_normalisation() {
        assert!(eval::<bool>(r#"return uni.caseless_eq("É", "e\u{0301}")"#));
        assert!(!eval::<bool>(r#"return uni.caseless_eq("apple", "orange")"#));
    }

    #[test]
    fn graphemes_groups_combining_and_emoji() {
        assert_eq!(eval::<usize>(r#"return #uni.graphemes("a👍🏽b")"#), 3);
        assert_eq!(eval::<String>(r#"return uni.graphemes("a👍🏽b")[1]"#), "a");
        assert_eq!(eval::<String>(r#"return uni.graphemes("a👍🏽b")[2]"#), "👍🏽");
    }

    #[test]
    fn grapheme_count_matches_array_length() {
        assert_eq!(eval::<u64>(r#"return uni.grapheme_count("a👍🏽b")"#), 3);
        assert_eq!(eval::<u64>(r#"return uni.grapheme_count("")"#), 0);
    }

    #[test]
    fn category_classifies_single_codepoints() {
        let cases = [
            ("A", "Lu"),
            ("a", "Ll"),
            ("1", "Nd"),
            (" ", "Zs"),
            ("-", "Pd"),
            ("$", "Sc"),
            ("+", "Sm"),
            ("\\0", "Cc"),
            ("Ⅳ", "Nl"),
            ("一", "Lo"),
        ];
        for (input, want) in cases {
            let got: String = eval(&format!(r#"return uni.category("{input}")"#));
            assert_eq!(got, want, "category({input:?})");
        }
    }

    #[test]
    fn category_refuses_anything_but_one_codepoint() {
        for bad in ["ab", "", "e\\u{0301}"] {
            let err = refused(&format!(r#"return uni.category("{bad}")"#));
            assert!(
                err.contains("argument #1 'ch'") && err.contains("expected a single codepoint"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn is_alphabetic_and_is_numeric_predicates() {
        assert!(eval::<bool>(r#"return uni.is_alphabetic("héllo")"#));
        assert!(!eval::<bool>(r#"return uni.is_alphabetic("abc1")"#));
        assert!(!eval::<bool>(r#"return uni.is_alphabetic("")"#), "the empty string is not alphabetic");
        assert!(eval::<bool>(r#"return uni.is_numeric("123")"#));
        assert!(!eval::<bool>(r#"return uni.is_numeric("12a")"#));
        assert!(!eval::<bool>(r#"return uni.is_numeric("")"#));
        // The CJK numeral is a letter, not a number.
        assert!(!eval::<bool>(r#"return uni.is_numeric("一")"#));
    }

    #[test]
    fn width_counts_east_asian_columns() {
        assert_eq!(eval::<u64>(r#"return uni.width("abc")"#), 3);
        // Two fullwidth CJK ideographs occupy four columns.
        assert_eq!(eval::<u64>(r#"return uni.width("世界")"#), 4);
        assert_eq!(eval::<u64>(r#"return uni.width("")"#), 0);
        assert_eq!(eval::<u64>(r#"return uni.width("e\u{0301}")"#), 1);
    }

    #[test]
    fn invalid_utf8_is_refused_with_its_place() {
        assert!(refused(r#"return uni.fold("a\255")"#).contains("lib:unicode.fold: argument #1 's'"));
    }
}
