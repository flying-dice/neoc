//! The VM's error type, and the classifier that turns a raw `require` failure into a typed one.

use thiserror::Error;

/// What the VM reports to its host.
#[derive(Error, Debug)]
pub enum LuaVmError {
    /// A script `require`d a module that is not registered.
    #[error("{message}")]
    ModuleNotFound {
        message: String,
        /// "did you mean ...?" and the list of available modules, when the registry offered them.
        suggestion: Option<String>,
    },

    /// The engine could not be built (library open, global stripping, module registration).
    #[error("engine initialisation failed: {0}")]
    EngineInit(String),

    /// A script failed at run time.
    #[error("{0}")]
    Runtime(String),
}

/// Classify `error` as [`LuaVmError::ModuleNotFound`] when it is the strict `require`'s "unknown module"
/// failure; `None` for every other error.
#[must_use]
pub fn extract_require_error(error: &mlua::Error) -> Option<LuaVmError> {
    let msg = error.to_string();
    let idx = msg.find("require: unknown module")?;
    let inner = &msg[idx..];

    let (suggestion, available) = parse_require_message(inner);
    let help: Vec<String> = suggestion.into_iter().chain(available).collect();
    let suggestion = if help.is_empty() { None } else { Some(help.join("\n")) };

    Some(LuaVmError::ModuleNotFound {
        message: inner.split(" — ").next().unwrap_or(inner).to_string(),
        suggestion,
    })
}

fn parse_require_message(msg: &str) -> (Option<String>, Option<String>) {
    let suggestion = msg.find("did you mean ").map(|i| {
        let rest = &msg[i..];
        rest.find('?').map_or_else(|| rest.to_string(), |end| rest[..=end].to_string())
    });
    let available = msg.find("available: ").map(|i| msg[i..].to_string());
    (suggestion, available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_require_message_extracts_did_you_mean() {
        let msg =
            r#"require: unknown module "lib:jsn" — did you mean "lib:json"? available: lib:json, lib:base64"#;
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
    fn extract_require_error_skips_non_require_messages() {
        let err = mlua::Error::external("totally unrelated boom");
        assert!(extract_require_error(&err).is_none());
    }

    #[test]
    fn extract_require_error_builds_module_not_found_with_help() {
        let raw = r#"require: unknown module "lib:jsn" — did you mean "lib:json"? available: lib:json"#;
        let err = mlua::Error::external(raw);
        let Some(LuaVmError::ModuleNotFound { message, suggestion }) = extract_require_error(&err) else {
            panic!("expected ModuleNotFound");
        };
        assert!(message.starts_with(r#"require: unknown module "lib:jsn""#));
        let help = suggestion.expect("help present");
        assert!(help.contains(r#"did you mean "lib:json"?"#));
        assert!(help.contains("available: lib:json"));
    }
}
