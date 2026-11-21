#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A typed Luau app (`tests/fixtures/typed`: `--!strict`, type aliases, annotations, a generic function, casts, `continue`,
//! `+=`, string interpolation, an `if` expression) built by the product's pipeline and run on the real host: it loads, an
//! agent calls its tool over MCP, and its window renders through `view.open`.

mod common;

use common::{
    browser, built_app, call, claude, connect_agent, publish_package, rpc, start, temp, tool_names,
};
use serde_json::{json, Value};

#[tokio::test]
async fn a_typed_luau_app_loads_and_runs() {
    let bundle = common::built_app("typed");
    let source = String::from_utf8(bundle.clone()).unwrap();
    // the types are stripped, the Luau syntax is not: the host's VM reads it as written
    assert!(
        !source.contains("type Stats") && !source.contains(": number") && !source.contains(":: string"),
        "{source}"
    );
    for luau in ["continue", "stats.count += 1", "`{label}: {value}`", "if stats.count > 2 then"] {
        assert!(source.contains(luau), "the bundle keeps `{luau}`: {source}");
    }

    let tmp = temp("typed-luau");
    let r = start(|c| c.agents = vec![claude()]).await;
    let url = publish_package(&tmp.join("pkg"), "typed", "app.luau", &[("app.luau", built_app("typed"))]);
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

    // the window, over /rpc as the browser: `vos:ui` is typed and runs
    let mut ws = browser(&r).await;
    let hello = rpc(
        &mut ws,
        1,
        "session.hello",
        json!({"proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC"}),
    )
    .await;
    assert!(hello.get("error").is_none(), "{hello}");
    let opened = rpc(
        &mut ws,
        2,
        "view.open",
        json!({"app": "typed", "window": "main", "windowId": "w1", "size": {"w": 320, "h": 240}}),
    )
    .await;
    assert!(opened.get("error").is_none(), "{opened}");
    let tree = opened["result"]["tree"].to_string();
    assert!(tree.contains("Hello from window "), "{tree}");
}
