use serde_json::Value as Json;

// TODO: clean-code - 0.6 - DEAD: TracingEvent/TracingSpan are produced only by the (unwired) lua::tracing_layer and read by nothing; remove with that module or land both behind their consumer.
pub struct TracingEvent {
    pub level: String,
    pub target: String,
    pub fields: Json,
    pub ts_ms: u64,
    pub span_id: Option<u64>,
}

pub struct TracingSpan {
    pub level: String,
    pub target: String,
    pub name: String,
    pub id: u64,
    pub parent_id: Option<u64>,
    pub fields: Json,
    pub started_at_ms: u64,
    pub duration_ms: u64,
}
