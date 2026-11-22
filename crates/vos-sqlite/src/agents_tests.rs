use crate::SqliteSystemDb;
use vos_application::ports::AgentStore;
use vos_domain::{hash_token, Agent, AgentId, AppId, Grants};

fn agent(id: &str) -> Agent {
    Agent {
        id: AgentId(id.into()),
        name: id.to_uppercase(),
        avatar: Some("sparkles".into()),
        role: "Pair programmer".into(),
        grants: Grants::apps([AppId::parse("notes").unwrap()]),
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

/// A database written when the host launched agents itself: the launch columns hold values and the `threads`
/// table has rows. It still opens, the contact reads back, and an update leaves the old columns as they were.
#[tokio::test]
async fn a_row_from_when_the_host_launched_agents_still_reads_and_keeps_its_old_columns() {
    let db = SqliteSystemDb::open(None).unwrap();
    db.db
        .exec(
            "INSERT INTO agents(id, name, role, command, args, env, grants, approval) VALUES('old', 'Old', 'r', \
             'claude-agent-acp', '[\"--flag\"]', '{\"KEY\":\"v\"}', '{\"all\":true,\"apps\":[]}', 'allow_all')"
                .into(),
            vec![],
        )
        .await
        .unwrap();
    db.db
        .exec(
            "INSERT INTO threads(id, agent_id, messages, updated_at) VALUES('t', 'old', '[]', 1)".into(),
            vec![],
        )
        .await
        .unwrap();
    let mut old = AgentStore::list(&db).await.unwrap().remove(0);
    assert_eq!((old.id.0.as_str(), old.name.as_str(), old.grants.all), ("old", "Old", true));
    old.role = "new role".into();
    assert!(AgentStore::update(&db, &old).await.unwrap());
    let kept =
        db.db.query("SELECT role, command, args, env, approval FROM agents".into(), vec![]).await.unwrap();
    assert_eq!(
        kept[0],
        serde_json::json!({"role": "new role", "command": "claude-agent-acp", "args": "[\"--flag\"]",
            "env": "{\"KEY\":\"v\"}", "approval": "allow_all"})
    );
}
