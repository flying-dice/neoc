//! Per-window view instances (ui-system.md §2.5): what the host keeps for each open window, and the protocol's
//! behaviour on top of the runtime and the live sources.
//!
//! * **open**: render, subscribe the sources the render used, answer with one complete snapshot.
//! * **events**: dispatch to the app's handler, then settle: re-render every view whose state changed (keyed
//!   `diff` -> `view.ops`), deliver the app's one-shot commands, refresh live sources. All of it is sent before
//!   the event's answer.
//! * **data**: live sources push `data.patch` when a commit touches what they read, whoever wrote (a handler,
//!   an agent's tool, a migration): the open view follows with no UI code in the app.
//! * **two-way writes**: `row.set` goes through the source's `write` callback with the `baseRev` check, the
//!   text merge and the revision store ([`crate::AppService::write_field`]); the ack says what happened.
//! * **client state**: mirrored entries reach `w.client:get`; `persist = "app"` entries are stored on the host.

use crate::{
    data::Params,
    error::PortError,
    live::{LiveSink, RowsDelta, SourceId, SourceUpdate, SubscriptionId, Window},
    ports::{AppObserver, ChangeBatch, RenderOutput, SourceBinding, ViewEffect, ViewEventCall},
    services::AppService,
    writeback::{BindingKind, Resolution, WriteAck as HostAck, WriteRequest},
    AppError,
};
use agentos_domain::{
    protocol::RpcError,
    ui::{
        bindings::{bound_fields, BoundField},
        diff,
        expr::{Expr, Namespace},
        node::limits,
        protocol::{
            codes, ClientState, DataPatch, EventAck, HostNotification, RowRestore, RowSet, SourcePatch,
            SourceSet, Undo, ViewClosed, ViewCommand, ViewError as ViewErrorMsg, ViewEvent, ViewOpen,
            ViewOps, ViewSnapshot, ViewTree, WriteAck, WriteResolution,
        },
        spec::BindingKind as SpecKind,
        ClientDecl, Node, SourceDecl,
    },
    Actor, AppId,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Weak,
    },
};

/// Where a view's notifications go: the connection that opened it. Sending never blocks.
pub trait ViewSink: Send + Sync {
    fn send(&self, notification: HostNotification);
}

/// Why a view request failed, with the JSON-RPC code it maps to.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    #[error("no such view (closed, or the host restarted): open it again")]
    NoSuchView,
    /// An app handler failed; the message is safe to show.
    #[error("{0}")]
    App(String),
    #[error("{0}")]
    InvalidTree(String),
    #[error("{0}")]
    Invalid(String),
}

impl ViewError {
    #[must_use]
    pub fn code(&self) -> i32 {
        match self {
            Self::NoSuchView => codes::NO_SUCH_VIEW,
            Self::App(_) => codes::APP_ERROR,
            Self::InvalidTree(_) => codes::INVALID_TREE,
            Self::Invalid(_) => codes::INVALID_PARAMS,
        }
    }

    #[must_use]
    pub fn to_rpc(&self) -> RpcError {
        RpcError { code: self.code(), message: self.to_string(), data: None }
    }

    fn from_port(e: PortError) -> Self {
        match e {
            PortError::Rejected(m) if m.contains("the rendered tree is invalid") => Self::InvalidTree(m),
            PortError::Rejected(m) | PortError::Internal(m) => Self::App(m),
            PortError::NotFound(m) => Self::Invalid(format!("not found: {m}")),
        }
    }

    fn from_app(e: AppError) -> Self {
        match e {
            AppError::Port(p) => Self::from_port(p),
            other => Self::Invalid(other.to_string()),
        }
    }
}

/// How many values of a bound field the host remembers per row: the base of a later merge.
const RING: usize = 8;

