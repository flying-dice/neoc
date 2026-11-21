//! Agents as contacts: who they are, how the host launches them, and what they may do.
//!
//! The default is **deny**: a new agent has no grants, so it sees no tools until the person allows apps (an
//! explicit allow-list, or the explicit "all apps" option). Its approval policy defaults to
//! [`ApprovalPolicy::AutoReadOnly`]: a request to run a tool that cannot write is allowed without a prompt,
//! everything else asks the person.

use crate::{AgentId, AgentIdentity, AppId, DomainError, ToolGrant};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// How the host starts an agent: any program that speaks ACP over stdio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntime {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Which apps' tools an agent may call. Deny by default: `Grants::default()` allows nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    /// The explicit "all apps" option, including apps installed later.
    #[serde(default)]
    pub all: bool,
    /// An allow-list of app ids.
    #[serde(default)]
    pub apps: Vec<AppId>,
}

impl Grants {
    #[must_use]
    pub fn allow_all() -> Self {
        Self { all: true, apps: vec![] }
    }

    #[must_use]
    pub fn apps(apps: impl IntoIterator<Item = AppId>) -> Self {
        let mut grants = Self { all: false, apps: apps.into_iter().collect() };
        grants.normalize();
        grants
    }

    /// Sorted and deduplicated, so equal sets compare equal.
    pub fn normalize(&mut self) {
        self.apps.sort();
        self.apps.dedup();
    }

    #[must_use]
    pub fn allows(&self, app: &AppId) -> bool {
        self.all || self.apps.contains(app)
    }

    /// The permissions an [`AgentIdentity`] carries for these grants.
    #[must_use]
    pub fn permissions(&self) -> Vec<ToolGrant> {
        if self.all {
            vec![ToolGrant::CallAnyTool]
        } else {
            self.apps.iter().cloned().map(ToolGrant::CallTools).collect()
        }
    }
}

/// What the host does with an agent's ACP permission request about an `AgentOS` tool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    /// Always ask the person.
    Ask,
    /// Allow a tool its app declared `readOnly` (the host stops it writing through the `app.*` API; it does not
    /// stop the app's own `std:fs` or `vnd:rusqlite` code, so this trusts the app's declaration) when the request is exactly
    /// `mcp__vos__<app>__<tool>`, the tool exists, the agent is granted the app and the agent's kind for the
    /// call is `read`, `search` or `fetch` (any other kind, or none, asks). It answers with allow-once, never a
    /// standing allow. Ask for everything else.
    #[default]
    AutoReadOnly,
    /// Never ask: **every** permission request is allowed, whatever it is for (a shell command, a file edit, a
    /// delete, as well as an `AgentOS` tool). It is not limited to app tools; grants still bound which app tools
    /// the agent can see. The stored and wire value stays `allow_all`.
    #[serde(rename = "allow_all")]
    AllowEverything,
}

impl ApprovalPolicy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AutoReadOnly => "auto_read_only",
            Self::AllowEverything => "allow_all",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ask" => Some(Self::Ask),
            "auto_read_only" => Some(Self::AutoReadOnly),
            "allow_all" => Some(Self::AllowEverything),
            _ => None,
        }
    }
}

/// An agent contact. The MCP bearer token is not part of it: only its hash is stored, apart from this record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default)]
    pub role: String,
    pub runtime: AgentRuntime,
    #[serde(default)]
    pub grants: Grants,
    #[serde(default)]
    pub approval: ApprovalPolicy,
}

impl AgentId {
    /// Lowercase letters, digits and `-`, 1 to 40 characters, not starting with `-`.
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let ok = (1..=40).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !s.starts_with('-');
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(DomainError::InvalidAgent(format!("id `{s}` must be 1-40 characters of a-z, 0-9 and `-`")))
        }
    }

    /// An id from a display name: `"Code Reviewer"` becomes `code-reviewer`.
    #[must_use]
    pub fn slug(name: &str) -> String {
        let mut out = String::new();
        for c in name.chars().flat_map(char::to_lowercase) {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        }
        let trimmed = out.trim_end_matches('-');
        let id: String = trimmed.chars().take(40).collect();
        if id.is_empty() {
            "agent".to_owned()
        } else {
            id
        }
    }
}

impl AgentRuntime {
    /// The host can start this agent. An agent without a command is an external MCP client (a person's own
    /// tool): it has an identity, grants and a token, and the host never launches it.
    #[must_use]
    pub fn is_launchable(&self) -> bool {
        !self.command.trim().is_empty()
    }
}

