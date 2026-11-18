//! `AppService::call_tool` hands the runtime the `readOnly` requirement of the tool it runs.

use crate::testing::Fakes;
use agentos_domain::{AgentId, AgentIdentity, Permission, QualifiedToolName};
use serde_json::json;

fn agent() -> AgentIdentity {
    AgentIdentity {
        id: AgentId("claude".into()),
        name: "Claude".into(),
        grants: vec![Permission::CallAnyTool],
    }
}

#[tokio::test]
async fn a_tool_the_app_declared_read_only_reaches_the_runtime_as_read_only_and_a_writing_one_does_not() {
    let f = Fakes::new();
    let url = f.add_app("notes", &[("search?", "x"), ("append", "x")]);
    let svc = f.service();
    svc.install_from_url(&url).await.unwrap();

    let search = QualifiedToolName::parse("notes__search").unwrap();
    let append = QualifiedToolName::parse("notes__append").unwrap();
    svc.call_tool(&agent(), &search, json!({})).await.unwrap();
    svc.call_tool(&agent(), &append, json!({})).await.unwrap();

    assert_eq!(
        f.runtime.read_only_calls(),
        vec![true, false],
        "the guarantee that lets an agent run `search` unprompted is passed through the AppRuntime port, not left to the adapter"
    );
}
