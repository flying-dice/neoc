//! Telemetry (proof of concept for NEOC-521): spans and logs, shaped after OpenTelemetry, kept in memory and read by
//! trace id.
//!
//! - [`TelemetryLayer`] sits next to the journal's fmt layer: every `tracing` span the host opens becomes a span record
//!   (a span named with a `trace_id` field joins that trace, as `app.call` does with the page's `traceparent`), and
//!   every event becomes a log record under the span it was written in. `lib:log` lines are events too, so a
//!   backend's log is in the trace of the call that wrote it.
//! - [`ingest`] takes what a frame recorded (its RPC spans, its errors) from the shell, stamped with the frame's app.
//! - [`query`] and [`traces`] read it back.
//!
//! Bounded: the newest [`MAX_SPANS`] spans and [`MAX_LOGS`] logs are kept. Not kept: an attribute whose key looks like
//! a credential, and anything over [`MAX_TEXT`] characters is cut.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Write as _,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{
    field::{Field, Visit},
    span, Event, Subscriber,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

pub const MAX_SPANS: usize = 5_000;
pub const MAX_LOGS: usize = 10_000;
pub const MAX_TEXT: usize = 2_000;
/// The most records one `ingest` takes.
pub const MAX_INGEST: usize = 200;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanRecord {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub name: String,
    /// `browser`, `host` or `backend`.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub start_ms: f64,
    pub end_ms: f64,
    /// `ok`, `error` or `unset`.
    pub status: String,
    #[serde(default)]
    pub attributes: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub time_ms: f64,
    /// `trace`, `debug`, `info`, `warn` or `error`.
    pub severity: String,
    pub body: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default)]
    pub attributes: Map<String, Value>,
}

#[derive(Default)]
struct Store {
    spans: Mutex<VecDeque<SpanRecord>>,
    logs: Mutex<VecDeque<LogRecord>>,
}

static STORE: LazyLock<Store> = LazyLock::new(Store::default);

fn push<T>(queue: &Mutex<VecDeque<T>>, max: usize, item: T) {
    let mut q = queue.lock();
    if q.len() >= max {
        q.pop_front();
    }
    q.push_back(item);
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

fn random_hex(bytes: usize) -> String {
    let mut buf = [0u8; 16];
    let buf = &mut buf[..bytes];
    if getrandom::fill(buf).is_err() {
        // no randomness: the clock and a counter, unique within this process
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seed =
            now_ms().to_bits() ^ NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).rotate_left(32);
        for (b, s) in buf.iter_mut().zip(seed.to_le_bytes().iter().cycle()) {
            *b = *s;
        }
    }
    buf.iter().fold(String::with_capacity(bytes * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()) && s.bytes().any(|b| b != b'0')
}

/// A W3C `traceparent` (`00-<trace id>-<parent span id>-<flags>`): its trace id and parent span id.
#[must_use]
pub fn parse_traceparent(header: &str) -> Option<(String, String)> {
    let mut parts = header.trim().split('-');
    let (version, trace, parent, _flags) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (version == "00" && is_hex(trace, 32) && is_hex(parent, 16))
        .then(|| (trace.to_ascii_lowercase(), parent.to_ascii_lowercase()))
}

fn is_secret(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["authorization", "token", "password", "secret", "cookie", "credential", "api_key", "apikey"]
        .iter()
        .any(|s| k.contains(s))
}

fn cut(mut s: String) -> String {
    if s.chars().count() > MAX_TEXT {
        s = s.chars().take(MAX_TEXT).collect::<String>() + "…";
    }
    s
}

fn clean(attributes: Map<String, Value>) -> Map<String, Value> {
    attributes
        .into_iter()
        .filter(|(k, _)| !is_secret(k))
        .map(|(k, v)| {
            let v = match v {
                Value::String(s) => Value::String(cut(s)),
                v @ (Value::Null | Value::Bool(_) | Value::Number(_)) => v,
                other => {
                    let text = other.to_string();
                    if text.len() > MAX_TEXT {
                        Value::String(cut(text))
                    } else {
                        other
                    }
                }
            };
            (k, v)
        })
        .collect()
}

// ---- the tracing layer -----------------------------------------------------------------------------------------

/// What a span carries while it is open.
struct Open {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    app: Option<String>,
    source: String,
    start_ms: f64,
    attributes: Map<String, Value>,
}

#[derive(Default)]
struct Fields {
    message: Option<String>,
    values: Map<String, Value>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::String(value.to_owned()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::Bool(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::String(format!("{value:?}")));
    }
}

impl Fields {
    fn put(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            self.message = Some(match value {
                Value::String(s) => s,
                other => other.to_string(),
            });
        } else {
            self.values.insert(field.name().to_owned(), value);
        }
    }
}

