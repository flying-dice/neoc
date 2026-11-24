//! A call of a module opens a span named after it: the host (`neoc-vm-core`'s `host`) wraps every call of the modules
//! this crate links.

use std::sync::{Arc, Mutex};

use neoc_vm::make_engine;
use tracing_core::span::{Attributes, Id};
use tracing_core::{Event, Subscriber};

/// Records `(target, name)` of every span it is asked to create.
#[derive(Default)]
struct SpanRecorder {
    spans: Arc<Mutex<Vec<(String, String)>>>,
}

impl Subscriber for SpanRecorder {
    fn enabled(&self, _: &tracing_core::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let m = span.metadata();
        self.spans.lock().unwrap().push((m.target().to_owned(), m.name().to_owned()));
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &tracing_core::span::Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// A call opens a span named `<stem>.<function>` at target `lua::lib::<stem>`: a sync function, a static, a
/// method and an async function alike.
#[test]
fn every_call_opens_a_span_named_after_the_function() {
    let recorder = SpanRecorder::default();
    let spans = recorder.spans.clone();
    let lua = make_engine().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    tracing::subscriber::with_default(recorder, || {
        runtime.block_on(async {
            let _: () = lua
                .load(
                    r#"
                    local b64, time = require("lib:base64"), require("std:time")
                    b64.encode("hi")
                    local d = time.Duration.from_millis(1)
                    d:as_millis()
                    time.sleep(d)
                "#,
                )
                .exec_async()
                .await
                .unwrap();
        });
    });
    let seen = spans.lock().unwrap().clone();
    let has = |target: &str, name: &str| seen.iter().any(|(t, n)| t == target && n == name);
    assert!(has("lua::lib::base64", "base64.encode"), "{seen:?}");
    assert!(has("lua::lib::time", "time.Duration.from_millis"), "{seen:?}");
    assert!(has("lua::lib::time", "time.Duration.as_millis"), "{seen:?}");
    assert!(has("lua::lib::time", "time.sleep"), "{seen:?}");
}
