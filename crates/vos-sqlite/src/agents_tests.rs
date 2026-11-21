use crate::SqliteSystemDb;
use vos_application::ports::{AgentStore, StoredThread, ThreadStore};
use vos_domain::{
    hash_token, thread::ThreadMessage, Agent, AgentId, AgentRuntime, AppId, ApprovalPolicy, Grants,
};

fn agent(id: &str) -> Agent {
    Agent {
        id: AgentId(id.into()),
        name: id.to_uppercase(),
        avatar: Some("sparkles".into()),
        role: "Pair programmer".into(),
        runtime: AgentRuntime {
            command: "claude-agent-acp".into(),
            args: vec!["--flag".into()],
            env: [("KEY".to_owned(), "v".to_owned())].into(),
        },
        grants: Grants::apps([AppId::parse("notes").unwrap()]),
        approval: ApprovalPolicy::Ask,
    }
}

#[tokio::test]
async fn agents_round_trip_in_insertion_order_and_ids_are_unique() {
    let db = SqliteSystemDb::open(None).unwrap();
    assert!(AgentStore::insert(&db, &agent("b"), None).await.unwrap());
    assert!(AgentStore::insert(&db, &agent("a"), Some(&hash_token("t"))).await.unwrap());
    assert!(!AgentStore::insert(&db, &agent("a"), None).await.unwrap(), "a taken id is refused");
    let all = AgentStore::list(&db).await.unwrap();
    assert_eq!(all, [agent("b"), agent("a")]);
    assert_eq!(
        db.token_hashes().await.unwrap(),
        [(AgentId("a".into()), hash_token("t"))],
        "only agents with a token appear, and only as hashes"
    );
}

#[tokio::test]
async fn update_delete_and_token_changes_report_whether_anything_matched() {
    let db = SqliteSystemDb::open(None).unwrap();
    AgentStore::insert(&db, &agent("a"), None).await.unwrap();
    let mut changed = agent("a");
    changed.grants = Grants::allow_all();
    changed.avatar = None;
    assert!(AgentStore::update(&db, &changed).await.unwrap());
    assert_eq!(AgentStore::list(&db).await.unwrap(), [changed]);
    assert!(!AgentStore::update(&db, &agent("ghost")).await.unwrap());
    assert!(db.set_token_hash(&AgentId("a".into()), "h1").await.unwrap());
    assert!(db.set_token_hash(&AgentId("a".into()), "h2").await.unwrap());
    assert_eq!(db.token_hashes().await.unwrap()[0].1, "h2");
    assert!(!db.set_token_hash(&AgentId("ghost".into()), "h").await.unwrap());
    assert!(AgentStore::delete(&db, &AgentId("a".into())).await.unwrap());
    assert!(!AgentStore::delete(&db, &AgentId("a".into())).await.unwrap());
    assert!(db.token_hashes().await.unwrap().is_empty());
}

#[tokio::test]
async fn transcripts_are_stored_whole_and_replaced_by_the_next_save() {
    let db = SqliteSystemDb::open(None).unwrap();
    assert!(ThreadStore::load(&db, "t1").await.unwrap().is_none());
    let mut thread = StoredThread {
        agent_id: AgentId("a".into()),
        messages: vec![ThreadMessage::User { id: "u1".into(), content: "hi".into() }],
    };
    db.save("t1", &thread, 10).await.unwrap();
    thread.messages.push(ThreadMessage::Assistant {
        id: "m1".into(),
        content: Some("hello".into()),
        tool_calls: vec![],
        vos: None,
    });
    db.save("t1", &thread, 20).await.unwrap();
    assert_eq!(ThreadStore::load(&db, "t1").await.unwrap(), Some(thread));
    ThreadStore::delete(&db, "t1").await.unwrap();
    assert!(ThreadStore::load(&db, "t1").await.unwrap().is_none());
}
