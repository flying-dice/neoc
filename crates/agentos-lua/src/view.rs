//! Windows: one view instance per open window, rendered from Lua into the wire tree.
//!
//! `app.window(name, { client = ..., render = function(w) ... end })` declares a window kind. Opening one
//! creates an instance with its own `w` (per-window state, client-state Refs, env Refs, one-shot commands);
//! `render(w)` runs as a pure, tracked scope (state it reads becomes the instance's dependencies); the Lua node
//! tree is converted to the wire form (closures stay here, addressed by `path#event`), validated against the
//! component schema, and handed to the view service, which diffs it against what the client has.

use crate::{
    convert::{json_to_lua, lua_to_json, lua_to_json_in, Walk},
    error::ScriptError,
    state::{make_state, DepKey},
    vm::{AppVm, Result, Shared, VmError},
};
use agentos_application::{
    live::{Conflict, SourceDef, SourceId, SourceKind, TotalMode},
    ports::{CallContext, RenderOutput, SourceBinding, ViewEffect, ViewEventCall},
};
use agentos_domain::ui::{
    bindings::{bound_fields, writable_by_source},
    node::limits,
    protocol::{Command, Toast, ToastAction},
    source::{ConflictPolicy, SourceDecl, TotalMode as WireTotal, ViewDecls},
    validate::{validate_with, Context, Severity},
    Action, ClientDecl, Handler, Node,
};
use agentos_luavm::mlua;
use agentos_luavm::mlua::{Function, Lua, Table, Value as LuaValue};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

/// One open window.
pub(crate) struct ViewInstance {
    pub window: String,
    pub spec: Table,
    pub w: Table,
    /// The client values `w.client:get(name)` reads: declared inits, restored values, the client's mirror.
    pub mirror: Table,
    pub decls: BTreeMap<String, ClientDecl>,
    /// `path#event` -> the closures of the last accepted render.
    pub handlers: HashMap<String, Vec<Function>>,
    /// The `onTap` closures of `w:toast` action buttons by toast id (`$toast/<id>`), each run once.
    pub toast_actions: Vec<(String, Function)>,
    pub toast_seq: u32,
    pub deps: BTreeSet<DepKey>,
    pub render_seq: u64,
    /// Values `w.client:set` wrote while the window was opening (they go into the first snapshot).
    pub opening: Option<Map<String, Value>>,
}

/// The source handles a render made, so each (declaration, params) pair gets one stable id.
#[derive(Default)]
pub(crate) struct RenderScope {
    handles: Vec<(String, String, Value)>,
}

impl RenderScope {
    /// `s2` for the first use of declaration `s2`, `s2_2` for the next distinct params, and so on.
    fn id_for(&mut self, def: &str, params: &Value) -> String {
        if let Some((id, ..)) = self.handles.iter().find(|(_, d, p)| d == def && p == params) {
            return id.clone();
        }
        let n = self.handles.iter().filter(|(_, d, _)| d == def).count();
        let id = if n == 0 { def.to_owned() } else { format!("{def}_{}", n + 1) };
        self.handles.push((id.clone(), def.to_owned(), params.clone()));
        id
    }
}

fn dollar(key: &LuaValue) -> bool {
    matches!(key, LuaValue::String(s) if s.as_bytes().first() == Some(&b'$'))
}

/// A table that answers every field with a ref (`w.client.search` -> `{"$ref": "client.search"}`) and leaves
/// `$`-prefixed keys alone.
fn ref_proxy(lua: &Lua, own: Table, prefix: String) -> mlua::Result<Table> {
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(move |lua, (_t, key): (LuaValue, LuaValue)| -> mlua::Result<LuaValue> {
            if dollar(&key) {
                return Ok(LuaValue::Nil);
            }
            let LuaValue::String(name) = key else { return Ok(LuaValue::Nil) };
            let r = lua.create_table()?;
            r.set("$ref", format!("{prefix}{}", name.to_string_lossy()))?;
            Ok(LuaValue::Table(r))
        })?,
    )?;
    own.set_metatable(Some(meta))?;
    Ok(own)
}

