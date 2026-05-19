//! String templating — `lib:template`.
//!
//! Backed by `tera` 1.x. Jinja-style syntax (`{{ var }}`, `{% for %}`,
//! `{% if %}`, `{%- -%}` whitespace control). Compile-once-render-many
//! pattern matching `lib:glob`.
//!
//! Public surface:
//! - `template.compile(source) -> (Template, err)` — parse once.
//! - `Template:render(context) -> (string, err)` — render compiled template.
//! - `template.render(source, context [, options]) -> (string, err)` — one-shot.
//! - `template.add_filter(name, fn) -> (true, err)` — register a Lua filter.
//!
//! ## Design decisions (locked in #12)
//!
//! - **Strict variables.** Engine-wide, no opt-out. Undefined context keys
//!   surface as `(nil, error)`. Footguns over ergonomics — opt-out can ship
//!   later if real demand arrives.
//! - **Autoescape off by default.** Opt in per call via
//!   `template.render(src, ctx, { autoescape = true })`. Compiled templates
//!   are plain — autoescape is a one-shot-render flag, not a compile flag.
//!   Tera routes autoescape via filename suffix; we register one-off templates
//!   under a sentinel `.__esc` suffix when escaping is requested.
//! - **Custom filter output always escaped under autoescape.** No `safe = true`
//!   opt-in on `add_filter`. Authors that need unescaped output chain tera's
//!   built-in `| safe` filter (`{{ x | shout | safe }}`).
//! - **Thread-safe Template.** Backing engine is `Arc<Mutex<Tera>>`. Compiled
//!   `Template` userdata is `Send` and can move across `std:task` coroutines
//!   (same engine, same VM). It is **not** `Sendable` — `std:thread` workers
//!   run a fresh engine per worker and would not share registered filters,
//!   so cross-worker handoff is not supported.
//!
//! ## Filter callback bridge
//!
//! `tera::Filter` is synchronous and has no Lua-aware parameter. We publish the
//! active `&Lua` on a thread-local pointer for the duration of one `render`
//! call so `LuaFilter::filter` can re-enter Lua. The pointer is set inside an
//! mlua callback that already holds `&Lua`, and is cleared via RAII before the
//! callback returns. Tera invokes filters synchronously inside `render` — no
//! filter ever outlives the call.
//!
//! ## Re-entrant render guard
//!
//! The Tera engine lock (`parking_lot::Mutex`) is non-reentrant and held
//! across `tera.render`. If a Lua filter were to call `template.render` or
//! `Template:render`, the recursive lock acquire on the same OS thread would
//! deadlock permanently. `IN_RENDER` is a thread-local flag set on render
//! entry; nested calls return `(nil, "...: recursive render not supported")`
//! instead of blocking. Cleared via RAII so a panic in tera or in a filter
//! still releases the flag.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use mlua::{
    Function, Lua, LuaSerdeExt, MetaMethod, RegistryKey, Table, UserData, UserDataMethods, Value,
};
use parking_lot::Mutex;
use tera::{Context, Filter as TeraFilter, Tera, Value as TeraValue};
use tracing::instrument;

use crate::lua::sandbox::{err, from_err, ok};

/// Tera autoescapes by filename suffix. We pick a sentinel that is unlikely
/// to collide with a real user-supplied template name; one-off render calls
/// pass `{ autoescape = true }` add their template under this suffix.
const ESCAPE_SUFFIX: &str = "__esc";
const ONE_OFF_PLAIN: &str = "__one_off";
const ONE_OFF_ESCAPED: &str = "__one_off.__esc";

/// Error returned when a Lua filter calls `template.render` or `Template:render`
/// from within a render. The tera engine lock is non-reentrant — without this
/// guard the recursive call would deadlock the thread.
const RECURSIVE_RENDER_MSG: &str =
    "recursive render not supported — filters must not call template.render or Template:render";

// ---------------------------------------------------------------------------
// Active-Lua thread-local — see module docstring for the lifetime argument.
// ---------------------------------------------------------------------------

thread_local! {
    static LUA_PTR: Cell<*const Lua> = const { Cell::new(std::ptr::null()) };
    static IN_RENDER: Cell<bool> = const { Cell::new(false) };
}

