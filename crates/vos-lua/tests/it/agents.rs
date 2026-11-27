#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The agent-facing part of the app API: `app.agents:list()`, and the MCP tokens an app leases for an agent of its
//! own (`app.tools.lease`, `app.tools.release`).

use crate::common;

use common::{agent_ctx, db, id, package, user_ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AgentCard, AgentRoster, AppData, AppRuntime, Lease, Notifier, PortResult, ToolLeases},
    testing::RecordingShell,
    PortError,
};
use vos_domain::{Actor, AgentId, AppId, Notification};
use vos_lua::{AppVm, LuaRuntime, VmConfig};

/// The first line of every source here: the line numbers the tests name are the body's plus one.
const MANIFEST: &str = "-- probe";

struct Cards;
impl AgentRoster for Cards {
    fn cards(&self) -> Vec<AgentCard> {
        vec![
            AgentCard {
                id: AgentId("claude".into()),
                name: "Claude".into(),
                avatar: Some("sparkles".into()),
                role: "Pair programmer".into(),
            },
            AgentCard {
                id: AgentId("other".into()),
                name: "Other".into(),
                avatar: None,
                role: String::new(),
            },
        ]
    }
}

async fn with_roster(body: &str) -> AppVm {
    let config = VmConfig { roster: Arc::new(Cards), ..VmConfig::default() };
    AppVm::load(&id("probe"), package(&format!("{MANIFEST}\n{body}"), &[]), db(), config).await.unwrap()
}

#[tokio::test]
async fn the_roster_is_listed_as_cards_without_grants_or_tokens() {
    let vm = with_roster(r#"RPC.on_request("who", function() return app.agents:list() end)"#).await;
    let out = common::serve(&vm, "who", json!({}), &agent_ctx(0)).await.unwrap();
    assert_eq!(
        out,
        json!([
            {"id": "claude", "name": "Claude", "avatar": "sparkles", "role": "Pair programmer"},
            {"id": "other", "name": "Other", "role": ""}
        ])
    );
}

/// What the app asked of the host's leases, in order.
#[derive(Default)]
struct Desk(Mutex<Vec<String>>);
impl Desk {
    fn asked(&self) -> Vec<String> {
        self.0.lock().clone()
    }
}
#[async_trait::async_trait]
impl ToolLeases for Desk {
    fn lease(&self, app: &AppId, agent: &Actor, host: Option<&str>) -> PortResult<Lease> {
        let host = host.unwrap_or("127.0.0.1");
        if host == "bad" {
            return Err(PortError::Rejected("`bad` is not a host name or an IP address".into()));
        }
        self.0.lock().push(format!("lease {app} {} {} {host}", agent.id, agent.name));
        Ok(Lease { token: format!("vos_{}", agent.id), url: format!("http://{host}:7400/mcp") })
    }
    async fn release(&self, app: &AppId, token: &str) -> bool {
        self.0.lock().push(format!("release {app} {token}"));
        true
    }
    async fn release_all(&self, app: &AppId) {
        self.0.lock().push(format!("release_all {app}"));
    }
}

const BROKER: &str = r#"
RPC.on_request("lease", function(args) return app.tools.lease(args.agent, args.opts) end)
RPC.on_request("release", function(args) app.tools.release(args.token); return true end)
RPC.on_request("refused", function(args)
  local ok, err = pcall(app.tools.lease, args.agent, args.opts)
  return { ok = ok, err = tostring(err) }
end)
"#;

async fn with_desk(desk: &Arc<Desk>) -> AppVm {
    let config = VmConfig { leases: desk.clone(), ..VmConfig::default() };
    AppVm::load(
        &id("probe"),
        package(
            &format!(
                "{MANIFEST}
{BROKER}"
            ),
            &[],
        ),
        db(),
        config,
    )
    .await
    .unwrap()
}

async fn ask(vm: &AppVm, method: &str, args: Value) -> Value {
    common::serve(vm, method, args, &user_ctx(0)).await.unwrap()
}

#[tokio::test]
async fn an_app_leases_a_token_for_its_own_agent_and_releases_it() {
    let desk = Arc::new(Desk::default());
    let vm = with_desk(&desk).await;
    let ada = json!({ "id": "ada", "name": "Ada" });

    // the agent is the one `app.tools.call` names: under the app's id
    let lease = ask(&vm, "lease", json!({ "agent": ada, "opts": { "host": "100.64.0.7" } })).await;
    assert_eq!(lease, json!({ "token": "vos_probe:ada", "url": "http://100.64.0.7:7400/mcp" }));
    let own = ask(&vm, "lease", json!({ "agent": ada })).await;
    assert_eq!(own["url"], "http://127.0.0.1:7400/mcp", "the host's own address when none is given");

    assert_eq!(ask(&vm, "release", json!({ "token": "vos_probe:ada" })).await, json!(true));
    assert_eq!(
        desk.asked(),
        [
            "lease probe probe:ada Ada 100.64.0.7",
            "lease probe probe:ada Ada 127.0.0.1",
            "release probe vos_probe:ada"
        ]
    );
}

#[tokio::test]
async fn a_lease_is_refused_for_a_bad_agent_a_bad_option_and_in_a_read_only_call() {
    let desk = Arc::new(Desk::default());
    let vm = with_desk(&desk).await;
    for (args, why) in [
        (json!({ "agent": { "id": "", "name": "Ada" } }), "needs `id`"),
        (json!({ "agent": { "id": "ada", "name": "x".repeat(201) } }), "needs `name`"),
        (json!({ "agent": { "id": "ada" } }), "name"),
        (json!({ "agent": { "id": "ada", "name": "Ada" }, "opts": { "port": 1 } }), "port"),
        (json!({ "agent": { "id": "ada", "name": "Ada" }, "opts": { "host": "bad" } }), "not a host name"),
    ] {
        let out = ask(&vm, "refused", args.clone()).await;
        assert_eq!(out["ok"], false, "{args}");
        assert!(out["err"].as_str().unwrap().contains(why), "{args}: {out}");
    }
    let ro =
        common::serve_ro(&vm, "lease", json!({ "agent": { "id": "ada", "name": "Ada" } }), &agent_ctx(0))
            .await
            .unwrap_err();
    assert!(ro.to_string().contains("read-only"), "{ro}");
    let ro = common::serve_ro(&vm, "release", json!({ "token": "t" }), &agent_ctx(0)).await.unwrap_err();
    assert!(ro.to_string().contains("read-only"), "{ro}");
    assert!(desk.asked().is_empty(), "nothing was leased or released");
}

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _n: Notification) -> PortResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn an_apps_leases_go_when_its_backend_stops_or_is_replaced() {
    let desk = Arc::new(Desk::default());
    let runtime =
        LuaRuntime::new(Arc::new(Quiet), Arc::new(RecordingShell::default())).with_leases(desk.clone());
    let load = || async {
        let data: Arc<dyn AppData> = db();
        runtime
            .load(
                &id("probe"),
                package(
                    &format!(
                        "{MANIFEST}
{BROKER}"
                    ),
                    &[],
                ),
                data,
            )
            .await
            .unwrap();
    };
    load().await;
    assert!(desk.asked().is_empty(), "a first start revokes nothing");
    load().await;
    assert_eq!(desk.asked(), ["release_all probe"], "a reload stops the old backend");
    runtime.unload(&id("probe")).await.unwrap();
    assert_eq!(desk.asked(), ["release_all probe", "release_all probe"]);
    runtime.unload(&id("probe")).await.unwrap();
    assert_eq!(desk.asked().len(), 2, "no backend, nothing to stop");
}