fn wire_key(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

fn to_u32(v: Option<&Value>) -> u32 {
    v.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

#[derive(Default)]
struct State {
    rev: u32,
    tree: Option<Node>,
    /// The newest render applied (renders can finish out of order).
    render_seq: u64,
    decls: BTreeMap<String, ClientDecl>,
    sources: BTreeMap<String, SourceBinding>,
    subs: BTreeMap<String, SubscriptionId>,
    client: Map<String, Value>,
    bound: Vec<BoundField>,
    /// source id -> row key -> the row the client has.
    rows: BTreeMap<String, BTreeMap<String, Value>>,
    /// `source|key|field` -> (rev, value) of what was sent, newest last.
    ring: HashMap<String, VecDeque<(u32, Value)>>,
    last_seq: u32,
}

struct View {
    id: String,
    app: AppId,
    window: String,
    conn: u64,
    sink: Arc<dyn ViewSink>,
    st: Mutex<State>,
}

impl View {
    fn send(&self, n: HostNotification) {
        self.sink.send(n);
    }
}

/// Receives the live updates of one source of one view.
struct SourceSink {
    view: Weak<View>,
    source: String,
}

#[async_trait]
impl LiveSink for SourceSink {
    async fn deliver(&self, _: SubscriptionId, update: SourceUpdate) {
        let Some(view) = self.view.upgrade() else { return };
        let patch = {
            let mut st = view.st.lock();
            patch_of(&mut st, &self.source, &update)
        };
        match (patch, update) {
            (Some(patch), _) => {
                let patches = BTreeMap::from([(self.source.clone(), patch)]);
                view.send(HostNotification::Data(DataPatch {
                    view_id: view.id.clone(),
                    state: None,
                    sources: Some(patches),
                    declare: None,
                }));
            }
            (None, SourceUpdate::Failed(message)) => view.send(HostNotification::Error(ViewErrorMsg {
                view_id: view.id.clone(),
                message: format!("a live source failed: {message}"),
                dev: None,
            })),
            (None, _) => {}
        }
    }
}

/// Which sources a render started and stopped using. Queries are remembered by their subscription; thread
/// sources are declared but never subscribed, so `st.sources` is where they are remembered.
fn source_changes(st: &State, used: &BTreeMap<String, SourceBinding>) -> (Vec<String>, Vec<String>) {
    let added = used
        .iter()
        .filter(|(k, b)| {
            let known = if is_thread(b) { st.sources.contains_key(*k) } else { st.subs.contains_key(*k) };
            !known
        })
        .map(|(k, _)| k.clone())
        .collect();
    let removed = st
        .subs
        .keys()
        .chain(st.sources.iter().filter(|(_, b)| is_thread(b)).map(|(k, _)| k))
        .filter(|k| !used.contains_key(*k))
        .cloned()
        .collect();
    (added, removed)
}

/// A `thread` source (`ChatTranscript`): declared to the client, never subscribed or queried.
fn is_thread(binding: &SourceBinding) -> bool {
    matches!(binding.decl, SourceDecl::Thread { .. })
}

/// The wire patch of an update, remembering what the client now has (rows, and the values a later merge
/// needs as its base).
fn patch_of(st: &mut State, source: &str, update: &SourceUpdate) -> Option<SourcePatch> {
    match update {
        SourceUpdate::Rows(d) => Some(rows_patch(st, source, d)),
        SourceUpdate::Row(d) => {
            let rev = to_u32(d.rev.as_ref());
            let key_col = match st.sources.get(source).map(|b| &b.decl) {
                Some(SourceDecl::Row { key, .. }) => Some(key.clone()),
                _ => None,
            };
            if let (Some(row), Some(key_col)) = (&d.row, key_col) {
                let key = row.get(&key_col).map(wire_key).unwrap_or_default();
                let fields: Vec<String> =
                    st.bound.iter().filter(|b| b.source == source).map(|b| b.field.clone()).collect();
                for field in fields {
                    if let Some(value) = row.get(&field) {
                        let ring = st.ring.entry(format!("{source}|{key}|{field}")).or_default();
                        ring.push_back((rev, value.clone()));
                        while ring.len() > RING {
                            ring.pop_front();
                        }
                    }
                }
            }
            Some(SourcePatch::Row { rev, row: d.row.clone() })
        }
        SourceUpdate::Scalar(v) => Some(SourcePatch::Value { value: v.clone() }),
        SourceUpdate::Failed(_) => None,
    }
}

fn rows_patch(st: &mut State, source: &str, d: &RowsDelta) -> SourcePatch {
    let cache = st.rows.entry(source.to_owned()).or_default();
    for key in &d.dropped {
        cache.remove(&wire_key(key));
    }
    let mut rows = BTreeMap::new();
    for (key, row) in &d.changed {
        cache.insert(wire_key(key), row.clone());
        rows.insert(wire_key(key), row.clone());
    }
    let window: Vec<String> = d.keys.iter().map(wire_key).collect();
    cache.retain(|k, _| window.contains(k));
    SourcePatch::Rows {
        total: u32::try_from(d.total.unwrap_or(d.from + d.keys.len() as u64)).unwrap_or(u32::MAX),
        from: u32::try_from(d.from).unwrap_or(0),
        keys: d.keys.clone(),
        rows,
        drop: d.dropped.clone(),
    }
}

/// A source param as the host can resolve it: client refs read the client's values, literals pass through,
/// anything else (a ref only the client can evaluate) is null.
fn resolve_param(value: &Value, client: &Map<String, Value>) -> Value {
    match Expr::parse(value) {
        Ok(Expr::Ref(r)) if r.namespace == Namespace::Client => {
            let mut parts = r.rest.split('.');
            let mut current = parts.next().and_then(|name| client.get(name)).cloned().unwrap_or(Value::Null);
            for part in parts {
                current = current.get(part).cloned().unwrap_or(Value::Null);
            }
            current
        }
        Ok(Expr::Lit(v)) => v,
        _ => Value::Null,
    }
}

fn resolve_params(decl: &SourceDecl, client: &Map<String, Value>) -> Params {
    Params::Named(decl.params().iter().map(|(k, v)| (k.clone(), resolve_param(v, client))).collect())
}

fn window_of(decl: &SourceDecl) -> Option<Window> {
    match decl {
        SourceDecl::Rows { page: Some(p), .. } => Some(Window { from: 0, to: u64::from(*p) }),
        _ => None,
    }
}

/// The view instances of every open window.
pub struct ViewService {
    app: Arc<AppService>,
    views: Mutex<HashMap<String, Arc<View>>>,
    next: AtomicU64,
    /// One settle at a time per app: `dirty_views` hands the dirty set to whoever asks first, so a settle that
    /// a background change started must finish (its ops sent) before an event's own settle can return and
    /// the event be acknowledged.
    /// An entry is made by the first settle of an app with open views and dropped when the app stops.
    settle_turns: Mutex<HashMap<AppId, Arc<futures_util::lock::Mutex<()>>>>,
    /// What apps see as the name of the person (`ctx.actor.name`).
    user: String,
}

impl ViewService {
    #[must_use]
    pub fn new(app: Arc<AppService>) -> Self {
        Self {
            app,
            views: Mutex::default(),
            next: AtomicU64::new(1),
            settle_turns: Mutex::default(),
            user: "You".to_owned(),
        }
    }

    fn get(&self, conn: u64, id: &str) -> Result<Arc<View>, ViewError> {
        self.views.lock().get(id).filter(|v| v.conn == conn).cloned().ok_or(ViewError::NoSuchView)
    }

    fn actor(&self) -> Actor {
        Actor::user(&self.user)
    }

    /// How many windows are open (tests, diagnostics).
    #[must_use]
    pub fn open_views(&self) -> usize {
        self.views.lock().len()
    }

    // ---- open / sync ---------------------------------------------------------------------------------------

    pub async fn open(
        &self,
        conn: u64,
        sink: Arc<dyn ViewSink>,
        req: ViewOpen,
    ) -> Result<ViewSnapshot, ViewError> {
        let app = AppId::parse(&req.app).map_err(|e| ViewError::Invalid(e.to_string()))?;
        let data = self.app.data_of(&app).map_err(ViewError::from_app)?;
        let runtime = self.app.runtime();

        // what the host stored for this window kind, then what the client persisted across a host restart
        let mut restored = data.ui_state(&req.window).await.map_err(ViewError::from_port)?;
        if let Some(resume) = &req.resume {
            restored.extend(resume.client.clone());
        }
        let id = format!("v{}", self.next.fetch_add(1, Ordering::Relaxed));
        let out = runtime
            .open_view(&app, &id, &req.window, req.params.clone().unwrap_or_default(), restored)
            .await
            .map_err(ViewError::from_port)?;
        if out.sources.len() > agentos_domain::ui::source::budgets::MAX_SOURCES_PER_VIEW {
            let _ = runtime.close_view(&app, &id).await;
            return Err(ViewError::InvalidTree(format!(
                "a view may use at most {} sources, this one uses {}",
                agentos_domain::ui::source::budgets::MAX_SOURCES_PER_VIEW,
                out.sources.len()
            )));
        }

        let view = Arc::new(View {
            id: id.clone(),
            app: app.clone(),
            window: req.window.clone(),
            conn,
            sink,
            st: Mutex::new(State::default()),
        });
        let snapshot = match self.activate(&view, out).await {
            Ok(s) => s,
            Err(e) => {
                let _ = runtime.close_view(&app, &id).await;
                return Err(e);
            }
        };
        self.views.lock().insert(id.clone(), view.clone());

        // drafts the client never saw acked: replay them with the `baseRev` they were written on
        if let Some(resume) = req.resume {
            for draft in resume.drafts {
                if let agentos_domain::ui::protocol::Draft::Row { source, key, field, value, base_rev } =
                    draft
                {
                    let write = RowSet { view_id: id.clone(), source, key, field, value, base_rev };
                    if let Err(e) = self.row_set(conn, write).await {
                        tracing::debug!(view = %id, error = %e, "a replayed draft was not applied");
                    }
                }
            }
        }
        Ok(snapshot)
    }

    /// Subscribe the render's sources and build the snapshot of the view.
    async fn activate(&self, view: &Arc<View>, out: RenderOutput) -> Result<ViewSnapshot, ViewError> {
        let live = self.app.live(&view.app).map_err(ViewError::from_app)?;
        let mut client: Map<String, Value> =
            out.client_decls.iter().map(|(n, d)| (n.clone(), d.init.clone())).collect();
        client.extend(out.client.clone());

        {
            let mut st = view.st.lock();
            st.bound = bound_fields(&out.tree);
            st.sources = out.sources.clone();
            client.clone_into(&mut st.client);
            st.decls = out.client_decls.clone();
        }
        let mut data = BTreeMap::new();
        let mut subs = BTreeMap::new();
        for (sid, binding) in &out.sources {
            if is_thread(binding) {
                continue; // its data is `/agui`, not a query
            }
            let sink = Arc::new(SourceSink { view: Arc::downgrade(view), source: sid.clone() });
            let params = resolve_params(&binding.decl, &client);
            match live.subscribe(&binding.def, params, window_of(&binding.decl), sink).await {
                Ok((sub, first)) => {
                    subs.insert(sid.clone(), sub);
                    if let Some(patch) = patch_of(&mut view.st.lock(), sid, &first) {
                        data.insert(sid.clone(), patch);
                    }
                }
                Err(e) => {
                    for sub in subs.values() {
                        live.unsubscribe(*sub);
                    }
                    return Err(ViewError::from_port(e));
                }
            }
        }
        let snapshot = {
            let mut st = view.st.lock();
            st.subs = subs;
            st.rev += 1;
            st.render_seq = out.seq;
            st.tree = Some(out.tree.clone());
            ViewSnapshot {
                view_id: view.id.clone(),
                proto: agentos_domain::ui::PROTO_VERSION,
                kit: agentos_domain::ui::KIT_VERSION.to_string(),
                rev: st.rev,
                client_decls: out.client_decls,
                client: out.client,
                state: BTreeMap::new(),
                sources: out.sources.iter().map(|(k, b)| (k.clone(), b.decl.clone())).collect(),
                tree: out.tree,
                data,
            }
        };
        Ok(snapshot)
    }

    /// `view.sync`: a full snapshot of what the view is now, replacing whatever the client has.
    pub async fn sync(&self, conn: u64, view_id: &str) -> Result<ViewSnapshot, ViewError> {
        let view = self.get(conn, view_id)?;
        let runtime = self.app.runtime();
        let live = self.app.live(&view.app).map_err(ViewError::from_app)?;
        let out = runtime.render_view(&view.app, &view.id).await.map_err(ViewError::from_port)?;
        for sub in std::mem::take(&mut view.st.lock().subs).into_values() {
            live.unsubscribe(sub);
        }
        {
            let mut st = view.st.lock();
            st.rows.clear();
            st.ring.clear();
        }
        let mut out = out;
        out.client = view.st.lock().client.clone();
        self.activate(&view, out).await
    }

    // ---- events ----------------------------------------------------------------------------------------------

    pub async fn event(&self, conn: u64, ev: ViewEvent) -> Result<EventAck, ViewError> {
        let view = self.get(conn, &ev.view_id)?;
        let row = {
            let mut st = view.st.lock();
            if st.last_seq != 0 && ev.seq <= st.last_seq {
                return Ok(EventAck { ok: true, reason: None }); // a replay after a reconnect
            }
            st.last_seq = ev.seq;
            ev.row_key.as_ref().and_then(|k| {
                let key = wire_key(k);
                st.rows.values().find_map(|rows| rows.get(&key).cloned())
            })
        };
        let call = ViewEventCall {
            path: ev.path.clone(),
            event: ev.event.clone(),
            row_key: ev.row_key.clone(),
            row,
            payload: ev.payload.clone(),
        };
        let ctx = crate::ports::CallContext { actor: self.actor(), now_ms: self.app.now_ms() };
        let handled = self.app.runtime().dispatch_view_event(&view.app, &view.id, call, ctx).await;
        // whatever the handler did (even half of it), the views follow
        self.settle(&view.app).await;
        match handled {
            Ok(true) => Ok(EventAck { ok: true, reason: None }),
            Ok(false) => Ok(EventAck { ok: false, reason: Some("stale".into()) }),
            Err(e) => Err(ViewError::from_port(e)),
        }
    }

    // ---- two-way writes --------------------------------------------------------------------------------------

    fn ack_of(ack: &HostAck) -> WriteAck {
        let resolution = match ack.resolution {
            Resolution::Accepted => WriteResolution::Accepted,
            Resolution::Merged => WriteResolution::Merged,
            Resolution::Replaced => WriteResolution::Replaced,
            Resolution::Rejected => WriteResolution::Rejected,
        };
        WriteAck {
            rev: to_u32(ack.rev.as_ref()),
            value: ack.value.clone(),
            conflict: Some(matches!(ack.resolution, Resolution::Replaced | Resolution::Rejected)),
            undo: ack
                .revision_id
                .zip(ack.lost.clone())
                .map(|(id, lost)| Undo { revision_id: u32::try_from(id).unwrap_or(u32::MAX), lost }),
            resolution,
        }
    }

    /// The row a subscription selects: its definition and the params it currently runs with.
    fn row_target(&self, view: &View, source: &str) -> Result<(SourceId, Params), ViewError> {
        let live = self.app.live(&view.app).map_err(ViewError::from_app)?;
        let st = view.st.lock();
        let binding =
            st.sources.get(source).ok_or_else(|| ViewError::Invalid(format!("no source `{source}`")))?;
        if !matches!(binding.decl, SourceDecl::Row { .. }) {
            return Err(ViewError::Invalid(format!("source `{source}` is not a single-row source")));
        }
        let sub = st.subs.get(source).ok_or(ViewError::NoSuchView)?;
        let params = live.params_of(*sub).ok_or(ViewError::NoSuchView)?;
        Ok((binding.def.clone(), params))
    }

    pub async fn row_set(&self, conn: u64, req: RowSet) -> Result<WriteAck, ViewError> {
        let view = self.get(conn, &req.view_id)?;
        let (def, params) = self.row_target(&view, &req.source)?;
        let binding = {
            let st = view.st.lock();
            let kind =
                st.bound.iter().find(|b| b.source == req.source && b.field == req.field).map(|b| b.kind);
            match kind {
                Some(SpecKind::Text) => {
                    let ring_key = format!("{}|{}|{}", req.source, wire_key(&req.key), req.field);
                    let base = st
                        .ring
                        .get(&ring_key)
                        .and_then(|r| r.iter().rev().find(|(rev, _)| *rev == req.base_rev))
                        .and_then(|(_, v)| v.as_str().map(str::to_owned));
                    BindingKind::Text { base }
                }
                _ => BindingKind::Scalar,
            }
        };
        let write = WriteRequest {
            source: def,
            params,
            key: req.key,
            field: req.field,
            value: req.value,
            base_rev: Some(json!(req.base_rev)),
            binding,
            actor: self.actor(),
        };
        let ack = self.app.write_field(&view.app, write).await.map_err(ViewError::from_app);
        self.settle(&view.app).await;
        ack.map(|a| Self::ack_of(&a))
    }

    /// `row.restore`: Undo.
    pub async fn row_restore(&self, conn: u64, req: RowRestore) -> Result<WriteAck, ViewError> {
        let view = self.get(conn, &req.view_id)?;
        let (_, params) = self.row_target(&view, &req.source)?;
        let ack = self
            .app
            .restore_revision(&view.app, i64::from(req.revision_id), params, self.actor())
            .await
            .map_err(ViewError::from_app);
        self.settle(&view.app).await;
        ack.map(|a| Self::ack_of(&a))
    }

    // ---- sources, client state, close ----------------------------------------------------------------------

    pub async fn source_set(&self, conn: u64, req: SourceSet) -> Result<SourcePatch, ViewError> {
        let view = self.get(conn, &req.view_id)?;
        let thread = view.st.lock().sources.get(&req.source).is_some_and(is_thread);
        if thread {
            // nothing to query: the thread's data is `/agui`. The client re-asks whenever a param ref changes
            // (the selected thread); an empty value is the quiet answer (a thread patch would be reported as
            // unsupported by the current client core).
            return Ok(SourcePatch::Value { value: Value::Null });
        }
        let live = self.app.live(&view.app).map_err(ViewError::from_app)?;
        let sub = *view
            .st
            .lock()
            .subs
            .get(&req.source)
            .ok_or_else(|| ViewError::Invalid(format!("no source `{}`", req.source)))?;
        let params = req.params.map(|m| Params::Named(m.into_iter().collect()));
        let window = req.range.map(|r| Window { from: u64::from(r.from), to: u64::from(r.to) });
        let update = live.update_always(sub, params, window).await.map_err(ViewError::from_port)?;
        let patch = patch_of(&mut view.st.lock(), &req.source, &update);
        patch.ok_or_else(|| ViewError::App("the source could not be read".into()))
    }

    pub async fn client_state(&self, conn: u64, n: ClientState) {
        let Ok(view) = self.get(conn, &n.view_id) else { return };
        let persist: Map<String, Value> = {
            let mut st = view.st.lock();
            st.client.extend(n.set.clone());
            n.set
                .iter()
                .filter(|(name, _)| {
                    st.decls.get(*name).is_some_and(|d| d.persist == agentos_domain::ui::source::Persist::App)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        if let Err(e) = self.app.runtime().set_client_values(&view.app, &view.id, n.set).await {
            tracing::debug!(view = %view.id, error = %e, "mirroring client state failed");
        }
        if !persist.is_empty() {
            match self.app.data_of(&view.app) {
                Ok(data) => {
                    if let Err(e) = data.set_ui_state(&view.window, persist).await {
                        tracing::warn!(view = %view.id, error = %e, "saving window state failed");
                    }
                }
                Err(e) => tracing::debug!(error = %e, "no database to save window state in"),
            }
        }
    }

    pub async fn close(&self, conn: u64, view_id: &str) -> Result<(), ViewError> {
        let view = self.get(conn, view_id)?;
        self.drop_view(&view, None).await;
        Ok(())
    }

    /// The socket is gone: its windows go with it (a reconnect opens fresh instances that adopt the client's
    /// persisted values).
    pub async fn connection_closed(&self, conn: u64) {
        let doomed: Vec<Arc<View>> = self.views.lock().values().filter(|v| v.conn == conn).cloned().collect();
        for view in doomed {
            self.drop_view(&view, None).await;
        }
    }

    async fn drop_view(&self, view: &Arc<View>, closed: Option<&str>) {
        self.views.lock().remove(&view.id);
        if let Ok(live) = self.app.live(&view.app) {
            for sub in std::mem::take(&mut view.st.lock().subs).into_values() {
                live.unsubscribe(sub);
            }
        }
        if let Err(e) = self.app.runtime().close_view(&view.app, &view.id).await {
            tracing::debug!(view = %view.id, error = %e, "closing the instance failed");
        }
        if let Some(reason) = closed {
            view.send(HostNotification::Closed(ViewClosed {
                view_id: view.id.clone(),
                reason: reason.to_owned(),
            }));
        }
    }

    // ---- following the app -----------------------------------------------------------------------------------

    fn views_of(&self, app: &AppId) -> Vec<Arc<View>> {
        let mut v: Vec<Arc<View>> = self.views.lock().values().filter(|v| &v.app == app).cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// After anything that may have changed state or data: re-render the views that read what changed,
    /// deliver what windows asked for, and refresh the live sources.
    pub async fn settle(&self, app: &AppId) {
        self.settle_with(app, true).await;
    }

    /// [`ViewService::settle`], with or without re-querying the live sources. The app's own background work does
    /// without: what it wrote to the database is pushed by the sources' change feed, and a poller would
    /// otherwise re-run every subscribed query many times a second while events wait behind it.
    async fn settle_with(&self, app: &AppId, refresh_sources: bool) {
        if self.views_of(app).is_empty() {
            return;
        }
        let turn = self.settle_turns.lock().entry(app.clone()).or_default().clone();
        let _one_at_a_time = turn.lock().await;
        let runtime = self.app.runtime();
        if let Ok(dirty) = runtime.dirty_views(app).await {
            for id in dirty {
                let view = self.views.lock().get(&id).cloned();
                if let Some(view) = view {
                    self.rerender(&view).await;
                }
            }
        }
        if let Ok(effects) = runtime.take_view_effects(app).await {
            for (id, effect) in effects {
                let view = self.views.lock().get(&id).cloned();
                if let Some(view) = view {
                    let persisted = Self::deliver_effect(&view, effect);
                    if let Some((name, value)) = persisted {
                        self.persist_client_value(&view, name, value).await;
                    }
                }
            }
        }
        if !refresh_sources {
            return;
        }
        if let Ok(live) = self.app.live(app) {
            live.refresh(&ChangeBatch::everything()).await;
        }
    }

    /// Send what a window asked for to its client. A `w.client:set` of a `persist = "app"` value is also
    /// returned, to be stored: the client only mirrors values the person changed, so a value the app chose
    /// (the chat it just opened) would otherwise be forgotten when the window closes.
    fn deliver_effect(view: &View, effect: ViewEffect) -> Option<(String, Value)> {
        match effect {
            ViewEffect::Command(command) => {
                view.send(HostNotification::Command(ViewCommand { view_id: view.id.clone(), command }));
                None
            }
            ViewEffect::ClientSet { path, value, force } => {
                let name = path.split('.').next().unwrap_or(&path).to_owned();
                let persisted = {
                    let mut st = view.st.lock();
                    st.client.insert(name.clone(), value.clone());
                    let app_scoped = path == name
                        && st
                            .decls
                            .get(&name)
                            .is_some_and(|d| d.persist == agentos_domain::ui::source::Persist::App);
                    app_scoped.then(|| (name, value.clone()))
                };
                view.send(HostNotification::ClientSet(agentos_domain::ui::protocol::ClientSet {
                    view_id: view.id.clone(),
                    path,
                    value,
                    force: force.then_some(true),
                }));
                persisted
            }
        }
    }

    async fn persist_client_value(&self, view: &View, name: String, value: Value) {
        match self.app.data_of(&view.app) {
            Ok(data) => {
                let one = Map::from_iter([(name, value)]);
                if let Err(e) = data.set_ui_state(&view.window, one).await {
                    tracing::warn!(view = %view.id, error = %e, "saving window state failed");
                }
            }
            Err(e) => tracing::debug!(error = %e, "no database to save window state in"),
        }
    }

    /// Render, diff against what the client has, send the ops. A render that fails keeps the last good tree.
    async fn rerender(&self, view: &Arc<View>) {
        let runtime = self.app.runtime();
        let out = match runtime.render_view(&view.app, &view.id).await {
            Ok(out) => out,
            Err(e) => {
                view.send(HostNotification::Error(ViewErrorMsg {
                    view_id: view.id.clone(),
                    message: e.to_string(),
                    dev: None,
                }));
                return;
            }
        };
        if out.seq <= view.st.lock().render_seq {
            return; // an older render finishing late
        }

        // sources a re-render started to use are declared and filled before the ops that reference them
        let live = self.app.live(&view.app).ok();
        let (added, removed, client) = {
            let st = view.st.lock();
            let (added, removed) = source_changes(&st, &out.sources);
            (added, removed, st.client.clone())
        };
        let mut declare = BTreeMap::new();
        let mut data = BTreeMap::new();
        if let Some(live) = &live {
            for sid in &added {
                let binding = &out.sources[sid];
                if is_thread(binding) {
                    view.st.lock().sources.insert(sid.clone(), binding.clone());
                    declare.insert(sid.clone(), binding.decl.clone());
                    continue;
                }
                let sink = Arc::new(SourceSink { view: Arc::downgrade(view), source: sid.clone() });
                let params = resolve_params(&binding.decl, &client);
                if let Ok((sub, first)) =
                    live.subscribe(&binding.def, params, window_of(&binding.decl), sink).await
                {
                    let mut st = view.st.lock();
                    st.sources.insert(sid.clone(), binding.clone());
                    st.subs.insert(sid.clone(), sub);
                    declare.insert(sid.clone(), binding.decl.clone());
                    if let Some(patch) = patch_of(&mut st, sid, &first) {
                        data.insert(sid.clone(), patch);
                    }
                }
            }
            for sid in &removed {
                if let Some(sub) = view.st.lock().subs.remove(sid) {
                    live.unsubscribe(sub);
                }
            }
        }

        let mut st = view.st.lock();
        if out.seq <= st.render_seq {
            return;
        }
        st.render_seq = out.seq;
        st.bound = bound_fields(&out.tree);
        st.decls = out.client_decls.clone();
        for sid in &removed {
            st.sources.remove(sid);
            st.rows.remove(sid);
        }
        // a source whose params changed keeps its subscription: the client re-asks with `source.set`
        let Some(old) = st.tree.clone() else { return };
        let ops = diff(&old, &out.tree);
        st.tree = Some(out.tree.clone());
        if !declare.is_empty() {
            view.send(HostNotification::Data(DataPatch {
                view_id: view.id.clone(),
                state: None,
                sources: Some(data),
                declare: Some(declare),
            }));
        }
        if ops.is_empty() {
            return;
        }
        let base = st.rev;
        st.rev += 1;
        if ops.len() > limits::MAX_OPS_PER_BATCH {
            view.send(HostNotification::Tree(ViewTree {
                view_id: view.id.clone(),
                rev: st.rev,
                tree: out.tree,
            }));
        } else {
            view.send(HostNotification::Ops(ViewOps { view_id: view.id.clone(), base, rev: st.rev, ops }));
        }
    }
}

#[async_trait]
impl AppObserver for ViewService {
    async fn mutated(&self, app: &AppId) {
        self.settle(app).await;
    }

    async fn background_changed(&self, app: &AppId) {
        self.settle_with(app, false).await;
    }

    async fn stopping(&self, app: &AppId, reason: &str) {
        for view in self.views_of(app) {
            self.drop_view(&view, Some(reason)).await;
        }
        self.settle_turns.lock().remove(app);
    }
}
