//! Identifier conversions between SQL names and neoc names.
//!
//! Assumptions (see `ASSUMPTIONS.md`): column names are assumed to be valid
//! neoc identifiers once cased. Query names arrive as the `dml` file stem.

/// `UserByName` / `user_by_name` → `user_by_name`. Lowers a name to `snake_case`,
/// inserting `_` at camelCase humps.
pub(crate) fn snake(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if ch == '_' {
            out.push('_');
            prev_lower = false;
        } else if ch.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower = false;
        } else {
            out.push(ch);
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    out
}

/// `user_by_name` / `UserByName` → `UserByName`. Upper-camel-cases a name,
/// treating both `_` and existing humps as word boundaries.
pub(crate) fn pascal(name: &str) -> String {
    let mut out = String::new();
    let mut upper_next = true;
    for ch in name.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Makes `name` a valid neoc identifier: non-`[A-Za-z0-9_]` characters (from
/// quoted SQL identifiers) become `_`, a leading digit gets a `_` prefix, and a
/// reserved neoc keyword gets a trailing `_`. Deterministic, so the same source
/// name sanitises identically at a struct field, a decoder binding, and a
/// function parameter.
pub(crate) fn sanitize_ident(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('_');
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if is_keyword(&s) {
        s.push('_');
    }
    s
}

/// The neoc reserved words (`LEXICAL.md` §5), including the reserved-for-future
/// set and the boolean literals — none may be used as a bare identifier.
fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "else"
            | "enum"
            | "ext"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "pub"
            | "return"
            | "self"
            | "Self"
            | "trait"
            | "type"
            | "use"
            | "where"
            | "while"
            | "true"
            | "false"
            | "async"
            | "await"
            | "crate"
            | "dyn"
            | "move"
            | "static"
            | "super"
            | "unsafe"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_from_camel_and_snake() {
        assert_eq!(snake("UserByName"), "user_by_name");
        assert_eq!(snake("user_by_name"), "user_by_name");
        assert_eq!(snake("AllUsers"), "all_users");
    }

    #[test]
    fn pascal_from_snake_and_camel() {
        assert_eq!(pascal("user_by_name"), "UserByName");
        assert_eq!(pascal("UserByName"), "UserByName");
    }

    #[test]
    fn sanitize_handles_keywords_digits_and_punctuation() {
        assert_eq!(sanitize_ident("type"), "type_", "neoc keyword");
        assert_eq!(sanitize_ident("match"), "match_", "neoc keyword");
        assert_eq!(
            sanitize_ident("mut"),
            "mut",
            "mut is an ordinary identifier again (#69)"
        );
        assert_eq!(sanitize_ident("name"), "name", "ordinary name is untouched");
        assert_eq!(sanitize_ident("2fa"), "_2fa", "leading digit");
        assert_eq!(
            sanitize_ident("first name"),
            "first_name",
            "quoted-id space"
        );
    }
}
