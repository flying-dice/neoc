use crate::{error::DomainError, AppId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// A tool as declared by an app (`app.tool { ... }`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    pub input_schema: Value,
    /// The tool does not change data (`readOnly = true` in Lua): shown to agents as a hint.
    #[serde(default)]
    pub read_only: bool,
}

/// How tools are exposed over MCP: `<app>__<tool>`; the tool is a local identifier `[a-z][a-z0-9_]*`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedToolName {
    app: AppId,
    tool: String,
}

impl QualifiedToolName {
    pub fn new(app: AppId, tool: &str) -> Result<Self, DomainError> {
        if Self::is_valid_tool_name(tool) {
            Ok(Self { app, tool: tool.to_owned() })
        } else {
            Err(DomainError::InvalidToolName(tool.to_owned()))
        }
    }
    /// A tool name is a local identifier: `[a-z][a-z0-9_]*`, at most 48 characters. The app id (which cannot
    /// contain `_`) and `__` make the MCP name unambiguous; dots and dashes are not allowed.
    #[must_use]
    pub fn is_valid_tool_name(tool: &str) -> bool {
        let mut bytes = tool.bytes();
        tool.len() <= 48
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
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
        assert!(QualifiedToolName::parse("notes__a/b").is_err());
        for bad in ["notes__notes.append", "notes__Append", "notes__1st", "notes__a-b", "notes__", "notes__é"]
        {
            assert!(QualifiedToolName::parse(bad).is_err(), "{bad} must be rejected");
        }
        let ok = QualifiedToolName::parse("notes__append_text2").unwrap();
        assert_eq!((ok.app().as_str(), ok.tool()), ("notes", "append_text2"));
    }
}
