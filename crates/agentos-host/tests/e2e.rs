#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! End-to-end: the REAL adapters (luau, sqlite, fetch, mcp, agui, http) wired by the composition root,
//! driven from the outside like the real clients: a WebSocket client (/rpc) and an MCP client (/mcp).

use agentos_domain::{AgentId, AgentIdentity, AppId, AppRef, FileRef, Manifest, MigrationSpec, Permission};
use agentos_host::{build, HostConfig};
use futures_util::{SinkExt, StreamExt};
use rmcp::{
    model::CallToolRequestParams,
    service::NotificationContext,
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport},
    ClientHandler, RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

fn publish_notes_app(root: &Path) -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes");
    std::fs::create_dir_all(root).unwrap();
    let mut files = vec![];
    for name in ["main.luau", "001_init.sql"] {
        let bytes = std::fs::read(src.join(name)).unwrap();
        std::fs::write(root.join(name), &bytes).unwrap();
        files.push(FileRef {
            path: name.into(),
            url: format!("file://{}/{name}", root.display()),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
    }
    let r = AppRef {
        manifest: Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "Notes".into(),
            version: "1.0.0".into(),
            main: "main.luau".into(),
            permissions: vec![Permission::Data, Permission::Notify],
            migrations: vec![MigrationSpec { version: 1, name: "init".into(), file: "001_init.sql".into() }],
        },
        files,
    };
    std::fs::write(root.join("app-ref.json"), serde_json::to_vec(&r).unwrap()).unwrap();
    format!("file://{}/app-ref.json", root.display())
}

#[derive(Clone, Default)]
struct Counting(Arc<AtomicUsize>);
impl ClientHandler for Counting {
    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

type Raw = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
/// WS client that keeps notifications that arrive while it waits for a response (they can overtake it).
struct Ws {
    raw: Raw,
    backlog: Vec<Value>,
}
async fn next_json(ws: &mut Raw) -> Value {
    loop {
        if let Message::Text(t) = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("frame in 5s")
            .unwrap()
            .unwrap()
        {
            return serde_json::from_str(&t).unwrap();
        }
    }
}
async fn rpc(ws: &mut Ws, id: i64, method: &str, params: Value) -> Value {
    ws.raw
        .send(Message::text(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()))
        .await
        .unwrap();
    loop {
        let m = next_json(&mut ws.raw).await;
        if m["id"] == id {
            return m;
        }
        ws.backlog.push(m);
    }
}
async fn next_notification(ws: &mut Ws, method: &str) -> Value {
    if let Some(i) = ws.backlog.iter().position(|m| m["method"] == method) {
        return ws.backlog.remove(i)["params"].clone();
    }
    loop {
        let m = next_json(&mut ws.raw).await;
        if m["method"] == method {
            return m["params"].clone();
        }
        ws.backlog.push(m);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one journey, read top to bottom
async fn install_over_rpc_then_use_from_mcp_and_ui_and_uninstall() {
    let tmp = std::env::temp_dir().join(format!("host-e2e-{}", std::process::id()));
    let app_ref_url = publish_notes_app(&tmp.join("pkg"));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let host = build(HostConfig {
        data_dir: Some(tmp.join("data")),
        client_dir: None,
        port: addr.port(),
        dev_client_port: None,
        agents: vec![(
            "tok-claude".into(),
            AgentIdentity {
                id: AgentId("claude".into()),
                name: "Claude".into(),
                grants: vec![Permission::CallAnyTool],
            },
        )],
        acp: HashMap::new(),
    })
    .unwrap();
    let shutdown = host.shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, host.router)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await
    });
    std::fs::create_dir_all(tmp.join("data/apps")).unwrap();

    // --- client (UI) connects on /rpc and installs the app from a file:// app ref
    let mut req = format!("ws://{addr}/rpc").into_client_request().unwrap();
    req.headers_mut().insert("origin", format!("http://127.0.0.1:{}", addr.port()).parse().unwrap());
    let (raw, _) = connect_async(req).await.unwrap();
    let mut ws = Ws { raw, backlog: vec![] };
    let r = rpc(&mut ws, 1, "app.install", json!({"url": app_ref_url})).await;
    assert_eq!(r["result"]["app"], "notes", "{r}");
    assert_eq!(next_notification(&mut ws, "app.installed").await["app"], "notes");

    // --- agent connects on /mcp with its own bearer token and sees the tool
    let counter = Counting::default();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")).auth_header("tok-claude"),
    );
    let agent = counter.clone().serve(transport).await.unwrap();
    let names: Vec<String> =
        agent.peer().list_tools(None).await.unwrap().tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["notes__add_note"]);

    // --- agent calls the tool: Luau runs, SQLite writes, app.notify reaches the UI over /rpc
    let args = json!({"body": "from the agent"}).as_object().unwrap().clone();
    let out = agent
        .peer()
        .call_tool(CallToolRequestParams::new("notes__add_note").with_arguments(args))
        .await
        .unwrap();
    assert_ne!(out.is_error, Some(true), "{out:?}");
    let n = next_notification(&mut ws, "notify").await;
    assert_eq!(
        (n["app"].as_str(), n["title"].as_str(), n["body"].as_str()),
        (Some("notes"), Some("Note added"), Some("from the agent"))
    );

    // --- user clicks "add" in the UI: ui.event -> Luau handler -> new view tree pushed back
    let r = rpc(
        &mut ws,
        2,
        "ui.event",
        json!({"app":"notes","event":{"target":"add-btn","name":"add","data":{"body":"from the user"}}}),
    )
    .await;
    assert!(r.get("error").is_none(), "{r}");
    let tree = next_notification(&mut ws, "ui.tree").await;
    println!("ui.tree -> {tree}");
    assert_eq!(tree["root"]["children"][0]["text"], "Notes: 2");
    let labels: Vec<&str> = tree["root"]["children"][1]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, vec!["from the agent", "from the user"]);

    // --- bad requests are JSON-RPC errors, not hangs
    let r = rpc(&mut ws, 3, "ui.event", json!({"app":"ghost","event":{"target":"x","name":"y"}})).await;
    assert_eq!(r["error"]["code"], -32002, "{r}");

    // --- uninstall: tool disappears for the agent (list_changed), VM + registry cleaned up
    let before = counter.0.load(Ordering::SeqCst);
    let r = rpc(&mut ws, 4, "app.uninstall", json!({"app":"notes"})).await;
    assert!(r.get("error").is_none(), "{r}");
    for _ in 0..100 {
        if counter.0.load(Ordering::SeqCst) > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(counter.0.load(Ordering::SeqCst) > before, "tools/list_changed after uninstall");
    assert!(agent.peer().list_tools(None).await.unwrap().tools.is_empty());

    // --- cross-site WebSocket is refused; /agui streams
    let mut evil = format!("ws://{addr}/rpc").into_client_request().unwrap();
    evil.headers_mut().insert("origin", "https://evil.example".parse().unwrap());
    assert!(connect_async(evil).await.is_err());
    let sse = reqwest::Client::new()
        .post(format!("http://{addr}/agui"))
        .json(&json!({"threadId":"t","runId":"r"}))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(sse.contains("RUN_STARTED") && sse.contains("RUN_FINISHED"));

    agent.cancel().await.unwrap();
    host.service.uninstall(&AppId::parse("notes").unwrap(), false).await.unwrap_err(); // already gone
    server.abort();
    let _ = std::fs::remove_dir_all(tmp);
}
