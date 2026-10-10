// TODO: clean-code - 0.7 - DEAD: this whole module (scope, LuaLayer, CURRENT_RUN, RunBuffers, drain) has zero consumers — LuaLayer is never registered as a subscriber and lib:vcr does not use it (the doc itself says the "not yet wired" service layer will). Land it behind the feature that consumes it, or remove until then. dto::procedures (TracingEvent/TracingSpan) is dead with it.
//! Per-run capture buckets + a `tracing_subscriber` Layer that funnels every
//! event and span emitted at a `lua*` target into the active run's
//! buffers. No transformation: clients receive tracing-shaped records.
//!
//! - `tracing::info!(target: "lua", ...)` → `TracingEvent` in `events`
//! - `tracing::event!(target: "lua::assert", ...)` → `TracingEvent` (clients
//!   filter by target)
//! - `tracing::info_span!(target: "lua::lib::*", ...)` → `TracingSpan` in
//!   `spans` when the span closes
//!
//! Run scoping uses a tokio task-local so concurrent runs each maintain
//! their own buffers. Stdlib modules emit through plain `tracing` macros;
//! there is no bespoke recorder API.
//!
//! # Intended use: VCR / mock generation
//!
//! `scope` is the entry point for recording a real interaction at the Lua
//! boundary. Wrap a procedure execution in `scope`, serialise the returned
//! events and spans, and you have a replayable cassette. The service layer
//! (not yet wired) will use this to generate mocks without hand-authoring
//! fixture data.

use parking_lot::Mutex;
use serde_json::{Map, Value as Json};
use std::sync::Arc;
use std::time::Instant;
use tokio::task_local;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use crate::dto::procedures::{TracingEvent, TracingSpan};

/// Per-run capture buckets. Created at the start of every `run_procedure` /
/// `run_procedure_test` / `run_snippet` call; drained at the end.
pub struct RunBuffers {
    pub started_at: Instant,
    events: Mutex<Vec<TracingEvent>>,
    spans: Mutex<Vec<TracingSpan>>,
}

impl Default for RunBuffers {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            events: Mutex::default(),
            spans: Mutex::default(),
        }
    }
}

impl RunBuffers {
    pub fn drain(&self) -> (Vec<TracingEvent>, Vec<TracingSpan>) {
        let events = std::mem::take(&mut *self.events.lock());
        let spans = std::mem::take(&mut *self.spans.lock());
        (events, spans)
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

task_local! {
    /// The active run's buffers. `LuaLayer` writes here on every
    /// `lua*`-targeted event / span. Reading is via `try_with` so that
    /// non-run code paths (e.g. tests, the actix-web server itself) emit
    /// tracing without touching this and without panicking.
    pub static CURRENT_RUN: Arc<RunBuffers>;
}

/// Wrap a future so every `lua*` `tracing` event / span emitted within is
/// captured. Returns the future's output plus the captured events / spans.
pub async fn scope<F, T>(future: F) -> (T, Vec<TracingEvent>, Vec<TracingSpan>)
where
    F: std::future::Future<Output = T>,
{
    let buffers = Arc::new(RunBuffers::default());
    let drain_handle = buffers.clone();
    let out = CURRENT_RUN.scope(buffers, future).await;
    let (events, spans) = drain_handle.drain();
    (out, events, spans)
}

/// Per-span state stashed in span extensions when the span opens, read on
/// close to materialise a `TracingSpan`.
#[derive(Debug)]
struct SpanState {
    started_at: Instant,
    started_at_ms_since_run: u64,
    fields: Map<String, Json>,
}

/// True for the `lua` target and its `lua::` children — not `luajit`, `lua_*`
/// or other crates that merely share the prefix.
fn is_lua_target(target: &str) -> bool {
    target == "lua" || target.starts_with("lua::")
}

/// `tracing_subscriber::Layer` that captures `lua`/`lua::*`-targeted events and
/// spans into the active `CURRENT_RUN` buffers.
pub struct LuaLayer;

impl<S> Layer<S> for LuaLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if !is_lua_target(attrs.metadata().target()) {
            return;
        }
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = JsonVisitor::default();
        attrs.values().record(&mut visitor);
        let started_at = Instant::now();
        let started_at_ms_since_run = CURRENT_RUN
            .try_with(|run| started_at.duration_since(run.started_at).as_millis() as u64)
            .unwrap_or(0);
        span.extensions_mut().insert(SpanState {
            started_at,
            started_at_ms_since_run,
            fields: visitor.0,
        });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if !is_lua_target(span.metadata().target()) {
            return;
        }
        let mut visitor = JsonVisitor::default();
        values.record(&mut visitor);
        if let Some(state) = span.extensions_mut().get_mut::<SpanState>() {
            for (k, v) in visitor.0 {
                state.fields.insert(k, v);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !is_lua_target(event.metadata().target()) {
            return;
        }
        let _ = CURRENT_RUN.try_with(|run| {
            let mut visitor = JsonVisitor::default();
            event.record(&mut visitor);
            let parent_id = ctx.event_span(event).map(|s| s.id().into_u64());
            run.events.lock().push(TracingEvent {
                level: event.metadata().level().to_string(),
                target: event.metadata().target().to_string(),
                fields: Json::Object(visitor.0),
                ts_ms: run.started_at.elapsed().as_millis() as u64,
                span_id: parent_id,
            });
        });
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        if !is_lua_target(span.metadata().target()) {
            return;
        }
        let _ = CURRENT_RUN.try_with(|run| {
            let exts = span.extensions();
            let Some(state) = exts.get::<SpanState>() else {
                return;
            };
            let duration_ms = state.started_at.elapsed().as_millis() as u64;
            run.spans.lock().push(TracingSpan {
                level: span.metadata().level().to_string(),
                target: span.metadata().target().to_string(),
                name: span.metadata().name().to_string(),
                id: id.into_u64(),
                parent_id: span.parent().map(|p| p.id().into_u64()),
                fields: Json::Object(state.fields.clone()),
                started_at_ms: state.started_at_ms_since_run,
                duration_ms,
            });
        });
    }
}

#[derive(Default, Debug)]
struct JsonVisitor(Map<String, Json>);

impl Visit for JsonVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .insert(field.name().into(), Json::String(value.into()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0
            .insert(field.name().into(), Json::Number(value.into()));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0
            .insert(field.name().into(), Json::Number(value.into()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().into(), Json::Bool(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        let n = serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into());
        self.0.insert(field.name().into(), Json::Number(n));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), Json::String(format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::is_lua_target;

    #[test]
    fn lua_target_and_children_are_captured() {
        assert!(is_lua_target("lua"));
        assert!(is_lua_target("lua::assert"));
        assert!(is_lua_target("lua::lib::archive"));
    }

    #[test]
    fn prefix_sharing_targets_are_ignored() {
        assert!(!is_lua_target("luau"));
        assert!(!is_lua_target("luajit::trace"));
        assert!(!is_lua_target("lua_bridge"));
        assert!(!is_lua_target("mlua"));
    }
}
