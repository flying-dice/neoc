use crate::{
    agents::{AgentError, AgentPatch, AgentService, McpEndpoint, NewAgent},
    ports::{AgentDirectory, AgentRoster, AgentStore, ToolLeases},
    testing::{MemAgentStore, MemRegistry, SeededRandom},
};
use std::sync::Arc;
use vos_domain::{hash_token, Actor, Agent, AgentId, AppId, Grants, QualifiedToolName, ToolGrant};

struct Rig {
    store: Arc<MemAgentStore>,
    tools: Arc<MemRegistry>,
    svc: AgentService,
}

fn rig() -> Rig {
    let store = Arc::new(MemAgentStore::default());
    let tools = Arc::new(MemRegistry::default());
    let svc = AgentService::new(store.clone(), tools.clone(), Arc::new(SeededRandom::default()))
        .with_mcp(McpEndpoint { scheme: "http".into(), host: "127.0.0.1".into(), port: 7400 });
    Rig { store, tools, svc }
}

fn new(name: &str) -> NewAgent {
    NewAgent { name: name.into(), ..NewAgent::default() }
}

fn app(id: &str) -> AppId {
    AppId::parse(id).unwrap()
}

#[tokio::test]
async fn a_new_agent_has_a_slug_id_and_no_grants() {
    let r = rig();
    let a = r.svc.create(new("Code Reviewer")).await.unwrap();
    assert_eq!(a.id.0, "code-reviewer");
    assert_eq!(a.grants, Grants::default(), "deny by default");
    let b = r.svc.create(new("Code Reviewer")).await.unwrap();
    assert_eq!(b.id.0, "code-reviewer-2", "a taken slug gets a number");
    let dup = NewAgent { id: Some("code-reviewer".into()), ..new("Other") };
    assert!(matches!(r.svc.create(dup).await, Err(AgentError::Exists(_))));
    assert!(matches!(r.svc.create(new("")).await, Err(AgentError::Domain(_))));
    assert_eq!(r.svc.list().len(), 2);
}

#[tokio::test]
async fn grants_take_effect_on_the_next_call_and_the_agent_is_told() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let token = r.svc.rotate_token(&a.id).await.unwrap();
    let notes = QualifiedToolName::new(app("notes"), "append").unwrap();

    let id = r.svc.authenticate(&token).unwrap();
    assert!(id.grants.is_empty() && agent_may(&id, &notes).is_err(), "nothing granted yet");

    r.svc.set_grants(&a.id, Grants::apps([app("notes")])).await.unwrap();
    let id = r.svc.authenticate(&token).unwrap();
    assert_eq!(id.grants, [ToolGrant::CallTools(app("notes"))]);
    assert!(agent_may(&id, &notes).is_ok());
    assert!(agent_may(&id, &QualifiedToolName::new(app("todo"), "add").unwrap()).is_err());
    assert_eq!(r.tools.visibility_changes(), ["claude"]);

    r.svc.set_grants(&a.id, Grants::apps([app("notes")])).await.unwrap();
    assert_eq!(r.tools.visibility_changes().len(), 1, "an unchanged grant set tells nobody");

    r.svc.set_grants(&a.id, Grants::allow_all()).await.unwrap();
    assert_eq!(r.svc.authenticate(&token).unwrap().grants, [ToolGrant::CallAnyTool]);
    r.svc.set_grants(&a.id, Grants::default()).await.unwrap();
    assert!(agent_may(&r.svc.authenticate(&token).unwrap(), &notes).is_err(), "revoking works too");
    assert_eq!(r.tools.visibility_changes().len(), 3);
}

fn agent_may(
    id: &vos_domain::AgentIdentity,
    tool: &QualifiedToolName,
) -> Result<(), vos_domain::DomainError> {
    id.authorize_tool_call(tool)
}

#[tokio::test]
async fn a_persistent_token_is_stored_only_as_a_hash_and_rotating_replaces_it() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let first = r.svc.rotate_token(&a.id).await.unwrap();
    assert!(first.starts_with("vos_") && first.len() > 40);
    let hashes = r.store.token_hashes().await.unwrap();
    assert_eq!(hashes, [(a.id.clone(), hash_token(&first))]);
    assert!(!hashes[0].1.contains(&first), "the plaintext is not stored");
    assert!(r.svc.authenticate(&first).is_some());

    let second = r.svc.rotate_token(&a.id).await.unwrap();
    assert_ne!(first, second);
    assert!(r.svc.authenticate(&first).is_none(), "the old token stops working");
    assert!(r.svc.authenticate(&second).is_some());
    assert!(r.svc.authenticate("").is_none() && r.svc.authenticate("nope").is_none());
    assert!(matches!(r.svc.rotate_token(&AgentId("ghost".into())).await, Err(AgentError::NotFound(_))));
}

