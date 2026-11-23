//! The one reader of an error string a module returned: `<module>.<function>: <kind>: <message>`.
//!
//! A fallible module function answers `(nil, err)` with `err` in that shape (`fs.read: not_found: /x: No such
//! file`). A module's `error_kind(err)` and `error_message(err)` both read it here, so the shape has one owner: a
//! script asks the module for the part it wants and never takes the string apart itself.

/// The parts of an error string of one module, after its `<module>.` prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorText<'a> {
    /// The function that failed (`read` in `fs.read: ...`).
    pub function: &'a str,
    /// The kind as written. The module decides whether it is one of its kinds.
    pub kind: &'a str,
    /// Everything after the kind, as the module wrote it; empty when the string ends at the kind.
    pub message: &'a str,
}

impl<'a> ErrorText<'a> {
    /// The parts of `err` when it is `<module>.<function>: <kind>[: <message>]`, with `<function>` a name of ASCII
    /// letters, digits and `_`. `None` for any other text.
    pub fn parse(module: &str, err: &'a str) -> Option<Self> {
        let rest = err.strip_prefix(module)?.strip_prefix('.')?;
        let (function, rest) = rest.split_once(": ")?;
        if function.is_empty()
            || !function
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }
        let (kind, message) = rest.split_once(": ").unwrap_or((rest, ""));
        Some(Self {
            function,
            kind,
            message,
        })
    }
}

/// The kind and the message of `err` when a function of the module `module` returned it: its function is a
/// `snake_case` name and `kind` knows its kind. `None` when the text is not one of that module's.
pub(crate) fn read<'a, K>(
    module: &str,
    err: &'a str,
    kind: impl FnOnce(&str) -> Option<K>,
) -> Option<(K, &'a str)> {
    let text = ErrorText::parse(module, err)?;
    if !text
        .function
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return None;
    }
    Some((kind(text.kind)?, text.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_function_kind_and_message() {
        assert_eq!(
            ErrorText::parse("fs", "fs.read: not_found: /x: No such file"),
            Some(ErrorText {
                function: "read",
                kind: "not_found",
                message: "/x: No such file"
            })
        );
        assert_eq!(
            ErrorText::parse(
                "files",
                "files.createDir: already_exists: notes: it is there"
            ),
            Some(ErrorText {
                function: "createDir",
                kind: "already_exists",
                message: "notes: it is there"
            })
        );
    }

    #[test]
    fn a_string_that_ends_at_the_kind_has_an_empty_message() {
        let text = ErrorText::parse("fs", "fs.read: other").expect("ours");
        assert_eq!((text.kind, text.message), ("other", ""));
        let text = ErrorText::parse("fs", "fs.read: other: ").expect("ours");
        assert_eq!((text.kind, text.message), ("other", ""));
    }

    #[test]
    fn parse_refuses_other_text() {
        for err in [
            "",
            "fs",
            "fs.",
            "fs.read",
            "fs.read:not_found",
            "fs.: not_found: x",
            "fs.re ad: not_found: x",
            "fsx.read: not_found: x",
            "hyper.get: timed_out: x",
            "not_found: x",
        ] {
            assert_eq!(ErrorText::parse("fs", err), None, "{err:?}");
        }
    }

    #[test]
    fn read_wants_a_snake_case_function_and_a_known_kind() {
        let known = |kind: &str| (kind == "not_found").then_some(7);
        assert_eq!(
            read("fs", "fs.read: not_found: /x: gone", known),
            Some((7, "/x: gone"))
        );
        assert_eq!(read("fs", "fs.read_dir: not_found", known), Some((7, "")));
        assert_eq!(read("fs", "fs.read: nonsense: /x", known), None);
        assert_eq!(read("fs", "fs.Read: not_found: /x", known), None);
        assert_eq!(read("fs", "fs.read2: not_found: /x", known), None);
        assert_eq!(read("fs", "bollard.ping: not_found: x", known), None);
    }
}