/// A script's span (`lib:tracing`) carries its fields as one JSON object under `fields`, each time it records: they
/// are spread into the span's own attributes.
fn spread_fields(values: &mut Map<String, Value>) {
    if let Some(Value::String(text)) = values.remove("fields") {
        if let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(&text) {
            values.extend(fields);
        }
    }
}

fn take_str(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    match map.remove(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Records every span and event into the store. Register it next to the fmt layer.
pub struct TelemetryLayer;

impl<S> Layer<S> for TelemetryLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let mut values = fields.values;
        spread_fields(&mut values);
        // what the parent span carries, copied out (its trace, its id, its app and source)
        let inherited: Option<(String, String, Option<String>, String)> = span.parent().and_then(|p| {
            p.extensions()
                .get::<Open>()
                .map(|o| (o.trace_id.clone(), o.span_id.clone(), o.app.clone(), o.source.clone()))
        });
        let parent_open = inherited.as_ref();
        let explicit_trace = take_str(&mut values, "trace_id");
        let explicit_parent = take_str(&mut values, "parent_span_id");
        let (trace_id, parent_span_id) = match explicit_trace {
            Some(t) => (t, explicit_parent),
            None => match parent_open {
                Some(p) => (p.0.clone(), Some(p.1.clone())),
                None => (random_hex(16), None),
            },
        };
        let app = take_str(&mut values, "app").or_else(|| parent_open.and_then(|p| p.2.clone()));
        // a span the app's VM opens (a runtime call such as `log.info`) is the backend's
        let target = attrs.metadata().target();
        let in_vm = app.is_some() && (target.starts_with("lua::") || target.starts_with("vos_lua"));
        let source = take_str(&mut values, "source")
            .or_else(|| in_vm.then(|| "backend".to_owned()))
            .or_else(|| parent_open.map(|p| p.3.clone()))
            .unwrap_or_else(|| "host".into());
        span.extensions_mut().insert(Open {
            trace_id,
            span_id: random_hex(8),
            parent_span_id,
            app,
            source,
            start_ms: now_ms(),
            attributes: values,
        });
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut values = fields.values;
        spread_fields(&mut values);
        let mut extensions = span.extensions_mut();
        if let Some(open) = extensions.get_mut::<Open>() {
            open.attributes.extend(values);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut attributes = fields.values;
        let mut body = fields.message.unwrap_or_default();
        // a `lib:log` line is a JSON object (`level`, `ts`, `msg`, then the fields): keep its fields as attributes
        if body.starts_with('{') {
            if let Ok(Value::Object(mut line)) = serde_json::from_str::<Value>(&body) {
                line.remove("level");
                line.remove("ts");
                body = take_str(&mut line, "msg").unwrap_or_default();
                attributes.extend(line);
            }
        }
        let (trace_id, span_id, span_app, span_source) = ctx
            .event_span(event)
            .and_then(|s| {
                s.extensions().get::<Open>().map(|o| {
                    (Some(o.trace_id.clone()), Some(o.span_id.clone()), o.app.clone(), o.source.clone())
                })
            })
            .unwrap_or((None, None, None, "host".into()));
        let app = take_str(&mut attributes, "app").or(span_app);
        // a line the app's backend wrote is the backend's, whatever span it was written in
        let source = if event.metadata().target().starts_with("vos_lua") && app.is_some() {
            "backend".to_owned()
        } else {
            span_source
        };
        attributes.insert("target".into(), Value::String(event.metadata().target().to_owned()));
        push(
            &STORE.logs,
            MAX_LOGS,
            LogRecord {
                time_ms: now_ms(),
                severity: event.metadata().level().as_str().to_ascii_lowercase(),
                body: cut(body),
                source,
                app,
                trace_id,
                span_id,
                attributes: clean(attributes),
            },
        );
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let Some(open) = span.extensions_mut().remove::<Open>() else { return };
        let mut attributes = open.attributes;
        let status = match take_str(&mut attributes, "outcome").as_deref() {
            Some("ok") => "ok",
            Some("error") => "error",
            _ => "unset",
        };
        push(
            &STORE.spans,
            MAX_SPANS,
            SpanRecord {
                trace_id: open.trace_id,
                span_id: open.span_id,
                parent_span_id: open.parent_span_id,
                name: span.name().to_owned(),
                source: open.source,
                app: open.app,
                start_ms: open.start_ms,
                end_ms: now_ms(),
                status: status.into(),
                attributes: clean(attributes),
            },
        );
    }
}

// ---- what a frame recorded ---------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Batch {
    pub spans: Vec<SpanRecord>,
    pub logs: Vec<LogRecord>,
}

