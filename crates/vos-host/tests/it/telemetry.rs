#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Telemetry (NEOC-521 POC): a page's call, the host's `app.call` span and the backend's `lib:log` line share the
//! page's trace, and what the frame recorded joins it, all read back by one trace id.

use crate::common::{browser, built_backend, publish_package, rpc, start, temp};
use serde_json::json;
use tracing_subscriber::layer::SubscriberExt;
use vos_host::startup::prepare_data_dir;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e47aa";
const PAGE_SPAN: &str = "00f067aa0ba902aa";

#[tokio::test]
async fn the_page_the_host_and_the_backend_share_one_trace() {
    // process-wide, as the host's own: a thread-local one misses the spans of the host's tasks once other tests have run
    // in this binary (their callsites are cached as unwanted, and the host's work is not all on this thread)
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(vos_http::telemetry::TelemetryLayer),
    );
    let tmp = temp("telemetry");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| c.data_dir = Some(tmp.join("data"))).await;
    let files = [("app.luau", built_backend("traced"))];
    let url = publish_package(&tmp.join("pkg-traced"), "traced", "app.luau", &files);
    r.host.service.install_from_url(&url).await.unwrap();
    let mut ws = browser(&r).await;

    let call = json!({
        "app": "traced", "method": "_overview", "args": {},
        "traceparent": format!("00-{TRACE}-{PAGE_SPAN}-01"),
    });
    let reply = rpc(&mut ws, 1, "app.call", call).await;
    assert_eq!(reply["result"], json!({"units": 3}), "{reply}");

    // what the frame recorded, passed on by the shell
    let page_span = json!({
        "traceId": TRACE, "spanId": PAGE_SPAN, "name": "rpc overview", "source": "browser",
        "startMs": 1.0, "endMs": 2.0, "status": "ok",
    });
    let page_error = json!({
        "timeMs": 3.0, "severity": "error", "body": "each_key_duplicate", "source": "browser", "traceId": TRACE,
    });
    let ingest = json!({"app": "traced", "spans": [page_span], "logs": [page_error]});
    assert_eq!(rpc(&mut ws, 2, "host.telemetry.ingest", ingest).await["result"]["kept"], 2);

    let found = rpc(&mut ws, 3, "host.telemetry.query", json!({"traceId": TRACE})).await;
    let spans = found["result"]["spans"].as_array().unwrap();
    let logs = found["result"]["logs"].as_array().unwrap();
    let by_name = |name: &str| spans.iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("{found}"));
    let host = by_name("app.call");
    assert_eq!(
        (&host["parentSpanId"], &host["source"], &host["status"], &host["app"]),
        (&json!(PAGE_SPAN), &json!("host"), &json!("ok"), &json!("traced")),
        "{found}"
    );
    assert_eq!(by_name("rpc overview")["source"], "browser");
    // the backend's log, in the span of the runtime call that wrote it (`log.info`), under the host's span
    let line = logs.iter().find(|l| l["body"] == "read the overview").unwrap_or_else(|| panic!("{found}"));
    assert_eq!((&line["source"], &line["attributes"]["units"]), (&json!("backend"), &json!(3)), "{found}");
    let written_in =
        spans.iter().find(|s| s["spanId"] == line["spanId"]).unwrap_or_else(|| panic!("{found}"));
    assert_eq!(
        (&written_in["parentSpanId"], &written_in["source"]),
        (&host["spanId"], &json!("backend")),
        "{found}"
    );
    assert!(logs.iter().any(|l| l["source"] == "browser" && l["body"] == "each_key_duplicate"), "{found}");

    // the backend's own span (`lib:tracing`): a child of the host's, with its fields, its event in it, and the event
    // of the task it started in it
    let own = by_name("list units");
    assert_eq!(
        (&own["parentSpanId"], &own["source"], &own["status"], &own["attributes"]["scope"]),
        (&host["spanId"], &json!("backend"), &json!("ok"), &json!("system")),
        "{found}"
    );
    let event = logs.iter().find(|l| l["body"] == "listed units").unwrap_or_else(|| panic!("{found}"));
    assert_eq!((&event["spanId"], &event["attributes"]["units"]), (&own["spanId"], &json!(3)), "{found}");
    let background =
        logs.iter().find(|l| l["body"] == "counted in the background").unwrap_or_else(|| panic!("{found}"));
    assert_eq!(background["traceId"], TRACE, "a task carries the span it was started in: {found}");

    let listed = rpc(&mut ws, 4, "host.telemetry.traces", json!({"app": "traced"})).await;
    assert!(
        listed["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["traceId"] == TRACE && t["name"] == "rpc overview"),
        "the page's span is the trace's root: {listed}"
    );
    r.stop().await;
}
