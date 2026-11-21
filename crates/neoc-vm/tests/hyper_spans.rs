#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! The module convention (`facade.rs`): every registered function opens a `tracing` span named `"<stem>.<name>"` at target
//! `lua::lib::<module>`. `vnd:hyper`'s client functions read their options before they build their future, so the
//! span is opened by hand in `client::send`; this holds the names (`hyper.get`, `hyper.post`, `hyper.request`) and the
//! target to what the `#[instrument]` attributes they replaced gave, so a subscriber filtering on one still sees it.

use std::sync::{Arc, Mutex};

use neoc_vm::lua::sandbox::make_engine;
use tracing_core::{
    Event, Metadata,
    span::{Attributes, Id, Record},
    subscriber::Subscriber,
};

/// A subscriber that records the name and target of every span it is told of.
struct Recorder {
    spans: Arc<Mutex<Vec<(String, String)>>>,
}

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let meta = attributes.metadata();
        let mut spans = self.spans.lock().unwrap();
        spans.push((meta.name().to_owned(), meta.target().to_owned()));
        Id::from_u64(spans.len() as u64)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[test]
fn each_client_function_opens_a_span_named_after_itself() {
    let spans = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Recorder {
        spans: spans.clone(),
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tracing::subscriber::with_default(subscriber, || {
        rt.block_on(async {
            let lua = make_engine().unwrap();
            // nothing listens on port 1: every call ends at once with `(nil, err)`
            lua.load(
                r#"local hyper = require("vnd:hyper")
                   hyper.get("http://127.0.0.1:1/")
                   hyper.post({ url = "http://127.0.0.1:1/", body = "x" })
                   hyper.request({ method = "PUT", url = "http://127.0.0.1:1/" })
                   hyper.request({ method = "PUT" }) -- options that cannot be read: the span is there too"#,
            )
            .exec_async()
            .await
            .unwrap();
        });
    });
    let spans = spans.lock().unwrap();
    for name in ["hyper.get", "hyper.post", "hyper.request"] {
        let found = spans.iter().filter(|(n, _)| n == name).count();
        assert!(found >= 1, "no `{name}` span in {spans:?}");
        assert!(
            spans
                .iter()
                .filter(|(n, _)| n == name)
                .all(|(_, target)| target == "lua::lib::hyper"),
            "{name}: wrong target in {spans:?}"
        );
    }
    assert_eq!(
        spans.iter().filter(|(n, _)| n == "hyper.request").count(),
        2,
        "both `request` calls, the unreadable one included: {spans:?}"
    );
    assert!(spans.iter().all(|(n, _)| n != "hyper.client"), "{spans:?}");
}