/// A source handle for a render: `{["$source"] = id}`, `:bind(field)` and every other field a ref.
pub(crate) fn make_handle(lua: &Lua, sh: &Shared, def: &str, params: &Table) -> mlua::Result<Table> {
    let params_json = match lua_to_json(&LuaValue::Table(params.clone()))? {
        Value::Array(a) if a.is_empty() => Value::Object(Map::new()),
        other => other,
    };
    let id = match sh.render_scope.lock().as_mut() {
        Some(scope) => scope.id_for(def, &params_json),
        None => def.to_owned(),
    };
    let handle = lua.create_table()?;
    handle.set("$source", id.as_str())?;
    let bind_id = id.clone();
    handle.set(
        "bind",
        lua.create_function(move |lua, (_this, field): (LuaValue, String)| {
            let r = lua.create_table()?;
            r.set("$ref", format!("src.{bind_id}.{field}"))?;
            Ok(r)
        })?,
    )?;
    ref_proxy(lua, handle, format!("src.{id}."))
}

// ---- Lua tree -> wire tree --------------------------------------------------------------------------------

fn bad(path: &str, why: impl std::fmt::Display) -> VmError {
    VmError::InvalidWindow(format!("{path}: {why}"))
}

/// What converting one rendered tree has used, against what a view may hold. The domain validates the
/// finished tree, but a Lua tree is untrusted until it is converted: one that contains itself, or is
/// shared at every level, would overflow the stack or the memory before validation ever saw it.
struct Wire {
    /// The closures of the handlers met so far, by `path#event`.
    fns: HashMap<String, Vec<Function>>,
    /// Nodes converted, against `limits::MAX_NODES`.
    nodes: usize,
    /// Every prop and action value, against a budget for the whole tree; also the tables the walk is inside
    /// of, so a cycle is found.
    walk: Walk,
}

impl Wire {
    fn new() -> Self {
        // a tree larger than one wire message can't be sent anyway
        Self {
            fns: HashMap::new(),
            nodes: 0,
            walk: Walk::with_limits(limits::MAX_NODES * 10, limits::MAX_MESSAGE_BYTES),
        }
    }
}

fn wire_handler(value: &LuaValue, path: &str, key: &str, wire: &mut Wire) -> Result<Handler> {
    let slot = format!("{path}#{key}");
    match value {
        LuaValue::Boolean(true) => {
            wire.fns.insert(slot, vec![]);
            Ok(Handler::Emit)
        }
        LuaValue::Function(f) => {
            wire.fns.insert(slot, vec![f.clone()]);
            Ok(Handler::Emit)
        }
        LuaValue::Table(list) => {
            let mut actions = Vec::new();
            let mut closures = Vec::new();
            for item in list.sequence_values::<LuaValue>() {
                match item.map_err(|e| bad(path, e))? {
                    LuaValue::Function(f) => closures.push(f),
                    LuaValue::Table(a) => {
                        let json = lua_to_json_in(&LuaValue::Table(a), path, &mut wire.walk)
                            .map_err(|e| bad(path, e))?;
                        actions.push(
                            serde_json::from_value::<Action>(json)
                                .map_err(|e| bad(path, format!("on.{key}: {e}")))?,
                        );
                    }
                    other => {
                        return Err(bad(
                            path,
                            format!(
                                "on.{key}: a handler holds functions and actions, got {}",
                                other.type_name()
                            ),
                        ))
                    }
                }
            }
            let handler = if actions.is_empty() {
                Handler::Emit
            } else {
                if !closures.is_empty() {
                    actions.push(Action::Emit { event: None, payload: None });
                }
                Handler::Actions(actions)
            };
            wire.fns.insert(slot, closures);
            Ok(handler)
        }
        other => Err(bad(
            path,
            format!("on.{key}: a handler is true, a function or a list, got {}", other.type_name()),
        )),
    }
}

fn wire_list(
    list: &Table,
    path: &str,
    slot: Option<&str>,
    wire: &mut Wire,
    depth: usize,
) -> Result<Vec<Node>> {
    let mut out = Vec::new();
    for child in list.sequence_values::<Table>() {
        let child = child.map_err(|e| bad(path, e))?;
        let key: String = child.get("key").map_err(|_| bad(path, "a child node has no key"))?;
        let child_path = agentos_domain::ui::node::join_path(path, slot, &key);
        out.push(wire_node(&child, &child_path, wire, depth + 1)?);
    }
    Ok(out)
}

