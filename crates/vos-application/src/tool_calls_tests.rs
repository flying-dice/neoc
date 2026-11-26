//! `AppService::call_tool` asks the runtime for a read-only run of a tool the contract says changes nothing.

use crate::testing::Fakes;
use serde_json::json;
use vos_domain::{AgentId, AgentIdentity, QualifiedToolName, ToolGrant};

fn agent() -> AgentIdentity {
    AgentIdentity {
        id: AgentId("claude".into()),
        name: "Claude".into(),
        grants: vec![ToolGrant::CallAnyTool],
    }
}

/// The contract of `notes`: `search` changes nothing, `append` does.
const CONTRACT: &str = r#"{
  "openrpc": "1.4.0", "info": { "title": "Notes", "version": "1.0.0" },
  "methods": [
    { "name": "search", "x-readOnly": true, "params": [], "result": { "name": "r", "schema": {} } },
    { "name": "append", "params": [], "result": { "name": "r", "schema": {} } }
  ]
}"#;

#[tokio::test]
async fn a_tool_described_as_read_only_reaches_the_runtime_as_read_only_and_a_writing_one_does_not() {
    let f = Fakes::new();
    let url = f.add_app_with("notes", &[("search", "x"), ("append", "x")], &[("openrpc.json", CONTRACT)]);
    f.edit_ref(&url, |app_ref| app_ref["manifest"]["contract"] = json!("openrpc.json"));
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
