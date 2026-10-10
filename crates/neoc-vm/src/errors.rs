use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum NeocError {
    #[error("{message}")]
    #[diagnostic(code(neoc::E0001), url("https://neoc.dev/errors/E0001"))]
    ModuleNotFound {
        message: String,
        #[help]
        suggestion: Option<String>,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("module required here")]
        span: miette::SourceSpan,
    },

    #[error("embedded payload corrupt")]
    #[diagnostic(code(neoc::E0002), url("https://neoc.dev/errors/E0002"))]
    CorruptPayload {
        #[help]
        detail: String,
    },

    #[error("engine initialisation failed: {0}")]
    #[diagnostic(code(neoc::E0003), url("https://neoc.dev/errors/E0003"))]
    EngineInit(String),

    #[error("{0}")]
    #[diagnostic()]
    Runtime(String),
}

pub fn extract_require_error(error: &mlua::Error, source: &str, path: &str) -> Option<NeocError> {
    let msg = format!("{error}");

    let idx = msg.find("require: unknown module")?;
    let inner = &msg[idx..];

    let (suggestion, available) = parse_require_message(inner);

    let mut help_parts = Vec::new();
    if let Some(ref s) = suggestion {
        help_parts.push(s.clone());
    }
    if let Some(ref a) = available {
        help_parts.push(a.clone());
    }
    let help = if help_parts.is_empty() {
        None
    } else {
        Some(help_parts.join("\n"))
    };

    let span = find_require_span(source, &msg);

    Some(NeocError::ModuleNotFound {
        message: inner.split(" — ").next().unwrap_or(inner).to_string(),
        suggestion: help,
        src: miette::NamedSource::new(path, source.to_string()),
        span,
    })
}

fn parse_require_message(msg: &str) -> (Option<String>, Option<String>) {
    let suggestion = msg.find("did you mean ").map(|i| {
        let rest = &msg[i..];
        rest.find('?')
            .map(|end| rest[..=end].to_string())
            .unwrap_or_else(|| rest.to_string())
    });

    let available = msg.find("available: ").map(|i| msg[i..].to_string());

    (suggestion, available)
}

fn find_require_span(source: &str, traceback: &str) -> miette::SourceSpan {
    let Some(line_num) = extract_line_number(traceback) else {
        return (0, 0).into();
    };
    let mut offset = 0;
    for (i, line) in source.split_inclusive('\n').enumerate() {
        if i + 1 == line_num {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(col) = trimmed.find("require") {
                let len = trimmed[col..]
                    .find(')')
                    .map(|e| e + 1)
                    .unwrap_or(trimmed.len() - col);
                return (offset + col, len).into();
            }
            return (offset, trimmed.len()).into();
        }
        offset += line.len();
    }
    (0, 0).into()
}

fn extract_line_number(traceback: &str) -> Option<usize> {
    // Anchor on the mlua/Lua chunk marker `]:LINE:`. The chunk name is
    // wrapped in `[string "..."]`, so `]:` reliably terminates it.
    let after = traceback.split("]:").nth(1)?;
    let line_str = after.split(':').next()?;
    line_str.trim().parse::<usize>().ok().filter(|&n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_line_number_parses_mlua_traceback() {
        let tb = r#"runtime error: [string "test.lua"]:3: require: unknown module"#;
        assert_eq!(extract_line_number(tb), Some(3));
    }

    #[test]
    fn extract_line_number_returns_none_when_no_chunk_marker() {
        assert_eq!(extract_line_number("no traceback here"), None);
    }

    #[test]
    fn extract_line_number_ignores_leading_numbers_in_message() {
        // Bare number anywhere — without `]:` anchor — must not match.
        assert_eq!(extract_line_number("port 8080: connection refused"), None);
    }

    #[test]
    fn extract_line_number_handles_chunk_with_numeric_name() {
        let tb = r#"[string "8080.lua"]:42: boom"#;
        assert_eq!(extract_line_number(tb), Some(42));
    }

    #[test]
    fn parse_require_message_extracts_did_you_mean() {
        let msg = r#"require: unknown module "lib:jsn" — did you mean "lib:json"? available: lib:json, lib:base64"#;
        let (sug, avail) = parse_require_message(msg);
        assert_eq!(sug.as_deref(), Some(r#"did you mean "lib:json"?"#));
        assert_eq!(avail.as_deref(), Some("available: lib:json, lib:base64"));
    }

    #[test]
    fn parse_require_message_no_match_returns_none() {
        let (sug, avail) = parse_require_message(r#"require: unknown module "foo""#);
        assert!(sug.is_none());
        assert!(avail.is_none());
    }

    #[test]
    fn find_require_span_lf_source_points_at_require_call() {
        let src = "local x = 1\nrequire(\"lib:jsn\")\nprint(x)\n";
        let tb = r#"[string "t.lua"]:2: require: unknown"#;
        let span: miette::SourceSpan = find_require_span(src, tb);
        let start = span.offset();
        let len = span.len();
        let slice = &src[start..start + len];
        assert_eq!(slice, r#"require("lib:jsn")"#);
    }

    #[test]
    fn find_require_span_crlf_source_accounts_for_carriage_return() {
        let src = "local x = 1\r\nrequire(\"lib:jsn\")\r\nprint(x)\r\n";
        let tb = r#"[string "t.lua"]:2: require: unknown"#;
        let span: miette::SourceSpan = find_require_span(src, tb);
        let start = span.offset();
        let len = span.len();
        let slice = &src[start..start + len];
        assert_eq!(slice, r#"require("lib:jsn")"#);
    }

    #[test]
    fn find_require_span_returns_zero_when_no_line_number() {
        let span: miette::SourceSpan = find_require_span("anything", "no chunk here");
        assert_eq!(span.offset(), 0);
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn extract_require_error_skips_non_require_messages() {
        let err = mlua::Error::external("totally unrelated boom");
        assert!(extract_require_error(&err, "src", "p").is_none());
    }

    #[test]
    fn extract_require_error_builds_module_not_found_with_help() {
        let raw =
            r#"require: unknown module "lib:jsn" — did you mean "lib:json"? available: lib:json"#;
        let err = mlua::Error::external(raw);
        let src = "require(\"lib:jsn\")\n";
        let neoc = extract_require_error(&err, src, "t.lua").expect("should classify");
        let NeocError::ModuleNotFound {
            message,
            suggestion,
            ..
        } = neoc
        else {
            panic!("expected ModuleNotFound");
        };
        assert!(message.starts_with(r#"require: unknown module "lib:jsn""#));
        let help = suggestion.expect("help present");
        assert!(help.contains(r#"did you mean "lib:json"?"#));
        assert!(help.contains("available: lib:json"));
    }
}