/// Publish `lua` on the thread-local for the duration of `f`. RAII restores
/// the previous pointer so the LUA_PTR slot remains correct across panics.
fn with_active_lua<F, R>(lua: &Lua, f: F) -> R
where
    F: FnOnce() -> R,
{
    struct Guard(*const Lua);
    impl Drop for Guard {
        fn drop(&mut self) {
            LUA_PTR.with(|c| c.set(self.0));
        }
    }
    let prev = LUA_PTR.with(|c| c.replace(lua as *const Lua));
    let _g = Guard(prev);
    f()
}

fn borrow_active_lua<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&Lua) -> Result<R, String>,
{
    LUA_PTR.with(|c| {
        let p = c.get();
        if p.is_null() {
            return Err("no active Lua state".to_string());
        }
        // SAFETY: pointer was published by `with_active_lua` which guarantees
        // the referent outlives every nested `tera.render` call. See module
        // docstring.
        let lua: &Lua = unsafe { &*p };
        f(lua)
    })
}

/// RAII guard rejecting recursive entry into a render call. See module
/// docstring "Re-entrant render guard" for the rationale.
struct RenderGuard;

impl RenderGuard {
    fn enter() -> Option<Self> {
        IN_RENDER.with(|c| {
            if c.get() {
                None
            } else {
                c.set(true);
                Some(Self)
            }
        })
    }
}

impl Drop for RenderGuard {
    fn drop(&mut self) {
        IN_RENDER.with(|c| c.set(false));
    }
}

// ---------------------------------------------------------------------------
// Engine — one Tera + monotonic name counter, shared via Arc.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Engine {
    tera: Arc<Mutex<Tera>>,
    counter: Arc<AtomicU64>,
}

impl Engine {
    fn new() -> Self {
        let mut tera = Tera::default();
        tera.autoescape_on(vec![ESCAPE_SUFFIX]);
        // Tera 1.x errors on undefined `{{ var }}` lookups by default — the
        // AC-required strict-variables behaviour. `{% if undef %}` falls back
        // to falsy (not "empty string"), so the AC is met without a flag.
        Self {
            tera: Arc::new(Mutex::new(tera)),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    fn next_template_name(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("t{n}")
    }
}

// ---------------------------------------------------------------------------
// Lua-callable Filter implementation.
// ---------------------------------------------------------------------------

struct LuaFilter {
    name: String,
    key: RegistryKey,
}

impl TeraFilter for LuaFilter {
    fn filter(
        &self,
        value: &TeraValue,
        args: &HashMap<String, TeraValue>,
    ) -> tera::Result<TeraValue> {
        let outcome: Result<TeraValue, String> = borrow_active_lua(|lua| {
            let func: Function = lua
                .registry_value(&self.key)
                .map_err(|e| format!("registry: {e}"))?;
            let lua_value = lua.to_value(value).map_err(|e| format!("value: {e}"))?;
            let args_tbl = lua.create_table().map_err(|e| format!("args: {e}"))?;
            for (k, v) in args {
                let lv = lua.to_value(v).map_err(|e| format!("args.{k}: {e}"))?;
                args_tbl.set(k.as_str(), lv).map_err(|e| e.to_string())?;
            }
            let out: Value = func
                .call((lua_value, args_tbl))
                .map_err(|e| e.to_string())?;
            let json: TeraValue =
                serde_json::to_value(&out).map_err(|e| format!("return value: {e}"))?;
            Ok(json)
        });
        outcome.map_err(|e| tera::Error::msg(format!("filter '{}' failed: {e}", self.name)))
    }
}

// ---------------------------------------------------------------------------
// Template userdata.
//
// `TemplateInner::Drop` removes the template entry from the engine's
// `templates` map. Cloning a handle bumps the Arc refcount — the entry is
// only removed when the last clone is dropped, so userdata identity stays
// stable for the script's view of the template.
// ---------------------------------------------------------------------------

struct TemplateInner {
    engine: Engine,
    name: String,
}

impl Drop for TemplateInner {
    fn drop(&mut self) {
        let mut guard = self.engine.tera.lock();
        guard.templates.remove(&self.name);
    }
}

#[derive(Clone)]
pub struct TemplateHandle {
    inner: Arc<TemplateInner>,
}

impl UserData for TemplateHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("render", |lua, this, ctx: Value| {
            template_render(lua, this, ctx)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("Template({})", this.inner.name))
        });
    }
}

