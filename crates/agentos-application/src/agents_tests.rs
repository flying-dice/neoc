use crate::{
    agents::{AgentError, AgentPatch, AgentService, NewAgent},
    ports::{AgentDirectory, AgentStore},
    testing::{MemAgentStore, MemRegistry, SeededRandom},
};
use agentos_domain::{
    hash_token, Agent, AgentId, AgentRuntime, AppId, ApprovalPolicy, Grants, Permission, QualifiedToolName,
};
use std::sync::Arc;

struct Rig {
    store: Arc<MemAgentStore>,
    tools: Arc<MemRegistry>,
    svc: AgentService,
}

fn rig() -> Rig {
    let store = Arc::new(MemAgentStore::default());
    let tools = Arc::new(MemRegistry::default());
    let svc = AgentService::new(store.clone(), tools.clone(), Arc::new(SeededRandom::default()));
    Rig { store, tools, svc }
}

fn new(name: &str) -> NewAgent {
    NewAgent {
        name: name.into(),
        runtime: AgentRuntime { command: "claude-agent-acp".into(), ..AgentRuntime::default() },
        ..NewAgent::default()
    }
}

fn app(id: &str) -> AppId {
    AppId::parse(id).unwrap()
}

#[tokio::test]
async fn a_new_agent_has_a_slug_id_no_grants_and_the_safe_default_policy() {
    let r = rig();
    let a = r.svc.create(new("Code Reviewer")).await.unwrap();
    assert_eq!(a.id.0, "code-reviewer");
    assert_eq!(a.grants, Grants::default(), "deny by default");
    assert_eq!(a.approval, ApprovalPolicy::AutoReadOnly);
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
    assert_eq!(id.grants, [Permission::CallTools(app("notes"))]);
    assert!(agent_may(&id, &notes).is_ok());
    assert!(agent_may(&id, &QualifiedToolName::new(app("todo"), "add").unwrap()).is_err());
    assert_eq!(r.tools.visibility_changes(), ["claude"]);

    r.svc.set_grants(&a.id, Grants::apps([app("notes")])).await.unwrap();
    assert_eq!(r.tools.visibility_changes().len(), 1, "an unchanged grant set tells nobody");

    r.svc.set_grants(&a.id, Grants::allow_all()).await.unwrap();
    assert_eq!(r.svc.authenticate(&token).unwrap().grants, [Permission::CallAnyTool]);
    r.svc.set_grants(&a.id, Grants::default()).await.unwrap();
    assert!(agent_may(&r.svc.authenticate(&token).unwrap(), &notes).is_err(), "revoking works too");
    assert_eq!(r.tools.visibility_changes().len(), 3);
}

fn agent_may(
    id: &agentos_domain::AgentIdentity,
    tool: &QualifiedToolName,
) -> Result<(), agentos_domain::DomainError> {
    id.authorize_tool_call(tool)
}

#[tokio::test]
async fn a_persistent_token_is_stored_only_as_a_hash_and_rotating_replaces_it() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let first = r.svc.rotate_token(&a.id).await.unwrap();
    assert!(first.starts_with("agentos_") && first.len() > 40);
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
async fn a_launch_token_names_the_agent_and_dies_with_its_session() {
    let r = rig();
    let a = r.svc.create(new("Claude")).await.unwrap();
    let b = r.svc.create(new("Other")).await.unwrap();
    let t1 = r.svc.launch_token(&a.id, "thread-1").unwrap();
    let t2 = r.svc.launch_token(&a.id, "thread-2").unwrap();
    let tb = r.svc.launch_token(&b.id, "thread-3").unwrap();
    assert_eq!(r.svc.authenticate(&t1).unwrap().name, "Claude");
    assert_eq!(r.svc.authenticate(&tb).unwrap().name, "Other");
    assert_ne!(t1, t2);
    assert!(r.store.token_hashes().await.unwrap().is_empty(), "launch tokens are never stored");

    r.svc.revoke_launch_tokens("thread-1");
    assert!(r.svc.authenticate(&t1).is_none());
    assert!(r.svc.authenticate(&t2).is_some(), "other sessions keep theirs");

    r.svc.delete(&a.id).await.unwrap();
    assert!(r.svc.authenticate(&t2).is_none(), "deleting the agent revokes everything it had");
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
                approval: Some(ApprovalPolicy::Ask),
                avatar: Some(Some("sparkles".into())),
                ..AgentPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (u.name.as_str(), u.role.as_str(), u.approval),
        ("Claude", "Pair programmer", ApprovalPolicy::Ask)
    );
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
        runtime: AgentRuntime { command: "x".into(), ..AgentRuntime::default() },
        grants: Grants::default(),
        approval: ApprovalPolicy::default(),
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
    assert_eq!(fresh.authenticate("fixed-token").unwrap().grants, [Permission::CallAnyTool]);
}

/// Records which agents the roster asked to have stopped.
#[derive(Default)]
struct StopLog(parking_lot::Mutex<Vec<String>>);

#[async_trait::async_trait]
impl crate::agents::AgentProcesses for StopLog {
    async fn stop_agent(&self, agent: &AgentId) {
        self.0.lock().push(agent.0.clone());
    }
}

fn rig_with_processes() -> (AgentService, Arc<StopLog>) {
    let log = Arc::new(StopLog::default());
    let svc = AgentService::new(
        Arc::new(MemAgentStore::default()),
        Arc::new(MemRegistry::default()),
        Arc::new(SeededRandom::default()),
    )
    .with_processes(log.clone());
    (svc, log)
}

#[tokio::test]
async fn changing_how_an_agent_is_launched_stops_its_process_and_other_edits_do_not() {
    let (svc, log) = rig_with_processes();
    let a = svc.create(new("Claude")).await.unwrap();
    let runtime = |command: &str, env: &[(&str, &str)]| AgentRuntime {
        command: command.into(),
        args: vec![],
        env: env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
    };
    let patch = |runtime| AgentPatch { runtime: Some(runtime), ..AgentPatch::default() };

    // edits that do not change the launch leave the process alone
    svc.update(&a.id, AgentPatch { role: Some("Pair".into()), ..AgentPatch::default() }).await.unwrap();
    svc.update(&a.id, patch(a.runtime.clone())).await.unwrap();
    svc.set_grants(&a.id, Grants::allow_all()).await.unwrap();
    assert!(log.0.lock().is_empty(), "{:?}", log.0.lock());

    svc.update(&a.id, patch(runtime("other-agent", &[]))).await.unwrap();
    assert_eq!(*log.0.lock(), ["claude"], "a new command stops the process");
    svc.update(&a.id, patch(runtime("other-agent", &[("API_KEY", "k2")]))).await.unwrap();
    assert_eq!(log.0.lock().len(), 2, "a new environment stops it too");

    // a rejected update changes nothing, and stops nothing
    let bad = AgentPatch {
        name: Some(String::new()),
        runtime: Some(runtime("third", &[])),
        ..AgentPatch::default()
    };
    assert!(svc.update(&a.id, bad).await.is_err());
    assert_eq!(log.0.lock().len(), 2);
}

#[tokio::test]
async fn deleting_an_agent_stops_its_process() {
    let (svc, log) = rig_with_processes();
    let a = svc.create(new("Claude")).await.unwrap();
    assert!(svc.delete(&AgentId("nobody".into())).await.is_err());
    assert!(log.0.lock().is_empty(), "a delete that failed stops nothing");
    svc.delete(&a.id).await.unwrap();
    assert_eq!(*log.0.lock(), ["claude"]);
}
