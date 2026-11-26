//! Who is acting: the human at a client, an agent, or another app. Attribution for writes (`ctx.actor` in Lua),
//! revisions and notifications.

use crate::AgentIdentity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
    /// Another app's backend (`app.request`, or delivering its notification to a listener).
    App,
}

impl ActorKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::App => "app",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
    pub name: String,
}

impl Actor {
    /// The human at a client. `name` is what the app stores as the author (`"You"` by default).
    #[must_use]
    pub fn user(name: &str) -> Self {
        Self { kind: ActorKind::User, id: "user".to_owned(), name: name.to_owned() }
    }

    #[must_use]
    pub fn agent(id: &str, name: &str) -> Self {
        Self { kind: ActorKind::Agent, id: id.to_owned(), name: name.to_owned() }
    }

    /// Another app's backend: its id is the app's id, and so is its name.
    #[must_use]
    pub fn app(id: &str) -> Self {
        Self { kind: ActorKind::App, id: id.to_owned(), name: id.to_owned() }
    }

    #[must_use]
    pub fn is_user(&self) -> bool {
        self.kind == ActorKind::User
    }
}

impl From<&AgentIdentity> for Actor {
    fn from(agent: &AgentIdentity) -> Self {
        Self::agent(&agent.id.0, &agent.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentId, ToolGrant};

    #[test]
    fn an_agent_identity_becomes_an_agent_actor() {
        let identity = AgentIdentity {
            id: AgentId("claude".into()),
            name: "Claude".into(),
            grants: vec![ToolGrant::CallAnyTool],
        };
        let actor = Actor::from(&identity);
        assert_eq!(
            (actor.kind, actor.id.as_str(), actor.name.as_str()),
            (ActorKind::Agent, "claude", "Claude")
        );
        assert!(!actor.is_user());
        assert!(Actor::user("You").is_user());
    }
}