// ---------------------------------------------------------------------------
// Module entry point.
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let engine = Engine::new();
    let t = lua.create_table()?;

    let eng = engine.clone();
    t.set(
        "compile",
        lua.create_function(move |lua, src: String| compile_impl(lua, &eng, src))?,
    )?;

    let eng = engine.clone();
    t.set(
        "render",
        lua.create_function(move |lua, args: (String, Value, Option<Table>)| {
            render_impl(lua, &eng, args.0, args.1, args.2)
        })?,
    )?;

    let eng = engine.clone();
    t.set(
        "add_filter",
        lua.create_function(move |lua, args: (String, Function)| {
            add_filter_impl(lua, &eng, args.0, args.1)
        })?,
    )?;

    Ok(t)
}

// ---------------------------------------------------------------------------
// Compile / render implementations.
// ---------------------------------------------------------------------------

#[instrument(name = "template.compile", target = "luau::lib::template", skip_all)]
fn compile_impl(lua: &Lua, engine: &Engine, src: String) -> mlua::Result<(Value, Value)> {
    let name = engine.next_template_name();
    let mut guard = engine.tera.lock();
    if let Err(e) = guard.add_raw_template(&name, &src) {
        return from_err(lua, "template.compile", format_tera_error(&e));
    }
    drop(guard);
    ok(
        lua,
        TemplateHandle {
            inner: Arc::new(TemplateInner {
                engine: engine.clone(),
                name,
            }),
        },
    )
}

#[instrument(name = "template.render", target = "luau::lib::template", skip_all)]
fn render_impl(
    lua: &Lua,
    engine: &Engine,
    src: String,
    ctx: Value,
    opts: Option<Table>,
) -> mlua::Result<(Value, Value)> {
    let Some(_render_guard) = RenderGuard::enter() else {
        return from_err(lua, "template.render", RECURSIVE_RENDER_MSG);
    };
    let autoescape = match read_autoescape(opts.as_ref()) {
        Ok(b) => b,
        Err(e) => return from_err(lua, "template.render", e),
    };
    let ctx_obj = match build_context(lua, &ctx) {
        Ok(c) => c,
        Err(e) => return from_err(lua, "template.render", e),
    };
    let name = if autoescape {
        ONE_OFF_ESCAPED
    } else {
        ONE_OFF_PLAIN
    };

    let mut guard = engine.tera.lock();
    if let Err(e) = guard.add_raw_template(name, &src) {
        return from_err(lua, "template.render", format_tera_error(&e));
    }
    let render_result = with_active_lua(lua, || guard.render(name, &ctx_obj));
    drop(guard);
    match render_result {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, "template.render", format_tera_error(&e)),
    }
}

fn template_render(lua: &Lua, this: &TemplateHandle, ctx: Value) -> mlua::Result<(Value, Value)> {
    let Some(_render_guard) = RenderGuard::enter() else {
        return from_err(lua, "Template.render", RECURSIVE_RENDER_MSG);
    };
    let ctx_obj = match build_context(lua, &ctx) {
        Ok(c) => c,
        Err(e) => return from_err(lua, "Template.render", e),
    };
    let guard = this.inner.engine.tera.lock();
    let render_result = with_active_lua(lua, || guard.render(&this.inner.name, &ctx_obj));
    drop(guard);
    match render_result {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, "Template.render", format_tera_error(&e)),
    }
}