fn wire_node(t: &Table, path: &str, wire: &mut Wire, depth: usize) -> Result<Node> {
    wire.nodes += 1;
    if wire.nodes > limits::MAX_NODES {
        return Err(bad(path, format!("a view holds at most {} nodes", limits::MAX_NODES)));
    }
    if depth > limits::MAX_DEPTH {
        return Err(bad(path, format!("trees are at most {} levels deep", limits::MAX_DEPTH)));
    }
    if !wire.walk.enter(t) {
        return Err(bad(path, "a node that contains itself"));
    }
    let node = wire_node_inside(t, path, wire, depth);
    wire.walk.leave(t);
    node
}

fn wire_node_inside(t: &Table, path: &str, wire: &mut Wire, depth: usize) -> Result<Node> {
    let type_: String = t.get("type").map_err(|_| bad(path, "a node has no type"))?;
    let key: String = t.get("key").map_err(|_| bad(path, "a node has no key"))?;
    let mut node = Node::new(type_, key);
    if let Some(props) = t.get::<Option<Table>>("props").map_err(|e| bad(path, e))? {
        for pair in props.pairs::<String, LuaValue>() {
            let (name, value) = pair.map_err(|e| bad(path, e))?;
            let json = lua_to_json_in(&value, "value", &mut wire.walk)
                .map_err(|e| bad(path, format!("props.{name}: {e}")))?;
            node.props.insert(name, json);
        }
    }
    if let Some(children) = t.get::<Option<Table>>("children").map_err(|e| bad(path, e))? {
        node.children = wire_list(&children, path, None, wire, depth)?;
    }
    if let Some(slots) = t.get::<Option<Table>>("slots").map_err(|e| bad(path, e))? {
        for pair in slots.pairs::<String, Table>() {
            let (name, list) = pair.map_err(|e| bad(path, e))?;
            node.slots.insert(name.clone(), wire_list(&list, path, Some(&name), wire, depth)?);
        }
    }
    if let Some(on) = t.get::<Option<Table>>("on").map_err(|e| bad(path, e))? {
        for pair in on.pairs::<String, LuaValue>() {
            let (event, value) = pair.map_err(|e| bad(path, e))?;
            let handler = wire_handler(&value, path, &event, wire)?;
            node.on.insert(event, handler);
        }
    }
    Ok(node)
}

/// The wire tree of a rendered Lua tree, and the closures of its handlers by `path#event`. The root's key is
/// the window name.
pub(crate) fn wire_tree(root: &Table, window: &str) -> Result<(Node, HashMap<String, Vec<Function>>)> {
    root.set("key", window).map_err(|e| bad(window, e))?;
    let mut wire = Wire::new();
    let node = wire_node(root, window, &mut wire, 0)?;
    Ok((node, wire.fns))
}

fn client_decls(spec: &Table) -> Result<BTreeMap<String, ClientDecl>> {
    let Some(client) = spec.get::<Option<Table>>("client").map_err(|e| VmError::BadValue(e.to_string()))?
    else {
        return Ok(BTreeMap::new());
    };
    let Value::Object(map) =
        lua_to_json(&LuaValue::Table(client)).map_err(|e| VmError::BadValue(e.to_string()))?
    else {
        return Ok(BTreeMap::new());
    };
    map.into_iter()
        .map(|(name, decl)| {
            serde_json::from_value::<ClientDecl>(decl)
                .map(|d| (name.clone(), d))
                .map_err(|e| VmError::InvalidWindow(format!("client.{name}: {e}")))
        })
        .collect()
}

fn source_decl(def: &SourceDef, params: &Value, writable: &BTreeSet<String>) -> SourceDecl {
    let params = params.as_object().cloned().unwrap_or_default();
    match &def.kind {
        SourceKind::Rows { key, page, total } => SourceDecl::Rows {
            key: key.clone(),
            page: *page,
            debounce: None,
            total: Some(match total {
                TotalMode::Exact => WireTotal::Exact,
                TotalMode::Estimate => WireTotal::Estimate,
                TotalMode::None => WireTotal::None,
            }),
            params,
        },
        SourceKind::Row { key, rev, writable: declared, conflict, .. } => SourceDecl::Row {
            key: key.clone(),
            writable: declared.clone().unwrap_or_else(|| writable.iter().cloned().collect()),
            rev: rev.clone(),
            conflict: Some(match conflict {
                Conflict::HumanWins => ConflictPolicy::Merge,
                Conflict::Reject => ConflictPolicy::Reject,
            }),
            params,
        },
        SourceKind::Scalar => SourceDecl::Value { params },
        SourceKind::Thread => SourceDecl::Thread { params },
    }
}