/// Keep what a frame of `app` recorded. The app and source are the shell's to say, never the frame's; ids that are
/// not W3C ids are refused; at most [`MAX_INGEST`] records are taken. Returns how many were kept.
pub fn ingest(app: &str, batch: Batch) -> usize {
    let mut kept = 0;
    for mut s in batch.spans.into_iter().take(MAX_INGEST) {
        if !is_hex(&s.trace_id, 32) || !is_hex(&s.span_id, 16) {
            continue;
        }
        s.parent_span_id = s.parent_span_id.filter(|p| is_hex(p, 16));
        s.app = Some(app.to_owned());
        s.source = "browser".into();
        s.name = cut(s.name);
        if !matches!(s.status.as_str(), "ok" | "error") {
            s.status = "unset".into();
        }
        s.attributes = clean(s.attributes);
        push(&STORE.spans, MAX_SPANS, s);
        kept += 1;
    }
    for mut l in batch.logs.into_iter().take(MAX_INGEST.saturating_sub(kept)) {
        l.trace_id = l.trace_id.filter(|t| is_hex(t, 32));
        l.span_id = l.span_id.filter(|s| is_hex(s, 16));
        l.app = Some(app.to_owned());
        l.source = "browser".into();
        l.body = cut(l.body);
        if !matches!(l.severity.as_str(), "trace" | "debug" | "info" | "warn" | "error") {
            l.severity = "info".into();
        }
        l.attributes = clean(l.attributes);
        push(&STORE.logs, MAX_LOGS, l);
        kept += 1;
    }
    kept
}

// ---- reading it back ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub trace_id: Option<String>,
    pub app: Option<String>,
    /// The lowest severity: `debug` keeps debug and above.
    pub severity: Option<String>,
    pub limit: Option<usize>,
}

fn rank(severity: &str) -> u8 {
    match severity {
        "trace" => 0,
        "debug" => 1,
        "info" => 2,
        "warn" => 3,
        _ => 4,
    }
}

#[derive(Debug, Serialize)]
pub struct Found {
    pub spans: Vec<SpanRecord>,
    pub logs: Vec<LogRecord>,
}

/// The last `limit` of `v`.
fn last<T>(v: Vec<T>, limit: usize) -> Vec<T> {
    let skip = v.len().saturating_sub(limit);
    v.into_iter().skip(skip).collect()
}

