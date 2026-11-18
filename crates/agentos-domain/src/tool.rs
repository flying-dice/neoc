use crate::{error::DomainError, AppId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// A tool as declared by an app (`app.tools.register{...}`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    pub input_schema: Value,
    /// The tool does not change data (`readOnly = true` in Luau): shown to agents as a hint.
    #[serde(default)]
    pub read_only: bool,
}

/// How tools are exposed over MCP: `<app>__<tool>` (MCP tool names allow `[a-zA-Z0-9_-]`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedToolName {
    app: AppId,
    tool: String,
}

impl QualifiedToolName {
    pub fn new(app: AppId, tool: &str) -> Result<Self, DomainError> {
        let ok = !tool.is_empty()
            && tool.len() <= 48
            && tool.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
        if ok {
            Ok(Self { app, tool: tool.to_owned() })
        } else {
            Err(DomainError::InvalidToolName(tool.to_owned()))
        }
    }
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let (app, tool) = s.split_once("__").ok_or_else(|| DomainError::InvalidToolName(s.to_owned()))?;
        Self::new(AppId::parse(app).map_err(|_| DomainError::InvalidToolName(s.to_owned()))?, tool)
    }
    pub fn app(&self) -> &AppId {
        &self.app
    }
    pub fn tool(&self) -> &str {
        &self.tool
    }
}
impl fmt::Display for QualifiedToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}__{}", self.app, self.tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let q = QualifiedToolName::parse("notes__add_note").unwrap();
        assert_eq!((q.app().as_str(), q.tool()), ("notes", "add_note"));
        assert_eq!(q.to_string(), "notes__add_note");
        assert!(QualifiedToolName::parse("nodelimiter").is_err());
        assert!(QualifiedToolName::parse("notes__bad name").is_err());
        let dotted = QualifiedToolName::parse("notes__notes.append").unwrap();
        assert_eq!((dotted.app().as_str(), dotted.tool()), ("notes", "notes.append"));
        assert!(QualifiedToolName::parse("notes__a/b").is_err());
    }
}
