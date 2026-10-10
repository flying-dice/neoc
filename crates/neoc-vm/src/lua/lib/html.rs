//! HTML escape / unescape — `lib:html`.
//!
//! Minimal API: `escape` produces the 5 XML entities; `unescape` reverses
//! them plus numeric character references. Unknown entities and malformed
//! references pass through unchanged — `unescape` is a string transform,
//! not a validating parser. See docs/src/rfcs for the full design rationale.

use mlua::{Lua, Table};

use crate::lua::facade::Module;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:html` surface through the facade. Both functions are
/// infallible string transforms; the registered Lua surface and the
/// `lua::lib::html` spans are byte-identical to the hand-written registration
/// this replaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:html")?;
    m.func(
        "escape",
        &[("input", "string"), ("opts", "LuaValue?")],
        "Escape the five XML entities (`&`, `<`, `>`, `\"`, `'`) in `input`.",
        |_, (input, _opts): (mlua::String, Option<Table>)| Ok(escape_str(input.to_str()?.as_ref())),
    )?
    .func(
        "unescape",
        &[("input", "string")],
        "Reverse HTML entity references (named and numeric) in `input`; unknown entities pass through unchanged.",
        |_, input: mlua::String| Ok(unescape_str(input.to_str()?.as_ref())),
    )?;
    Ok(m)
}

// ---------------------------------------------------------------------------
// escape
// ---------------------------------------------------------------------------

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// unescape
// ---------------------------------------------------------------------------

// Longest legitimate body between `&` and `;` is `"x10FFFF"` (7 bytes) for
// the maximum hex Unicode form. 8 covers a one-byte overshoot
// (e.g. `"x110000"`) and bounds the search window, so a pathological
// input of `&` followed by a long run of non-`;` bytes stays linear.
// Longer numeric bodies (e.g. `"1114112"`) fall through the cap and emit
// as literal text — semantically equivalent to "out-of-range, passthrough".
const ENTITY_BODY_MAX: usize = 8;

fn unescape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let Some(amp) = rest.find('&') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];

        // Cap the scan window so unbalanced `&` followed by long runs of
        // non-`;` bytes does not degrade to O(n²).
        let scan_len = after_amp.len().min(ENTITY_BODY_MAX + 1);
        let semi = after_amp[..scan_len].find(';');

        match semi.and_then(|s| decode_entity(&after_amp[..s]).map(|c| (c, s))) {
            Some((ch, semi_pos)) => {
                out.push(ch);
                rest = &after_amp[semi_pos + 1..];
            }
            None => {
                // No `;` in window, or body is not a known/valid entity —
                // emit a literal `&` and resume scanning past it. Anything
                // we skipped over will be re-examined naturally.
                out.push('&');
                rest = after_amp;
            }
        }
    }
    out
}

fn decode_entity(body: &str) -> Option<char> {
    match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => decode_numeric(body),
    }
}