fn ctx_actor(lua: &Lua, ctx: &CallContext) -> mlua::Result<Table> {
    let actor = lua.create_table()?;
    actor.set("kind", ctx.actor.kind.as_str())?;
    actor.set("id", ctx.actor.id.as_str())?;
    actor.set("name", ctx.actor.name.as_str())?;
    Ok(actor)
}

// ---- the window context `w` ------------------------------------------------------------------------------------

fn command(sh: &Arc<Shared>, view: &str, what: &str, c: Command) -> mlua::Result<()> {
    sh.push_view_effect(what, view, ViewEffect::Command(c))
}

fn text_of(v: &LuaValue) -> mlua::Result<String> {
    match v {
        LuaValue::String(s) => Ok(s.to_string_lossy()),
        other => Err(mlua::Error::runtime(format!("expected text, got {}", other.type_name()))),
    }
}

/// The event path prefix of a toast's action button (`$toast/<id>`): never a node path, since keys cannot
/// start with `$`.
const TOAST_PATH: &str = "$toast/";

/// How many toast actions a window keeps (the oldest are forgotten: a toast is gone long before).
const MAX_TOAST_ACTIONS: usize = 32;

/// `w:toast { text, icon?, kind?, duration?, action? = { title, onTap } }`: the toast, and the closure its
/// action button runs (kept by the instance under the id the toast carries).
fn toast_of(opts: &Table) -> mlua::Result<(Toast, Option<(String, Function)>)> {
    let text: String =
        opts.get::<Option<String>>("text")?.ok_or_else(|| mlua::Error::runtime("w:toast needs `text`"))?;
    let action = match opts.get::<Option<Table>>("action")? {
        None => None,
        Some(a) => {
            let title: String = a
                .get::<Option<String>>("title")?
                .ok_or_else(|| mlua::Error::runtime("w:toast action needs `title`"))?;
            let tap: Function = a
                .get::<Option<Function>>("onTap")?
                .ok_or_else(|| mlua::Error::runtime("w:toast action needs `onTap`"))?;
            Some((title, tap))
        }
    };
    let toast = Toast {
        text,
        icon: opts.get::<Option<String>>("icon")?,
        kind: opts.get::<Option<String>>("kind")?,
        action: None,
        duration: opts.get::<Option<u32>>("duration")?,
    };
    Ok((toast, action))
}

