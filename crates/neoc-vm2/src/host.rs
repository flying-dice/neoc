//! How the modules written with mlua-ir take part in neoc's runtime: one span per call, and every async body
//! under the lifecycle's rules ([`crate::lifecycle`]).
//!
//! mlua-ir tells the host each call; a function or method says its role with `#[host("Mixed")]` (the
//! [`Role`] names: `Vm`, `Wait`, `Timer`, `Mixed`) and defaults to `Wait`.

use std::any::Any;

use mlua_ir::{Body, Call, Host};
use tracing::Instrument as _;

use crate::lifecycle::{Role, leaf};
use crate::span::dyn_span;

struct Neoc;

/// Installs the host, once per process (the engine factory does it).
pub fn install() {
    mlua_ir::set_host(Neoc);
}

/// The call's span: named `<module stem>.<function>`, at target `lua::lib::<module stem>`.
fn span(call: &Call) -> tracing::Span {
    let module = call.module.unwrap_or("globals");
    let stem = module.split_once(':').map_or(module, |(_, stem)| stem);
    dyn_span(&format!("lua::lib::{stem}"), &format!("{stem}.{}", call.name))
}

fn role(hint: Option<&str>) -> Role {
    match hint {
        None | Some("Wait") => Role::Wait,
        Some("Vm") => Role::Vm,
        Some("Timer") => Role::Timer,
        Some("Mixed") => Role::Mixed,
        #[allow(clippy::panic)] // a typo in a role is a bug in the module, found by its first test
        Some(other) => panic!("`#[host({other:?})]` is not a role: Vm, Wait, Timer or Mixed"),
    }
}

impl Host for Neoc {
    fn enter(&self, call: &Call) -> Option<Box<dyn Any>> {
        Some(Box::new(span(call).entered()))
    }

    fn wrap(&self, call: &Call, body: Body) -> Body {
        Box::pin(leaf(role(call.hint), body.instrument(span(call))))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_core::span::{Attributes, Id};
    use tracing_core::{Event, Subscriber};

    use crate::make_engine;

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

    #[test]
    #[should_panic(expected = "is not a role")]
    fn a_role_that_does_not_exist_is_a_bug_found_at_the_first_call() {
        let _ = super::role(Some("Quick"));
    }
}
