use crate::{error::DomainError, tool::QualifiedToolName, AppId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

/// What an app may ask the host for (declared in its manifest) and what an agent may do to apps.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "app", rename_all = "snake_case")]
pub enum Permission {
    /// Agent may call any tool of this app (`*` handled by `CallAnyTool`).
    CallTools(AppId),
    CallAnyTool,
    /// App may send notifications.
    Notify,
    /// App may use its SQLite database.
    Data,
}

/// Per-agent identity resolved from the MCP bearer token; the unit of attribution and permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: AgentId,
    pub name: String,
    pub grants: Vec<Permission>,
}

impl AgentIdentity {
    pub fn authorize_tool_call(&self, tool: &QualifiedToolName) -> Result<(), DomainError> {
        let allowed = self.grants.iter().any(|g| match g {
            Permission::CallAnyTool => true,
            Permission::CallTools(app) => app == tool.app(),
            _ => false,
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

    fn agent(grants: Vec<Permission>) -> AgentIdentity {
        AgentIdentity { id: AgentId("claude".into()), name: "Claude".into(), grants }
    }

    #[test]
    fn per_app_and_wildcard_grants() {
        let t = QualifiedToolName::new(AppId::parse("notes").unwrap(), "add_note").unwrap();
        assert!(agent(vec![Permission::CallTools(AppId::parse("notes").unwrap())])
            .authorize_tool_call(&t)
            .is_ok());
        assert!(agent(vec![Permission::CallAnyTool]).authorize_tool_call(&t).is_ok());
        assert!(agent(vec![Permission::CallTools(AppId::parse("todo").unwrap())])
            .authorize_tool_call(&t)
            .is_err());
        assert!(agent(vec![]).authorize_tool_call(&t).is_err());
    }
}