#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // a flat table of window methods (s, v, m: shared, view, mirror)
fn build_w(
    lua: &Lua,
    sh: &Arc<Shared>,
    view: &str,
    spec: &Table,
    params: &Map<String, Value>,
    mirror: &Table,
) -> mlua::Result<Table> {
    let w = lua.create_table()?;
    w.set("id", view)?;
    w.set("params", json_to_lua(lua, &Value::Object(params.clone()))?)?;
    let initial_state = spec.get::<Option<Table>>("state")?.unwrap_or(lua.create_table()?);
    w.set("state", make_state(lua, &sh.tracker, &initial_state)?)?;

    // w.client: Refs for every name, plus :get(name) and :set(name, value)
    let client = lua.create_table()?;
    let m = mirror.clone();
    client.set(
        "get",
        lua.create_function(move |_, (_this, name): (LuaValue, String)| m.raw_get::<LuaValue>(name))?,
    )?;
    let (m, s, v) = (mirror.clone(), sh.clone(), view.to_owned());
    client.set(
        "set",
        lua.create_function(
            move |_, (_this, name, value, force): (LuaValue, String, LuaValue, Option<bool>)| {
                s.writable("w.client:set")?; // before the mirror changes, not only before the effect is queued
                let json = lua_to_json(&value)?;
                m.raw_set(name.as_str(), value)?;
                if let Some(opening) = s.views.lock().get_mut(&v).and_then(|i| i.opening.as_mut()) {
                    opening.insert(name.clone(), json.clone());
                }
                s.push_view_effect(
                    "w.client:set",
                    &v,
                    ViewEffect::ClientSet { path: name, value: json, force: force.unwrap_or(false) },
                )
            },
        )?,
    )?;
    w.set("client", ref_proxy(lua, client, "client.".to_owned())?)?;
    w.set("env", ref_proxy(lua, lua.create_table()?, "env.".to_owned())?)?;

    let method = |name: &str, f: Function| w.set(name, f);
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "toast",
        lua.create_function(move |_, (_this, opts): (LuaValue, Table)| {
            s.writable("w:toast")?; // before the action closure is kept
            let (mut toast, action) = toast_of(&opts)?;
            if let Some((title, tap)) = action {
                if let Some(instance) = s.views.lock().get_mut(&v) {
                    instance.toast_seq += 1;
                    let id = format!("t{}", instance.toast_seq);
                    instance.toast_actions.push((id.clone(), tap));
                    if instance.toast_actions.len() > MAX_TOAST_ACTIONS {
                        instance.toast_actions.remove(0);
                    }
                    toast.action = Some(ToastAction { title, id });
                }
            }
            command(&s, &v, "w:toast", Command::Toast(toast))
        })?,
    )?;
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "focus",
        lua.create_function(move |_, (_this, key): (LuaValue, LuaValue)| {
            command(&s, &v, "w:focus", Command::Focus { key: text_of(&key)? })
        })?,
    )?;
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "scrollTo",
        lua.create_function(move |_, (_this, key): (LuaValue, LuaValue)| {
            command(&s, &v, "w:scrollTo", Command::ScrollTo { key: Some(text_of(&key)?), row: None })
        })?,
    )?;
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "announce",
        lua.create_function(move |_, (_this, text): (LuaValue, LuaValue)| {
            command(&s, &v, "w:announce", Command::Announce { text: text_of(&text)? })
        })?,
    )?;
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "copy",
        lua.create_function(move |_, (_this, text): (LuaValue, LuaValue)| {
            command(&s, &v, "w:copy", Command::Copy { text: text_of(&text)? })
        })?,
    )?;
    let (s, v) = (sh.clone(), view.to_owned());
    method(
        "close",
        lua.create_function(move |_, _this: LuaValue| command(&s, &v, "w:close", Command::CloseWindow))?,
    )?;
    let s = sh.clone();
    method(
        "openWindow",
        lua.create_function(move |_, (_this, name, params): (LuaValue, String, Option<LuaValue>)| {
            s.writable("w:openWindow")?;
            s.declared_window(&name)?;
            let params = match params {
                Some(p) => lua_to_json(&p)?,
                None => Value::Null,
            };
            s.push_effect(crate::vm::Effect::OpenWindow { name, params })
        })?,
    )?;
    Ok(w)
}

// ---- AppVm: windows ---------------------------------------------------------------------------------------------

impl AppVm {
    /// The window kinds the app declared.
    pub fn windows(&self) -> Vec<String> {
        self.window_names()
    }

    /// Create the instance, run `onOpen`, render.
    pub async fn open_view(
        &self,
        view: &str,
        window: &str,
        params: Map<String, Value>,
        restored: Map<String, Value>,
    ) -> Result<RenderOutput> {
        let sh = self.shared().clone();
        let spec =
            sh.windows.lock().get(window).cloned().ok_or_else(|| VmError::NoSuchWindow(window.to_owned()))?;
        self.in_call(false, self.open_view_in_call(view, window, spec, params, restored)).await
    }

    /// [`AppVm::open_view`] once the call holds the VM.
    async fn open_view_in_call(
        &self,
        view: &str,
        window: &str,
        spec: Table,
        params: Map<String, Value>,
        restored: Map<String, Value>,
    ) -> Result<RenderOutput> {
        let sh = self.shared().clone();
        let convert = |e: mlua::Error| VmError::BadValue(e.to_string());
        let decls = client_decls(&spec)?;
        let lua = self.lua();

        // declared inits, then what was restored
        let mirror = lua.create_table().map_err(convert)?;
        let mut overrides = Map::new();
        for (name, decl) in &decls {
            mirror.raw_set(name.as_str(), json_to_lua(lua, &decl.init).map_err(convert)?).map_err(convert)?;
            if let Some(v) = restored.get(name) {
                mirror.raw_set(name.as_str(), json_to_lua(lua, v).map_err(convert)?).map_err(convert)?;
                overrides.insert(name.clone(), v.clone());
            }
        }
        let w = build_w(lua, &sh, view, &spec, &params, &mirror).map_err(convert)?;
        sh.views.lock().insert(
            view.to_owned(),
            ViewInstance {
                window: window.to_owned(),
                spec: spec.clone(),
                w: w.clone(),
                mirror,
                decls,
                handlers: HashMap::new(),
                toast_actions: Vec::new(),
                toast_seq: 0,
                deps: BTreeSet::new(),
                render_seq: 0,
                opening: Some(Map::new()),
            },
        );
        let opened = async {
            if let Some(on_open) = spec.get::<Option<Function>>("onOpen").map_err(convert)? {
                let p = json_to_lua(lua, &Value::Object(params)).map_err(convert)?;
                on_open
                    .call_async::<()>((w, p))
                    .await
                    .map_err(|e| VmError::Script(ScriptError::from_lua(&e)))?;
            }
            self.render_inner(view).await
        }
        .await;
        match opened {
            Ok(mut out) => {
                if let Some(opening) = sh.views.lock().get_mut(view).and_then(|i| i.opening.take()) {
                    overrides.extend(opening);
                }
                out.client = overrides;
                Ok(out)
            }
            Err(e) => {
                sh.views.lock().remove(view);
                Err(e)
            }
        }
    }

