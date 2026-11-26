#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A typed Luau backend (`tests/fixtures/backends/typed.luau`: `--!strict`, type aliases, annotations, a generic function,
//! casts, `continue`, `+=`, string interpolation, an `if` expression) built by the product's pipeline and run on the real
//! host: it loads, an agent calls its tool over MCP, and its page calls its handler through `app.call`.

use crate::common;

use common::{
    browser, built_backend, call, claude, connect_agent, publish_package, rpc, start, temp, tool_names,
};
use serde_json::{json, Value};

#[tokio::test]
async fn a_typed_luau_app_loads_and_runs() {
    let bundle = built_backend("typed");
    let source = String::from_utf8(bundle.clone()).unwrap();
    // the types are stripped, the Luau syntax is not: the host's VM reads it as written
    assert!(
        !source.contains("type Stats")
            && !source.contains(": number")
            && !source.contains(":: string")
            && !source.contains("vos_app_Context"),
        "{source}"
    );
    for luau in ["continue", "stats.count += 1", "`{label}: {value}`", "if stats.count > 2 then"] {
        assert!(source.contains(luau), "the bundle keeps `{luau}`: {source}");
    }

    let tmp = temp("typed-luau");
    let r = start(|c| c.agents = vec![claude()]).await;
    let url = publish_package(&tmp.join("pkg"), "typed", "app.luau", &[("app.luau", bundle)]);
    r.host.service.install_from_url(&url).await.unwrap();

    // the tool, over MCP as an agent: the typed code ran
    let (agent, _) = connect_agent(r.addr, "tok-claude").await;
    assert_eq!(tool_names(&agent).await, vec!["typed__summarize"]);
    let out = call(&agent, "typed__summarize", json!({ "numbers": [1, -2, 3, 4] })).await;
    let text = out.content[0].as_text().expect("the tool returns text").text.clone();
    let result: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(
        result,
        json!({ "count": 3, "total": 8, "text": "sum: 8", "size": "many", "encoded": "bWFueQ==" }),
        "{result}"
    );

    // the handler, over /rpc as the app's page: the typed handler ran, as the person
    let mut ws = browser(&r).await;
    let greeted =
        rpc(&mut ws, 1, "app.call", json!({"app": "typed", "method": "_greet", "args": {"name": "Ada"}}))
            .await;
    assert_eq!(greeted["result"], "Hello Ada from a user", "{greeted}");
    agent.cancel().await.unwrap();
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}
