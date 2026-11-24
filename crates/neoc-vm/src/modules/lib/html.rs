//! HTML escape and unescape: `lib:html`.
//!
//! `escape` writes the five entities (`&amp;` `&lt;` `&gt;` `&quot;` `&apos;`); `unescape` reads them back, and
//! numeric character references (`&#65;`, `&#x41;`). An entity that is not one of the five and a malformed
//! reference pass through unchanged: `unescape` is a string transform, not a validating parser.

/// The longest body between `&` and `;` that can be an entity is `x10FFFF` (7 bytes). 8 covers a one-byte overshoot
/// (`x110000`) and bounds the search, so a `&` followed by a long run without `;` stays linear. A longer numeric
/// body (`1114112`) is past the window and passes through, which is what out of range means anyway.
const ENTITY_BODY_MAX: usize = 8;

fn escape_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
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

fn unescape_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        // The window may end inside a multi-byte character (`AT&T Ünïcödé`); a `str` cannot be cut there.
        let window = &after[..after.floor_char_boundary(ENTITY_BODY_MAX + 1)];
        let entity = window.find(';').and_then(|end| Some((decode_entity(&after[..end])?, end)));
        match entity {
            Some((ch, end)) => {
                out.push(ch);
                rest = &after[end + 1..];
            }
            // No `;` in the window, or not an entity: a literal `&`, and what follows is looked at again.
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
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
    let (radix, number) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (16, hex),
        None => (10, digits),
    };
    // `from_str_radix` takes a leading `+` (`&#+5;`); Python's, Go's and Deno's `html` do not, and pass it through.
    if number.is_empty() || !number.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    // `char::from_u32` refuses surrogates and anything past U+10FFFF. NUL is a character, and is refused here: a
    // reference must not smuggle one in.
    match u32::from_str_radix(number, radix).ok()? {
        0 => None,
        code => char::from_u32(code),
    }
}

#[mlua_ir::module("lib:html")]
pub mod html {
    use super::{escape_str, unescape_str};

    /// `text` with `&`, `<`, `>`, `"` and `'` written as entities: safe as HTML text and inside a quoted attribute.
    ///
    /// ```luau
    /// assert(html.escape('<a href="x">it\'s</a>') == "&lt;a href=&quot;x&quot;&gt;it&apos;s&lt;/a&gt;")
    /// ```
    pub fn escape(text: String) -> String {
        escape_str(&text)
    }