    /// Render the instance again.
    pub async fn render_view(&self, view: &str) -> Result<RenderOutput> {
        self.in_call(false, self.render_inner(view)).await
    }

    // TODO: clean-code - 0.4 - SRP: runs render under tracking, wires the tree, builds the source bindings and validates in one function; extract `bind_sources` and `check_tree`. Also 0.35 - PANIC: a call cancelled mid-render (the client socket closes at a host call) leaves `tracker.pure` and `render_scope` set until a later render ends; reset them in a drop guard.
    async fn render_inner(&self, view: &str) -> Result<RenderOutput> {
        let sh = self.shared().clone();
        let (spec, w, window, decls) = {
            let views = sh.views.lock();
            let i = views.get(view).ok_or_else(|| VmError::NoSuchView(view.to_owned()))?;
            (i.spec.clone(), i.w.clone(), i.window.clone(), i.decls.clone())
        };
        let render: Function = spec
            .get("render")
            .map_err(|_| VmError::InvalidWindow(format!("window `{window}` has no render function")))?;

        *sh.render_scope.lock() = Some(RenderScope::default());
        sh.tracker.lock().begin(true);
        let rendered = render.call_async::<LuaValue>(w).await;
        let deps = sh.tracker.lock().end();
        let scope = sh.render_scope.lock().take().unwrap_or_default();

        let root = match rendered.map_err(|e| VmError::Script(ScriptError::from_lua(&e)))? {
            LuaValue::Table(t) => t,
            other => {
                return Err(VmError::InvalidWindow(format!(
                    "render must return a node, got {}",
                    other.type_name()
                )))
            }
        };
        let (tree, handlers) = wire_tree(&root, &window)?;

        // sources: one declaration per (definition, params) the render used
        let bound = bound_fields(&tree);
        let writable = writable_by_source(&bound);
        let all_defs = sh.sources.lock().clone();
        let mut sources = BTreeMap::new();
        for (id, def_id, params) in &scope.handles {
            let def = all_defs
                .iter()
                .find(|d| d.id.0 == *def_id)
                .ok_or_else(|| VmError::InvalidWindow(format!("unknown source `{def_id}`")))?;
            let fields = writable.get(id).cloned().unwrap_or_default();
            sources.insert(
                id.clone(),
                SourceBinding { def: SourceId(def_id.clone()), decl: source_decl(def, params, &fields) },
            );
        }

        let decls_for_check = ViewDecls {
            client: decls.clone(),
            sources: sources.iter().map(|(k, b)| (k.clone(), b.decl.clone())).collect(),
        };
        let problems: Vec<String> = validate_with(&tree, &Context::from_decls(&decls_for_check))
            .into_iter()
            .filter(|v| v.severity == Severity::Error)
            .map(|v| v.to_string())
            .collect();
        if !problems.is_empty() {
            return Err(VmError::InvalidTree(problems));
        }

        let seq = {
            let mut views = sh.views.lock();
            let i = views.get_mut(view).ok_or_else(|| VmError::NoSuchView(view.to_owned()))?;
            i.handlers = handlers;
            i.deps = deps;
            i.render_seq += 1;
            i.render_seq
        };
        Ok(RenderOutput { tree, client_decls: decls, sources, client: Map::new(), seq })
    }