/// The spans and logs that match, newest last, at most `limit` (500) of each.
#[must_use]
pub fn query(q: &Query) -> Found {
    let limit = q.limit.unwrap_or(500).min(MAX_LOGS);
    let floor = q.severity.as_deref().map_or(0, rank);
    let matches = |trace: Option<&str>, app: Option<&str>| {
        q.trace_id.as_deref().is_none_or(|t| trace == Some(t))
            && q.app.as_deref().is_none_or(|a| app == Some(a))
    };
    let spans =
        STORE.spans.lock().iter().filter(|s| matches(Some(&s.trace_id), s.app.as_deref())).cloned().collect();
    let logs = STORE
        .logs
        .lock()
        .iter()
        .filter(|l| rank(&l.severity) >= floor && matches(l.trace_id.as_deref(), l.app.as_deref()))
        .cloned()
        .collect();
    Found { spans: last(spans, limit), logs: last(logs, limit) }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSummary {
    pub trace_id: String,
    /// The root span's name (the earliest span when the root was not recorded).
    pub name: String,
    pub app: Option<String>,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub spans: usize,
    pub errors: usize,
}

/// The newest traces that have an app in them (or `app`'s), newest first, at most `limit` (100).
#[must_use]
pub fn traces(app: Option<&str>, limit: Option<usize>) -> Vec<TraceSummary> {
    let mut by_trace: HashMap<String, Vec<SpanRecord>> = HashMap::new();
    for s in STORE.spans.lock().iter() {
        by_trace.entry(s.trace_id.clone()).or_default().push(s.clone());
    }
    let errors_in = |trace: &str| {
        STORE
            .logs
            .lock()
            .iter()
            .filter(|l| l.trace_id.as_deref() == Some(trace) && l.severity == "error")
            .count()
    };
    let mut out: Vec<TraceSummary> = by_trace
        .into_iter()
        .filter_map(|(trace_id, spans)| {
            let trace_app = spans.iter().find_map(|s| s.app.clone());
            if trace_app.is_none() || app.is_some_and(|a| trace_app.as_deref() != Some(a)) {
                return None;
            }
            let start = spans.iter().map(|s| s.start_ms).fold(f64::INFINITY, f64::min);
            let end = spans.iter().map(|s| s.end_ms).fold(f64::NEG_INFINITY, f64::max);
            let ids: Vec<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
            let root = spans
                .iter()
                .filter(|s| s.parent_span_id.as_deref().is_none_or(|p| !ids.contains(&p)))
                .min_by(|a, b| a.start_ms.total_cmp(&b.start_ms))?;
            let errors = spans.iter().filter(|s| s.status == "error").count() + errors_in(&trace_id);
            Some(TraceSummary {
                name: root.name.clone(),
                app: trace_app,
                start_ms: start,
                duration_ms: end - start,
                spans: spans.len(),
                errors,
                trace_id,
            })
        })
        .collect();
    out.sort_by(|a, b| b.start_ms.total_cmp(&a.start_ms));
    out.truncate(limit.unwrap_or(100));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Instrument;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn a_traceparent_is_read_and_a_bad_one_is_not() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(
            parse_traceparent(tp),
            Some(("4bf92f3577b34da6a3ce929d0e0e4736".into(), "00f067aa0ba902b7".into()))
        );
        assert_eq!(parse_traceparent("00-0000-00f067aa0ba902b7-01"), None);
        assert_eq!(parse_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01"), None);
    }

    #[tokio::test]
    async fn a_span_with_a_trace_id_joins_it_and_its_events_and_children_follow() {
        let subscriber = tracing_subscriber::registry().with(TelemetryLayer);
        let _guard = tracing::subscriber::set_default(subscriber);
        let trace = "4bf92f3577b34da6a3ce929d0e0e4737";
        let span = tracing::info_span!(
            "app.call",
            app = "demo",
            trace_id = trace,
            parent_span_id = "00f067aa0ba902b7",
            outcome = tracing::field::Empty
        );
        async {
            tracing::info!(target: "vos_lua::vm", app = "demo", r#"{{"level":"info","ts":"x","msg":"hi","n":1,"token":"no"}}"#);
            tracing::info_span!("inner").in_scope(|| tracing::warn!("careful"));
        }
        .instrument(span.clone())
        .await;
        span.record("outcome", "ok");
        drop(span);

        let found = query(&Query { trace_id: Some(trace.into()), ..Query::default() });
        assert_eq!(found.spans.len(), 2, "{:?}", found.spans);
        let root = found.spans.iter().find(|s| s.name == "app.call").unwrap();
        assert_eq!(root.parent_span_id.as_deref(), Some("00f067aa0ba902b7"));
        assert_eq!(root.status, "ok");
        assert_eq!(root.app.as_deref(), Some("demo"));
        let inner = found.spans.iter().find(|s| s.name == "inner").unwrap();
        assert_eq!(inner.parent_span_id.as_deref(), Some(root.span_id.as_str()));

        assert_eq!(found.logs.len(), 2);
        let line = &found.logs[0];
        assert_eq!((line.body.as_str(), line.source.as_str()), ("hi", "backend"));
        assert_eq!(line.attributes.get("n"), Some(&Value::from(1)));
        assert!(!line.attributes.contains_key("token"), "a credential-like key is dropped");
        assert_eq!(found.logs[1].span_id.as_deref(), Some(inner.span_id.as_str()));

        let listed = traces(Some("demo"), None);
        assert!(listed.iter().any(|t| t.trace_id == trace && t.name == "app.call" && t.spans == 2));
    }

    #[test]
    fn ingest_stamps_the_app_and_refuses_bad_ids() {
        let trace = "4bf92f3577b34da6a3ce929d0e0e4738";
        let batch: Batch = serde_json::from_value(serde_json::json!({
            "spans": [
                { "traceId": trace, "spanId": "00f067aa0ba902b8", "name": "rpc overview", "source": "host",
                  "app": "someone-else", "startMs": 1.0, "endMs": 2.0, "status": "ok" },
                { "traceId": "nope", "spanId": "00f067aa0ba902b9", "name": "x", "source": "browser",
                  "startMs": 1.0, "endMs": 2.0, "status": "ok" }
            ],
            "logs": [{ "timeMs": 1.5, "severity": "error", "body": "boom", "source": "browser", "traceId": trace }]
        }))
        .unwrap();
        assert_eq!(ingest("services", batch), 2);
        let found = query(&Query { trace_id: Some(trace.into()), ..Query::default() });
        assert_eq!(found.spans[0].app.as_deref(), Some("services"));
        assert_eq!(found.spans[0].source, "browser");
        assert_eq!(found.logs[0].app.as_deref(), Some("services"));
    }
}