    /// `text` with the five entities and numeric references (`&#65;`, `&#x41;`) read back, in one pass. Anything
    /// else that starts with `&` passes through unchanged: other named entities (`&copy;`), a reference to NUL, to
    /// a surrogate or past U+10FFFF, one without its `;`.
    ///
    /// ```luau
    /// assert(html.unescape("&lt;b&gt; &#65;&#x42; &copy; &amp;lt;") == "<b> AB &copy; &lt;")
    /// ```
    pub fn unescape(text: String) -> String {
        unescape_str(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::{escape_str, unescape_str};

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:html")"#).eval().unwrap();
        lua.globals().set("html", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn escape_writes_the_five_entities() {
        for (text, escaped) in [
            (r#"<script>alert("xss")</script>"#, "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"),
            ("it's", "it&apos;s"),
            ("", ""),
            ("hello", "hello"),
            ("café 🦀", "café 🦀"),
            ("&amp;", "&amp;amp;"),
            ("<b>", "&lt;b&gt;"),
        ] {
            assert_eq!(escape_str(text), escaped, "escape({text:?})");
        }
    }

    #[test]
    fn unescape_reads_the_five_entities_and_numeric_references() {
        for (text, unescaped) in [
            ("&amp;&lt;&gt;&quot;&apos;", r#"&<>"'"#),
            ("&lt;b&gt;bold&lt;/b&gt;", "<b>bold</b>"),
            ("&#65;", "A"),
            ("&#x41;&#X41;", "AA"),
            ("&#x41;&#65;", "AA"),
            // 999999 is U+F423F, a private-use character: a valid scalar, so it is read
            ("&#999999;", "\u{F423F}"),
            ("&#x10FFFF;", "\u{10FFFF}"),
            ("&#x1F980;", "🦀"),
        ] {
            assert_eq!(unescape_str(text), unescaped, "unescape({text:?})");
        }
    }

    #[test]
    fn unescape_passes_through_what_is_not_an_entity() {
        for text in [
            "&notareal;",
            // a real HTML entity, outside the five on purpose
            "&copy;",
            // NUL: no injection through a reference
            "&#0;",
            // a surrogate
            "&#xD800;",
            // 1114112 is 0x110000, the first number past U+10FFFF
            "&#1114112;",
            "&#x110000;",
            "&#;",
            "&#x;",
            "&#xGG;",
            // `from_str_radix` would take the sign
            "&#+5;",
            "&#x+5;",
            "&#-5;",
            // no `;`, a space after `&`, a lone `&`
            "&amp",
            "& amp;",
            "foo&",
            "&",
            // the body is longer than the window
            "&abcdefghijk;",
            "&ÜÜÜÜÜÜÜÜ;",
            "plain text",
            "",
        ] {
            assert_eq!(unescape_str(text), text, "unescape({text:?})");
        }
    }

    #[test]
    fn unescape_is_utf8_safe_when_the_scan_window_ends_inside_a_character() {
        // `&` followed by multi-byte characters so that byte 9 after it is inside one, in every alignment
        for pad in 0..4 {
            let text = format!("AT&T {}Ünïcödé 🦀🦀🦀 &amp; &#x1F980;", "x".repeat(pad));
            let want = format!("AT&T {}Ünïcödé 🦀🦀🦀 & 🦀", "x".repeat(pad));
            assert_eq!(unescape_str(&text), want, "pad {pad}");
        }
    }

    #[test]
    fn unescape_is_linear_in_ampersands_without_a_semicolon() {
        let text = "&".repeat(200_000) + &"a".repeat(200_000);
        assert_eq!(unescape_str(&text), text);
    }

    /// Each call reads one layer: `&amp;lt;` is `&lt;`, then `<`. That is correct, not something to fix.
    #[test]
    fn unescape_reads_one_layer_per_call() {
        assert_eq!(unescape_str("&amp;lt;"), "&lt;");
        assert_eq!(unescape_str(&unescape_str("&amp;lt;")), "<");
    }

    #[test]
    fn what_escape_wrote_reads_back() {
        for text in ["hello", r#"<>&"'"#, "", "&amp;", "café", "🦀", "a & b < c > d \"e\" 'f'", "&#65;"] {
            assert_eq!(unescape_str(&escape_str(text)), text, "{text:?}");
        }
    }

    #[test]
    fn the_module_escapes_and_unescapes() {
        let (escaped, unescaped, round_trip): (String, String, String) = eval(
            r#"
            return html.escape([[<script>alert("xss")</script>]]),
                html.unescape("&lt;b&gt;bold&lt;/b&gt; &#65;&#x41; &copy; &amp;lt;"),
                html.unescape(html.escape([[<>&"' café 🦀]]))
            "#,
        );
        assert_eq!(escaped, "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;");
        assert_eq!(unescaped, "<b>bold</b> AA &copy; &lt;");
        assert_eq!(round_trip, r#"<>&"' café 🦀"#);
    }

    #[test]
    fn arguments_are_one_text() {
        let (options, number, bytes): (String, String, String) = eval(
            r#"
            local _, options = pcall(html.escape, "<b>", { context = "attribute" })
            local _, number = pcall(html.escape, 42)
            local _, bytes = pcall(html.unescape, "\255")
            assert(not pcall(html.unescape, nil) and not pcall(html.escape))
            return tostring(options), tostring(number), tostring(bytes)
            "#,
        );
        assert!(
            options.contains("lib:html.escape") && options.contains("takes 1 argument"),
            "got: {options}"
        );
        assert!(number.contains("lib:html.escape: argument #1 'text'"), "got: {number}");
        assert!(bytes.contains("lib:html.unescape: argument #1 'text'"), "got: {bytes}");
    }
}