#[tokio::test]
async fn deleting_an_agent_revokes_its_token_and_tells_its_sessions() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let b = r.svc.create(new("Other")).await.unwrap();
    let ta = r.svc.rotate_token(&a.id).await.unwrap();
    let tb = r.svc.rotate_token(&b.id).await.unwrap();
    assert_eq!(r.svc.authenticate(&ta).unwrap().name, "Claude");
    assert_eq!(r.svc.authenticate(&tb).unwrap().name, "Other");

    r.svc.delete(&a.id).await.unwrap();
    assert!(r.svc.authenticate(&ta).is_none(), "deleting the agent revokes its token");
    assert!(r.svc.authenticate(&tb).is_some());
    assert_eq!(r.tools.visibility_changes(), ["claude"]);
    assert!(matches!(r.svc.delete(&a.id).await, Err(AgentError::NotFound(_))));
}

#[tokio::test]
async fn update_changes_only_what_it_is_given() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let u = r
        .svc
        .update(
            &a.id,
            AgentPatch {
                role: Some("Pair programmer".into()),
                avatar: Some(Some("sparkles".into())),
                ..AgentPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((u.name.as_str(), u.role.as_str()), ("Claude", "Pair programmer"));
    assert_eq!(u.avatar.as_deref(), Some("sparkles"));
    let cleared =
        r.svc.update(&a.id, AgentPatch { avatar: Some(None), ..AgentPatch::default() }).await.unwrap();
    assert!(cleared.avatar.is_none());
    let bad = AgentPatch { name: Some(String::new()), ..AgentPatch::default() };
    assert!(matches!(r.svc.update(&a.id, bad).await, Err(AgentError::Domain(_))));
    assert_eq!(r.svc.get(&a.id).unwrap().role, "Pair programmer", "a rejected update changes nothing");
}

#[tokio::test]
async fn ensure_seeds_an_agent_once_and_never_overwrites_what_the_person_edited() {
    let r = rig();
    let seed = Agent {
        id: AgentId("claude".into()),
        name: "Claude".into(),
        avatar: None,
        role: String::new(),
        grants: Grants::default(),
    };
    r.svc.ensure(seed.clone(), Some("fixed-token")).await.unwrap();
    assert!(r.svc.authenticate("fixed-token").is_some());
    r.svc.set_grants(&seed.id, Grants::allow_all()).await.unwrap();
    let again = r.svc.ensure(seed, None).await.unwrap();
    assert!(again.grants.all, "the edited grants survive a re-seed");

    // a second service over the same store reads the roster back
    let fresh = AgentService::new(r.store.clone(), r.tools.clone(), Arc::new(SeededRandom::default()));
    fresh.load().await.unwrap();
    assert_eq!(fresh.list().len(), 1);
    assert_eq!(fresh.authenticate("fixed-token").unwrap().grants, [ToolGrant::CallAnyTool]);
}

#[test]
fn every_agent_token_is_minted_with_the_vos_prefix_and_192_random_bits() {
    let random = SeededRandom::default();
    let (a, b) = (crate::mint_agent_token(&random), crate::mint_agent_token(&random));
    for token in [&a, &b] {
        let hex = token.strip_prefix("vos_").expect("a leaked token is recognisable by its prefix");
        assert_eq!(hex.len(), 48);
        assert!(hex.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
    assert_ne!(a, b);
}

/// The actor `app.tools.call` names for the Agents app's agent `ada`.
fn ada() -> Actor {
    Actor::agent("agents:ada", "Ada")
}

#[tokio::test]
async fn a_lease_authenticates_as_the_apps_agent_and_may_call_every_app_but_its_own() {
    let r = rig();
    let lease = r.svc.lease(&app("agents"), &ada(), None).unwrap();
    assert!(lease.token.starts_with("vos_"), "minted like every agent token");
    assert_eq!(lease.url, "http://127.0.0.1:7400/mcp");

    let id = r.svc.authenticate(&lease.token).unwrap();
    assert_eq!(Actor::from(&id), ada(), "a call over /mcp is attributed as `app.tools.call` attributes it");
    assert_eq!(id.grants, [ToolGrant::CallAnyToolBut(app("agents"))]);
    assert!(agent_may(&id, &QualifiedToolName::new(app("notes"), "append").unwrap()).is_ok());
    assert!(
        agent_may(&id, &QualifiedToolName::new(app("agents"), "send").unwrap()).is_err(),
        "not its own app"
    );
}

#[tokio::test]
async fn a_lease_is_never_stored_nor_listed() {
    let r = rig();
    r.svc.create(new("Claude")).await.unwrap();
    let lease = r.svc.lease(&app("agents"), &ada(), None).unwrap();
    assert!(r.svc.authenticate(&lease.token).is_some());
    assert_eq!(r.svc.list().len(), 1, "the roster is the person's agents only");
    assert_eq!(r.svc.cards().len(), 1, "apps are not shown it either");
    assert!(r.store.token_hashes().await.unwrap().is_empty(), "nothing of it is stored");
}

#[tokio::test]
async fn releasing_a_lease_revokes_it_and_tells_its_sessions() {
    let r = rig();
    let first = r.svc.lease(&app("agents"), &ada(), None).unwrap();
    let second = r.svc.lease(&app("agents"), &ada(), None).unwrap();
    assert_ne!(first.token, second.token, "each lease is a token of its own");

    assert!(!r.svc.release(&app("other"), &first.token).await, "another app cannot release it");
    assert!(r.svc.authenticate(&first.token).is_some());
    assert!(r.svc.release(&app("agents"), &first.token).await);
    assert!(r.svc.authenticate(&first.token).is_none(), "released");
    assert!(r.svc.authenticate(&second.token).is_some(), "the other lease is left alone");
    assert_eq!(r.tools.visibility_changes(), ["agents:ada"]);
    assert!(!r.svc.release(&app("agents"), &first.token).await, "a second release finds nothing");
    assert_eq!(r.tools.visibility_changes().len(), 1);
}

#[tokio::test]
async fn every_lease_of_an_app_goes_when_its_backend_stops() {
    let r = rig();
    let a = r.svc.lease(&app("agents"), &ada(), None).unwrap();
    let b = r.svc.lease(&app("agents"), &Actor::agent("agents:bo", "Bo"), None).unwrap();
    let other = r.svc.lease(&app("broker"), &Actor::agent("broker:cy", "Cy"), None).unwrap();
    r.svc.release_all(&app("agents")).await;
    assert!(r.svc.authenticate(&a.token).is_none() && r.svc.authenticate(&b.token).is_none());
    assert!(r.svc.authenticate(&other.token).is_some(), "another app's leases stay");
    assert_eq!(r.tools.visibility_changes(), ["agents:ada", "agents:bo"]);
}

#[test]
fn a_lease_url_names_the_host_it_is_given_or_the_hosts_own() {
    let r = rig();
    let url = |host: Option<&str>| r.svc.lease(&app("agents"), &ada(), host).map(|l| l.url);
    assert_eq!(url(Some("100.64.0.7")).unwrap(), "http://100.64.0.7:7400/mcp");
    assert_eq!(url(Some("fd7a:115c::7")).unwrap(), "http://[fd7a:115c::7]:7400/mcp", "IPv6 is bracketed");
    assert_eq!(url(Some("[fd7a:115c::7]")).unwrap(), "http://[fd7a:115c::7]:7400/mcp");
    assert_eq!(url(Some("studio.local")).unwrap(), "http://studio.local:7400/mcp");
    for bad in ["", "a b", "evil.com/x", "user@host", "host:80", "-x", "[nope]"] {
        assert!(url(Some(bad)).is_err(), "{bad:?}");
    }
    let tls = McpEndpoint { scheme: "https".into(), host: "::1".into(), port: 8443 };
    assert_eq!(tls.url(None).unwrap(), "https://[::1]:8443/mcp");

    let unserved = AgentService::new(
        Arc::new(MemAgentStore::default()),
        Arc::new(MemRegistry::default()),
        Arc::new(SeededRandom::default()),
    );
    assert!(unserved.lease(&app("agents"), &ada(), None).is_err(), "no /mcp, no lease");
}
