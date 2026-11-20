use crate::{error::DomainError, tool::QualifiedToolName, AppId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

/// What an agent may do to apps: which apps' tools it may call. (Apps themselves have no permissions: an app
/// is the person's own code on their own host.)
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "app", rename_all = "snake_case")]
pub enum ToolGrant {
    /// Agent may call any tool of this app (`*` handled by `CallAnyTool`).
    CallTools(AppId),
    CallAnyTool,
}

/// Per-agent identity resolved from the MCP bearer token; the unit of attribution and of what it may call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: AgentId,
    pub name: String,
    pub grants: Vec<ToolGrant>,
}

impl AgentIdentity {
    pub fn authorize_tool_call(&self, tool: &QualifiedToolName) -> Result<(), DomainError> {
        let allowed = self.grants.iter().any(|g| match g {
            ToolGrant::CallAnyTool => true,
            ToolGrant::CallTools(app) => app == tool.app(),
        });
        if allowed {
            Ok(())
        } else {
            Err(DomainError::PermissionDenied { agent: self.name.clone(), action: format!("call `{tool}`") })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(grants: Vec<ToolGrant>) -> AgentIdentity {
        AgentIdentity { id: AgentId("claude".into()), name: "Claude".into(), grants }
    }

    #[test]
    fn per_app_and_wildcard_grants() {
        let t = QualifiedToolName::new(AppId::parse("notes").unwrap(), "add_note").unwrap();
        assert!(agent(vec![ToolGrant::CallTools(AppId::parse("notes").unwrap())])
            .authorize_tool_call(&t)
            .is_ok());
        assert!(agent(vec![ToolGrant::CallAnyTool]).authorize_tool_call(&t).is_ok());
        assert!(agent(vec![ToolGrant::CallTools(AppId::parse("todo").unwrap())])
            .authorize_tool_call(&t)
            .is_err());
        assert!(agent(vec![]).authorize_tool_call(&t).is_err());
    }
}
