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

    let inner = if let Some(idx) = msg.find("require: unknown module") {
        &msg[idx..]
    } else {
        return None;
    };

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
    if let Some(line_num) = extract_line_number(traceback) {
        let mut offset = 0;
        for (i, line) in source.lines().enumerate() {
            if i + 1 == line_num {
                if let Some(col) = line.find("require") {
                    let len = line[col..]
                        .find(')')
                        .map(|e| e + 1)
                        .unwrap_or(line.len() - col);
                    return (offset + col, len).into();
                }
                return (offset, line.len()).into();
            }
            offset += line.len() + 1;
        }
    }
    (0, 0).into()
}

fn extract_line_number(traceback: &str) -> Option<usize> {
    for part in traceback.split(':') {
        if let Ok(n) = part.trim().parse::<usize>()
            && n > 0
        {
            return Some(n);
        }
    }
    None
}
