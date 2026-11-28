//! How the modules written with mlua-ir take part in neoc's runtime: one span per call, and every async body
//! under the lifecycle's rules ([`crate::lifecycle`]). An async call's span says how it ended (`outcome`, and the
//! `error` it raised), so a call that failed is a failed span in a trace, whether or not the script caught it.
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

/// `lib:tracing` opens the script's own spans: a span of its own around each of its calls would only stand between
/// the script's span and its parent.
fn spanned(call: &Call) -> bool {
    call.module != Some("lib:tracing")
}

/// The first line of what a call raised: the message, without the Lua stack under it.
fn error_text(e: &mlua::Error) -> String {
    let text = e.to_string();
    let message = text
        .split_once(
            "
stack traceback:",
        )
        .map_or(text.as_str(), |(message, _)| message);
    message.trim().to_owned()
}

/// `body`, with how it ended recorded on `span` (`outcome` and, when it raised, `error`).
fn recorded(span: tracing::Span, body: Body) -> Body {
    Box::pin(async move {
        let out = body.await;
        match &out {
            Ok(_) => span.record("outcome", "ok"),
            Err(e) => span.record("outcome", "error").record("error", error_text(e).as_str()),
        };
        out
    })
}

impl Host for Neoc {
    fn enter(&self, call: &Call) -> Option<Box<dyn Any>> {
        spanned(call).then(|| Box::new(span(call).entered()) as Box<dyn Any>)
    }

    fn wrap(&self, call: &Call, body: Body) -> Body {
        if spanned(call) {
            let span = span(call);
            Box::pin(leaf(role(call.hint), recorded(span.clone(), body).instrument(span)))
        } else {
            Box::pin(leaf(role(call.hint), body))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing_core::span::{Attributes, Id, Record};
    use tracing_core::{Event, Metadata, Subscriber};

    /// Every field recorded on a span after it was made, as `name=value`.
    #[derive(Clone, Default)]
    struct Recorded(Arc<Mutex<Vec<String>>>);

    impl Visit for Recorded {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.lock().unwrap().push(format!("{}={value:?}", field.name()));
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.lock().unwrap().push(format!("{}={value}", field.name()));
        }
    }

    impl Subscriber for Recorded {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, values: &Record<'_>) {
            values.record(&mut self.clone());
        }
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    fn outcome_of(body: super::Body) -> Vec<String> {
        let recorder = Recorded::default();
        let seen = recorder.0.clone();
        tracing::subscriber::with_default(recorder, || {
            let span = crate::span::dyn_span("lua::lib::test", "test.call");
            let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let _ = runtime.block_on(super::recorded(span, body));
        });
        seen.lock().unwrap().clone()
    }

    #[test]
    fn a_call_that_raised_is_a_failed_span_with_its_message_and_no_stack() {
        let seen = outcome_of(Box::pin(async {
            Err(mlua::Error::runtime(
                "connect: the pipe is not there
stack traceback:
	[C]: in ?",
            ))
        }));
        assert_eq!(seen, ["outcome=error", "error=runtime error: connect: the pipe is not there"]);
    }

    #[test]
    fn a_call_that_returned_is_an_ok_span() {
        let seen = outcome_of(Box::pin(async { Ok(Box::new(()) as mlua_ir::Erased) }));
        assert_eq!(seen, ["outcome=ok"]);
    }

    #[test]
    #[should_panic(expected = "is not a role")]
    fn a_role_that_does_not_exist_is_a_bug_found_at_the_first_call() {
        let _ = super::role(Some("Quick"));
    }
}