fn decode_numeric(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let (radix, num) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (16u32, hex),
        None => (10u32, digits),
    };
    if num.is_empty() {
        return None;
    }
    // Validate strict digit set up-front — `u32::from_str_radix` accepts a
    // leading `+` (e.g. `&#+5;` would otherwise decode to `\x05`), which
    // diverges from Python `html`, Go `html`, and Deno `@std/html` (all
    // passthrough). Reject anything outside the radix's digit set.
    let all_digits = num.bytes().all(|b| match radix {
        16 => b.is_ascii_hexdigit(),
        _ => b.is_ascii_digit(),
    });
    if !all_digits {
        return None;
    }
    let n = u32::from_str_radix(num, radix).ok()?;
    // `char::from_u32` already filters surrogates (D800..=DFFF) and
    // anything beyond U+10FFFF. NUL is a valid scalar in Rust but
    // explicitly excluded here to prevent NUL injection per the spec.
    if n == 0 {
        return None;
    }
    char::from_u32(n)
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    /// Test-only engine. Binds `html = require("lib:html")` as a global to
    /// keep inline test bodies focused on behaviour, not require plumbing
    /// (covered separately).
    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:html")"#).eval().unwrap();
        lua.globals().set("html", m).unwrap();
        lua
    }

    #[test]
    fn escape_all_five_entities() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.escape([[<script>alert("xss")</script>]])"#)
            .eval()
            .unwrap();
        assert_eq!(s, r#"&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"#);
    }

    #[test]
    fn escape_apostrophe() {
        let lua = engine();
        let s: String = lua.load(r#"return html.escape("it's")"#).eval().unwrap();
        assert_eq!(s, "it&apos;s");
    }

    #[test]
    fn escape_empty_is_empty() {
        let lua = engine();
        let s: String = lua.load(r#"return html.escape("")"#).eval().unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn escape_preserves_non_ascii() {
        let lua = engine();
        let s: String = lua.load(r#"return html.escape("café 🦀")"#).eval().unwrap();
        assert_eq!(s, "café 🦀");
    }

    #[test]
    fn escape_opts_table_ignored() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.escape("<b>", { context = "attribute", unknown = true })"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&lt;b&gt;");
    }

    #[test]
    fn escape_double_escape_preserves_round_trip() {
        // "&amp;" -> "&amp;amp;" -> unescape -> "&amp;"
        let lua = engine();
        let s: String = lua.load(r#"return html.escape("&amp;")"#).eval().unwrap();
        assert_eq!(s, "&amp;amp;");
        let r: String = lua
            .load(r#"return html.unescape(html.escape("&amp;"))"#)
            .eval()
            .unwrap();
        assert_eq!(r, "&amp;");
    }

    #[test]
    fn unescape_named_entities() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&lt;b&gt;bold&lt;/b&gt;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "<b>bold</b>");
    }

    #[test]
    fn unescape_unknown_named_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&notareal;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&notareal;");
    }

    #[test]
    fn unescape_copy_unsupported_named_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&copy;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&copy;");
    }

    #[test]
    fn unescape_decimal_reference() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&#65;")"#).eval().unwrap();
        assert_eq!(s, "A");
    }

    #[test]
    fn unescape_hex_reference_lower_and_upper_x() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#x41;&#X41;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "AA");
    }

    #[test]
    fn unescape_nul_passthrough() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&#0;")"#).eval().unwrap();
        assert_eq!(s, "&#0;");
    }

    #[test]
    fn unescape_surrogate_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#xD800;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&#xD800;");
    }

    #[test]
    fn unescape_out_of_range_decimal_passthrough() {
        // 1114112 = 0x110000 — the first codepoint past U+10FFFF.
        // The issue spec table cites `&#999999;` as "out-of-range" but
        // 999999 = 0xF423F is a valid Unicode scalar (Supplementary
        // Private Use Area-A). Spec table is wrong; standard runtimes
        // (Python `html`, Go `html`, Deno `@std/html`) decode it. Flagged
        // on MR for the author to amend.
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#1114112;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&#1114112;");
    }

    #[test]
    fn unescape_decimal_in_private_use_area_decodes() {
        // Counter-test for the spec correction above: 999999 = U+F423F is
        // a valid scalar (PUA-A) and must decode, not pass through.
        let lua = engine();
        let s: mlua::String = lua
            .load(r#"return html.unescape("&#999999;")"#)
            .eval()
            .unwrap();
        assert_eq!(s.as_bytes().as_ref(), "\u{F423F}".as_bytes());
    }

    #[test]
    fn unescape_beyond_unicode_max_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#x110000;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&#x110000;");
    }

    #[test]
    fn unescape_malformed_empty_numeric_passthrough() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&#;")"#).eval().unwrap();
        assert_eq!(s, "&#;");
    }

    #[test]
    fn unescape_malformed_empty_hex_passthrough() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&#x;")"#).eval().unwrap();
        assert_eq!(s, "&#x;");
    }

    #[test]
    fn unescape_invalid_hex_digits_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#xGG;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&#xGG;");
    }

    #[test]
    fn unescape_leading_plus_decimal_passthrough() {
        // `u32::from_str_radix` accepts a leading `+`; Python `html`, Go
        // `html`, and Deno `@std/html` do not. Match standard runtimes —
        // passthrough.
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&#+5;")"#).eval().unwrap();
        assert_eq!(s, "&#+5;");
    }

    #[test]
    fn unescape_leading_plus_hex_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&#x+5;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&#x+5;");
    }

    #[test]
    fn unescape_missing_semicolon_passthrough() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("&amp")"#).eval().unwrap();
        assert_eq!(s, "&amp");
    }

    #[test]
    fn unescape_space_after_ampersand_passthrough() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("& amp;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "& amp;");
    }

    #[test]
    fn unescape_idempotence_note_double_amp_lt() {
        // Documented non-idempotence: "&amp;lt;" -> "&lt;" -> "<".
        // Each call to `unescape` performs exactly one entity expansion pass.
        let lua = engine();
        let once: String = lua
            .load(r#"return html.unescape("&amp;lt;")"#)
            .eval()
            .unwrap();
        assert_eq!(once, "&lt;");
        let twice: String = lua
            .load(r#"return html.unescape(html.unescape("&amp;lt;"))"#)
            .eval()
            .unwrap();
        assert_eq!(twice, "<");
    }

    #[test]
    fn round_trip_invariant_holds() {
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape(html.escape([[<>&"']]))"#)
            .eval()
            .unwrap();
        assert_eq!(s, r#"<>&"'"#);
    }

    #[test]
    fn unescape_long_body_falls_through_as_literal() {
        // Body exceeds ENTITY_BODY_MAX so the `;` is outside the scan
        // window — we emit a literal `&` and resume past it.
        let lua = engine();
        let s: String = lua
            .load(r#"return html.unescape("&abcdefghijk;")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "&abcdefghijk;");
    }

    #[test]
    fn unescape_bare_ampersand_at_end_passthrough() {
        let lua = engine();
        let s: String = lua.load(r#"return html.unescape("foo&")"#).eval().unwrap();
        assert_eq!(s, "foo&");
    }
}