impl Agent {
    /// What the host checks before storing an agent.
    pub fn validate(&self) -> Result<(), DomainError> {
        AgentId::parse(&self.id.0)?;
        let bad = |why: &str| Err(DomainError::InvalidAgent(why.to_owned()));
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            return bad("name must be 1-60 characters");
        }
        if self.role.chars().count() > 200 {
            return bad("role must be at most 200 characters");
        }
        if self.avatar.as_ref().is_some_and(|a| a.chars().count() > 64) {
            return bad("avatar must be at most 64 characters (an icon name or a colour)");
        }
        Ok(())
    }

    /// Who this agent is for attribution and permission checks.
    #[must_use]
    pub fn identity(&self) -> AgentIdentity {
        AgentIdentity { id: self.id.clone(), name: self.name.clone(), grants: self.grants.permissions() }
    }
}

/// SHA-256 of a bearer token, as lowercase hex: all that is stored of a token.
#[must_use]
pub fn hash_token(token: &str) -> String {
    hex_of(&Sha256::digest(token.as_bytes()))
}

/// Lowercase hex of some bytes.
#[must_use]
pub fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Constant-time equality of two hashes (no early exit on the first differing byte).
#[must_use]
pub fn same_hash(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Agent {
        Agent {
            id: AgentId("claude".into()),
            name: "Claude".into(),
            avatar: None,
            role: "Pair programmer".into(),
            runtime: AgentRuntime { command: "claude-agent-acp".into(), ..AgentRuntime::default() },
            grants: Grants::default(),
            approval: ApprovalPolicy::default(),
        }
    }

    #[test]
    fn a_new_agent_is_denied_everything_and_asks_for_writes() {
        let a = agent();
        assert_eq!(a.grants, Grants::default());
        assert!(a.identity().grants.is_empty(), "deny by default");
        assert_eq!(a.approval, ApprovalPolicy::AutoReadOnly);
        let notes = AppId::parse("notes").unwrap();
        assert!(!a.grants.allows(&notes));
    }

    #[test]
    fn grants_are_an_allow_list_or_the_explicit_all_option() {
        let (notes, todo) = (AppId::parse("notes").unwrap(), AppId::parse("todo-list").unwrap());
        let some = Grants::apps([todo.clone(), notes.clone(), notes.clone()]);
        assert_eq!(some.apps, [notes.clone(), todo.clone()], "sorted, deduplicated");
        assert!(some.allows(&notes) && !some.allows(&AppId::parse("files").unwrap()));
        assert_eq!(some.permissions(), [ToolGrant::CallTools(notes.clone()), ToolGrant::CallTools(todo)]);
        let all = Grants::allow_all();
        assert!(all.allows(&AppId::parse("anything").unwrap()));
        assert_eq!(all.permissions(), [ToolGrant::CallAnyTool]);
    }

    #[test]
    fn grants_and_policy_have_a_stable_wire_form() {
        let json = serde_json::json!({"all": false, "apps": ["notes"]});
        let g: Grants = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&g).unwrap(), json);
        assert_eq!(serde_json::to_value(ApprovalPolicy::AutoReadOnly).unwrap(), "auto_read_only");
        assert!(serde_json::from_value::<Grants>(serde_json::json!({"apps": [], "root": true})).is_err());
        for p in [ApprovalPolicy::Ask, ApprovalPolicy::AutoReadOnly, ApprovalPolicy::AllowEverything] {
            assert_eq!(ApprovalPolicy::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn ids_and_records_are_validated() {
        assert!(AgentId::parse("code-reviewer").is_ok());
        for bad in ["", "-x", "Has Caps", "x".repeat(41).as_str(), "under_score"] {
            assert!(AgentId::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(AgentId::slug("  Code Reviewer!! "), "code-reviewer");
        assert_eq!(AgentId::slug("???"), "agent");
        assert!(agent().validate().is_ok());
        let mut a = agent();
        a.runtime.command = " ".into();
        assert!(a.validate().is_ok() && !a.runtime.is_launchable(), "an external MCP client has no command");
        a = agent();
        a.name = String::new();
        assert!(a.validate().is_err());
    }

    #[test]
    fn a_token_is_stored_as_its_hash_and_compared_whole() {
        let h = hash_token("s3cret");
        assert_eq!(h.len(), 64);
        assert_ne!(h, hash_token("s3cre7"));
        assert!(same_hash(&h, &hash_token("s3cret")));
        assert!(!same_hash(&h, &hash_token("other")) && !same_hash(&h, "short"));
    }

    #[test]
    fn hex_is_lowercase_and_two_digits_per_byte() {
        assert_eq!(hex_of(&[0, 1, 0xab, 0xff]), "0001abff");
        assert_eq!(hex_of(&[]), "");
        assert_eq!(hash_token(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