    /// Run the closures of `path#event`; `false` when there are none (the node is gone: stale).
    pub async fn dispatch_view_event(
        &self,
        view: &str,
        call: ViewEventCall,
        ctx: &CallContext,
    ) -> Result<bool> {
        let sh = self.shared().clone();
        let closures = {
            let mut views = sh.views.lock();
            let i = views.get_mut(view).ok_or_else(|| VmError::NoSuchView(view.to_owned()))?;
            match call.path.strip_prefix(TOAST_PATH) {
                // a toast's action button: its closure runs once
                Some(id) => i
                    .toast_actions
                    .iter()
                    .position(|(t, _)| t == id)
                    .map(|n| vec![i.toast_actions.remove(n).1]),
                None => i.handlers.get(&format!("{}#{}", call.path, call.event)).cloned(),
            }
        };
        let Some(closures) = closures else { return Ok(false) };
        self.in_call(false, self.run_closures(closures, call, ctx)).await
    }

    /// The closures of one view event, in the call that holds the VM.
    // TODO: clean-code - 0.45 - DRY: `run_closures` (and the onOpen/render/onClose sites) re-implement `vm::script()`: downcast `ToolFailure`, else `ScriptError::from_lua`; use `script()` everywhere so `ctx:fail` behaves the same.
    async fn run_closures(
        &self,
        closures: Vec<Function>,
        call: ViewEventCall,
        ctx: &CallContext,
    ) -> Result<bool> {
        let lua = self.lua();
        let convert = |e: mlua::Error| VmError::BadValue(e.to_string());
        for f in closures {
            let e = lua.create_table().map_err(convert)?;
            if let Some(k) = &call.row_key {
                e.set("key", json_to_lua(lua, k).map_err(convert)?).map_err(convert)?;
            }
            if let Some(r) = &call.row {
                e.set("row", json_to_lua(lua, r).map_err(convert)?).map_err(convert)?;
            }
            if let Some(p) = &call.payload {
                e.set("payload", json_to_lua(lua, p).map_err(convert)?).map_err(convert)?;
                if let Some(v) = p.get("value") {
                    e.set("value", json_to_lua(lua, v).map_err(convert)?).map_err(convert)?;
                }
            }
            e.set("actor", ctx_actor(lua, ctx).map_err(convert)?).map_err(convert)?;
            f.call_async::<()>(e).await.map_err(|e| match e.downcast_ref::<crate::error::ToolFailure>() {
                Some(fail) => VmError::ToolFailed(fail.0.clone()),
                None => VmError::Script(ScriptError::from_lua(&e)),
            })?;
        }
        Ok(true)
    }

    /// The client mirrored synced or persisted entries.
    pub fn set_client_values(&self, view: &str, values: &Map<String, Value>) -> Result<()> {
        let sh = self.shared();
        let views = sh.views.lock();
        let i = views.get(view).ok_or_else(|| VmError::NoSuchView(view.to_owned()))?;
        for (name, value) in values {
            let v = json_to_lua(self.lua(), value).map_err(|e| VmError::BadValue(e.to_string()))?;
            i.mirror.raw_set(name.as_str(), v).map_err(|e| VmError::BadValue(e.to_string()))?;
        }
        Ok(())
    }

    /// Close the instance: `onClose(w)`, then it is gone.
    pub async fn close_view(&self, view: &str) -> Result<()> {
        let sh = self.shared().clone();
        let Some(instance) = sh.views.lock().remove(view) else { return Ok(()) };
        let on_close = instance.spec.get::<Option<Function>>("onClose").ok().flatten();
        if let Some(f) = on_close {
            self.in_call(false, f.call_async::<()>(instance.w.clone()))
                .await
                .map_err(|e| VmError::Script(ScriptError::from_lua(&e)))?;
        }
        Ok(())
    }

    /// Views whose last render read state that has changed since.
    pub fn dirty_views(&self) -> Vec<String> {
        let sh = self.shared();
        let dirty = sh.tracker.lock().take_dirty();
        if dirty.is_empty() {
            return vec![];
        }
        let views = sh.views.lock();
        let mut ids: Vec<String> =
            views.iter().filter(|(_, i)| !i.deps.is_disjoint(&dirty)).map(|(id, _)| id.clone()).collect();
        ids.sort();
        ids
    }

    /// What windows asked for since the last call.
    pub fn take_view_effects(&self) -> Vec<(String, ViewEffect)> {
        std::mem::take(&mut *self.shared().view_effects.lock())
    }
}