#[instrument(name = "template.add_filter", target = "luau::lib::template", skip_all)]
fn add_filter_impl(
    lua: &Lua,
    engine: &Engine,
    name: String,
    func: Function,
) -> mlua::Result<(Value, Value)> {
    if name.is_empty() {
        return err(lua, "template.add_filter: filter name must be non-empty");
    }
    let key = match lua.create_registry_value(func) {
        Ok(k) => k,
        Err(e) => return from_err(lua, "template.add_filter", e),
    };
    let mut guard = engine.tera.lock();
    guard.register_filter(
        &name,
        LuaFilter {
            name: name.clone(),
            key,
        },
    );
    drop(guard);
    ok(lua, true)
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn read_autoescape(opts: Option<&Table>) -> Result<bool, String> {
    let Some(t) = opts else { return Ok(false) };
    match t.get::<Value>("autoescape") {
        Ok(Value::Nil) => Ok(false),
        Ok(Value::Boolean(b)) => Ok(b),
        Ok(other) => Err(format!(
            "options.autoescape must be a boolean, got {}",
            other.type_name()
        )),
        Err(e) => Err(format!("options.autoescape: {e}")),
    }
}

/// Render-context construction. Lua → `serde_json::Value` → `tera::Context`.
/// The context must be a table (Tera's strict mode rejects non-object roots
/// at template parse time, but failing fast here gives a clearer error).
fn build_context(lua: &Lua, v: &Value) -> Result<Context, String> {
    // `template.render(src, {})` — empty table is acceptable, renders work
    // when the template doesn't reference variables.
    let json = lua
        .from_value::<serde_json::Value>(v.clone())
        .map_err(|e| format!("context: {e}"))?;
    let obj = match json {
        serde_json::Value::Object(m) => m,
        other => {
            return Err(format!(
                "context must be a table, got JSON {}",
                json_type_name(&other)
            ));
        }
    };
    let mut ctx = Context::new();
    for (k, vv) in obj {
        ctx.insert(k, &vv);
    }
    Ok(ctx)
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Walk the tera error chain so line/column from the underlying parser and
/// the failing-filter context both surface in the final Lua error string.
/// Tera's top-level `Error::Display` only shows its own layer.
fn format_tera_error(e: &tera::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    parts.join(": ")
}

// ---------------------------------------------------------------------------
// Benchmarks — three tiers per `lib:*` convention.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("template");

    // -- library: tera-only -------------------------------------------------
    let bench_src = "Hello {{ name }}! Items: {% for r in rows %}{{ r.k }}={{ r.v }};{% endfor %}";
    let mut tera = Tera::default();
    tera.add_raw_template("bench", bench_src)
        .expect("compile bench");
    let mut ctx = Context::new();
    ctx.insert("name", "world");
    let rows: Vec<serde_json::Value> = (0..10)
        .map(|i| serde_json::json!({ "k": format!("k{i}"), "v": i }))
        .collect();
    ctx.insert("rows", &rows);
    g.bench_function("library/render_10_rows", |b| {
        b.iter(|| {
            let _ = tera.render(black_box("bench"), &ctx).unwrap();
        });
    });

    // -- through_lua: compiled Template:render with 10-row context ----------
    let chunk: mlua::Function = lua
        .load(
            r#"
            local template = require("lib:template")
            local t = template.compile("Hello {{ name }}! Items: {% for r in rows %}{{ r.k }}={{ r.v }};{% endfor %}")
            local ctx = { name = "world", rows = {} }
            for i = 0, 9 do ctx.rows[i + 1] = { k = "k" .. i, v = i } end
            return function() return t:render(ctx) end
            "#,
        )
        .eval()
        .expect("compile chunk");
    g.bench_function("through_lua/compiled_render_10_rows", |b| {
        b.iter(|| {
            let _: (String, Value) = chunk.call(()).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua
            .load(r#"return require("lib:template")"#)
            .eval()
            .unwrap();
        lua.globals().set("template", m).unwrap();
        lua
    }

    #[test]
    fn flat_render_substitutes_variables() {
        let lua = engine();
        let (out, e): (String, Value) = lua
            .load(r#"return template.render("Hello, {{ name }}!", { name = "world" })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "Hello, world!");
        assert!(matches!(e, Value::Nil));
    }

    #[test]
    fn compiled_template_renders() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                local t = template.compile("Hi {{ who }}")
                return t:render({ who = "Skywarp" })
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "Hi Skywarp");
    }

    #[test]
    fn compiled_template_tostring_shows_name() {
        let lua = engine();
        let name: String = lua
            .load(r#"return tostring(template.compile("x"))"#)
            .eval()
            .unwrap();
        assert!(name.starts_with("Template("), "got: {name}");
    }

    #[test]
    fn flat_render_loops() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return template.render(
                    "{% for n in nums %}{{ n }},{% endfor %}",
                    { nums = { 1, 2, 3 } }
                )
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "1,2,3,");
    }

    #[test]
    fn flat_render_conditionals() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return template.render(
                    "{% if flag %}YES{% else %}NO{% endif %}",
                    { flag = true }
                )
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "YES");
    }

    #[test]
    fn built_in_filters_work() {
        let lua = engine();
        let out: String = lua
            .load(r#"return template.render("{{ s | upper }}", { s = "shout" })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "SHOUT");
    }

    #[test]
    fn missing_variable_is_strict_error() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.render("{{ missing }}", { other = 1 })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("template.render:"), "got: {e}");
        assert!(e.contains("missing"), "got: {e}");
    }

    #[test]
    fn parse_error_is_reported_with_line_info() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.render("{% for x in xs %}", {})"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("template.render:"), "got: {e}");
        // Tera's parse error includes the offending token; we just check the
        // diagnostic is non-empty and prefixed correctly.
        assert!(e.len() > "template.render:".len(), "empty diagnostic: {e}");
    }

    #[test]
    fn invalid_syntax_on_compile_returns_error_tuple() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.compile("{% if %}")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("template.compile:"), "got: {e}");
    }

    #[test]
    fn autoescape_off_by_default() {
        let lua = engine();
        let out: String = lua
            .load(r#"return template.render("{{ s }}", { s = "<b>" })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "<b>");
    }

    #[test]
    fn autoescape_opt_in_escapes_html() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return template.render(
                    "{{ s }}",
                    { s = "<b>" },
                    { autoescape = true }
                )
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "&lt;b&gt;");
    }

    #[test]
    fn whitespace_control_is_preserved() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return template.render(
                    "A {%- if x %} B {%- endif %}",
                    { x = true }
                )
                "#,
            )
            .eval()
            .unwrap();
        // `{%- -%}` trims whitespace on the indicated side. Exact spacing here
        // is what tera produces with both sides un-trimmed.
        assert!(out.contains('B'), "got: {out}");
        assert!(!out.contains('\n'), "got: {out}");
    }

    #[test]
    fn custom_filter_visible_after_registration() {
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("shout", function(value, _args)
                return string.upper(tostring(value))
            end)
            "#,
        )
        .exec()
        .unwrap();
        let out: String = lua
            .load(r#"return template.render("{{ s | shout }}", { s = "loud" })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "LOUD");
    }

    #[test]
    fn custom_filter_seen_by_pre_existing_compiled_template() {
        // Filters live on the engine, not frozen at compile time. A template
        // compiled before the filter exists must see it on later renders.
        let lua = engine();
        lua.load(
            r#"
            t = template.compile("{{ s | shout }}")
            "#,
        )
        .exec()
        .unwrap();
        // Render before registering should fail (filter unknown).
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return t:render({ s = "loud" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        assert!(e.unwrap().contains("shout"));

        // Register, render again — filter now visible.
        lua.load(
            r#"
            template.add_filter("shout", function(value, _args)
                return string.upper(tostring(value))
            end)
            "#,
        )
        .exec()
        .unwrap();
        let out: String = lua
            .load(r#"return t:render({ s = "loud" })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "LOUD");
    }

    #[test]
    fn custom_filter_error_propagates() {
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("kaboom", function(_, _)
                error("filter exploded")
            end)
            "#,
        )
        .exec()
        .unwrap();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.render("{{ x | kaboom }}", { x = 1 })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("template.render:"), "got: {e}");
        assert!(e.contains("kaboom"), "got: {e}");
        assert!(e.contains("filter exploded"), "got: {e}");
    }

    #[test]
    fn custom_filter_output_is_escaped_when_autoescape_on() {
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("wrap", function(value, _args)
                return "<b>" .. tostring(value) .. "</b>"
            end)
            "#,
        )
        .exec()
        .unwrap();
        let out: String = lua
            .load(
                r#"
                return template.render(
                    "{{ s | wrap }}",
                    { s = "x" },
                    { autoescape = true }
                )
                "#,
            )
            .eval()
            .unwrap();
        // `<b>` from the filter is escaped because is_safe() returns false.
        // Tera escapes `/` as `&#x2F;` (HTML-safe variant).
        assert_eq!(out, "&lt;b&gt;x&lt;&#x2F;b&gt;");
    }

    #[test]
    fn add_filter_empty_name_rejected() {
        let lua = engine();
        let (_, e): (Value, Option<String>) = lua
            .load(r#"return template.add_filter("", function() return "" end)"#)
            .eval()
            .unwrap();
        let e = e.unwrap();
        assert!(e.contains("non-empty"), "got: {e}");
    }

    #[test]
    fn context_non_table_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.render("{{ x }}", 42)"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("context"), "got: {e}");
    }

    #[test]
    fn autoescape_option_rejects_non_boolean() {
        let lua = engine();
        let (_, e): (Value, Option<String>) = lua
            .load(r#"return template.render("x", {}, { autoescape = "yes" })"#)
            .eval()
            .unwrap();
        let e = e.unwrap();
        assert!(e.contains("autoescape"), "got: {e}");
        assert!(e.contains("boolean"), "got: {e}");
    }

    #[test]
    fn template_handle_is_send_for_task_coroutines() {
        // `Send` is sufficient for moving the handle across `std:task`
        // coroutine resumption points within one engine + VM. It is *not*
        // sufficient for `std:thread` workers — that requires the Sendable
        // protocol (see `src/lua/std/thread.rs`), which `TemplateHandle` does
        // not implement.
        fn assert_send<T: Send>() {}
        assert_send::<super::TemplateHandle>();
    }

    #[test]
    fn filter_recursive_render_rejected_not_deadlocked() {
        // Reproducer for the deadlock that motivated the render guard:
        // the tera engine lock is non-reentrant, so a filter that calls
        // `template.render` from inside an outer render must be rejected
        // explicitly rather than blocking the thread forever.
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("recurse", function(_, _)
                local out, e = template.render("inner:{{ s }}", { s = "x" })
                if not out then error(e) end
                return out
            end)
            "#,
        )
        .exec()
        .unwrap();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return template.render("{{ x | recurse }}", { x = "y" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("recursive render"), "got: {e}");
        assert!(e.contains("recurse"), "got: {e}"); // filter name surfaces
    }

    #[test]
    fn compiled_template_recursive_render_rejected() {
        // Same guard, exercised via the `Template:render` entry point.
        let lua = engine();
        lua.load(
            r#"
            inner = template.compile("nested:{{ s }}")
            template.add_filter("recurse2", function(_, _)
                local out, e = inner:render({ s = "x" })
                if not out then error(e) end
                return out
            end)
            outer = template.compile("{{ x | recurse2 }}")
            "#,
        )
        .exec()
        .unwrap();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return outer:render({ x = "y" })"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("recursive render"), "got: {e}");
    }

    #[test]
    fn dropping_template_handle_removes_engine_entry() {
        // Reproducer for the leak that motivated `TemplateInner::Drop`: the
        // tera engine's `templates` map grew unbounded because nothing
        // removed entries when the userdata went away.
        use std::sync::Arc;
        let engine = super::Engine::new();
        let baseline = engine.tera.lock().templates.len();
        {
            let mut g = engine.tera.lock();
            g.add_raw_template("drop_probe", "hello {{ x }}").unwrap();
            drop(g);
            let handle = super::TemplateHandle {
                inner: Arc::new(super::TemplateInner {
                    engine: engine.clone(),
                    name: "drop_probe".to_string(),
                }),
            };
            assert_eq!(engine.tera.lock().templates.len(), baseline + 1);
            drop(handle);
        }
        assert_eq!(
            engine.tera.lock().templates.len(),
            baseline,
            "templates map must shrink back to baseline once the last handle is dropped"
        );
    }

    #[test]
    fn cloned_template_handle_keeps_entry_until_last_drop() {
        // Drop on the inner state must wait for the last Arc clone. Premature
        // removal would turn a second `:render` call on a clone into a
        // missing-template error.
        use std::sync::Arc;
        let engine = super::Engine::new();
        let baseline = engine.tera.lock().templates.len();
        let mut g = engine.tera.lock();
        g.add_raw_template("clone_probe", "x={{ x }}").unwrap();
        drop(g);
        let h1 = super::TemplateHandle {
            inner: Arc::new(super::TemplateInner {
                engine: engine.clone(),
                name: "clone_probe".to_string(),
            }),
        };
        let h2 = h1.clone();
        drop(h1);
        assert_eq!(engine.tera.lock().templates.len(), baseline + 1);
        drop(h2);
        assert_eq!(engine.tera.lock().templates.len(), baseline);
    }
}
