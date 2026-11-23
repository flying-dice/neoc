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
//! ## Limits and refusals
//!
//! Tera has no limits of its own and a deeply nested or recursive template overflows the native stack, which
//! aborts the process, so templates are bounded (the rationale and measurements are in the section comment
//! further down):
//!
//! - `include`, `import` and `extends` are compile errors: a template cannot name another.
//! - At most 24 nested blocks (`for`, `if`, `macro`, `block`, `filter`), 48 operators and 24 nested brackets in
//!   one tag, and 48 levels of blocks and macro calls when rendered (a call is two levels plus the blocks
//!   around it, and a call inside another call's arguments counts one deeper).
//! - A macro may call itself, directly or through others. The depth is bounded at run time: what is left of
//!   the 48 levels after the cycle's own call overhead (a factorial whose call sits in one `if` gets 13
//!   frames, a tree walk with the call under an `if` and a `for` 9), and past that the render fails with
//!   `macro recursion is more than N levels deep`.
//! - A bare macro call may not be an operand of arithmetic or of `<` `>` `<=` `>=` (Tera panics evaluating
//!   it); `set` its result first, or filter it (`self::m() | int + 1` is fine).
//! - `range()` yields at most 16 Mi elements and rejects a zero step.
//! - A panic inside Tera (`i64::MIN % -1` computed from a context value, say) is a render error. That relies on
//!   `catch_unwind`, so it holds only under `panic = "unwind"` (neoc's profile): an embedder built with
//!   `panic = "abort"` still aborts on these panics, and a caught panic still prints the panic hook's message
//!   to stderr.
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
//! `tera::Filter` is synchronous and has no Lua-aware parameter. We publish a
//! clone of the active `Lua` handle (a cheap reference-counted handle to the same VM) on a thread-local
//! for the duration of one `render` call so `LuaFilter::filter` can re-enter Lua. The handle is set inside
//! an mlua callback that already holds the `Lua`, and is restored via RAII before the callback returns.
//! Tera invokes filters synchronously inside `render` — no filter ever outlives the call.
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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use mlua::{
    Function, Lua, LuaSerdeExt, MetaMethod, RegistryKey, Table, UserData, UserDataMethods, Value,
};
use parking_lot::Mutex;
use tera::ast::{
    Expr, ExprVal, FunctionCall, If, LogicExpr, LogicOperator, MathExpr, MathOperator, Node, WS,
};
use tera::{Context, Filter as TeraFilter, Function as TeraFunction, Tera, Value as TeraValue};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};
use crate::lua::serde_guard::{BUFFER_REFUSED, contains_buffer, refuse_buffer};

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
    static ACTIVE_LUA: RefCell<Option<Lua>> = const { RefCell::new(None) };
    static IN_RENDER: Cell<bool> = const { Cell::new(false) };
}

/// Publish `lua` on the thread-local for the duration of `f`. RAII restores
/// the previous handle so the `ACTIVE_LUA` slot remains correct across panics.
fn with_active_lua<F, R>(lua: &Lua, f: F) -> R
where
    F: FnOnce() -> R,
{
    struct Guard(Option<Lua>);
    impl Drop for Guard {
        fn drop(&mut self) {
            ACTIVE_LUA.with(|c| *c.borrow_mut() = self.0.take());
        }
    }
    let prev = ACTIVE_LUA.with(|c| c.borrow_mut().replace(lua.clone()));
    let _g = Guard(prev);
    f()
}

fn borrow_active_lua<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&Lua) -> Result<R, String>,
{
    // Clone the handle out so no `RefCell` borrow is held while `f` runs Lua code.
    let active = ACTIVE_LUA.with(|c| c.borrow().clone());
    match active {
        Some(lua) => f(&lua),
        None => Err("no active Lua state".to_string()),
    }
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
    /// Template names whose handle was dropped while the engine lock was
    /// busy. Drained by [`Engine::lock`]. Never held across Lua or tera calls.
    pending_removal: Arc<Mutex<Vec<String>>>,
}

impl Engine {
    fn new() -> Self {
        let mut tera = Tera::default();
        tera.autoescape_on(vec![ESCAPE_SUFFIX]);
        tera.register_function("range", BoundedRange);
        tera.register_function(DEPTH_GUARD, RecursionLimit);
        // Tera 1.x errors on undefined `{{ var }}` lookups by default — the
        // AC-required strict-variables behaviour. `{% if undef %}` falls back
        // to falsy (not "empty string"), so the AC is met without a flag.
        Self {
            tera: Arc::new(Mutex::new(tera)),
            counter: Arc::new(AtomicU64::new(0)),
            pending_removal: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Lock the tera engine, first dropping templates whose handles were
    /// collected while the lock was busy.
    fn lock(&self) -> parking_lot::MutexGuard<'_, Tera> {
        let mut guard = self.tera.lock();
        for name in self.pending_removal.lock().drain(..) {
            guard.templates.remove(&name);
        }
        guard
    }

    /// Remove a template without ever blocking on the engine lock.
    ///
    /// Lua's GC can finalize a handle while a filter runs inside `render`, on
    /// the thread that already holds the non-reentrant engine lock. Blocking
    /// here would deadlock that thread, so a busy lock defers removal to the
    /// next [`Engine::lock`].
    fn remove_template(&self, name: &str) {
        if let Some(mut guard) = self.tera.try_lock() {
            guard.templates.remove(name);
        } else {
            self.pending_removal.lock().push(name.to_string());
        }
    }

    fn next_template_name(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("t{n}")
    }
}

// ---------------------------------------------------------------------------
// What Tera would let take the host down, and what this module does instead.
//
// - `range(step_by=0)` loops forever pushing onto a `Vec`, and `range(end=1e18)` allocates until the
//   allocator aborts. `range` is replaced by [`BoundedRange`].
// - Tera has no recursion limit, and rendering, parsing and walking the syntax tree are recursive. Every
//   macro frame, block and nested expression costs native stack, and a stack overflow aborts the process.
//   Measured on bare tera 1.20.1 in a debug build on a 2 MiB stack: a macro that calls itself from under
//   one `if` overflows between 90 and 100 frames, and one that calls itself from under an `if`, a `for` and
//   an `if` between 50 and 60 frames, about 280 "levels" (a block, or half a macro call) either way. The
//   budget here, [`MAX_RENDER_DEPTH`] = 48 levels, is about a sixth of that, which leaves the rest of the
//   stack to whatever called the render.
// - [`check_complexity`] reads the source (before Tera sees it) and refuses blocks nested more than
//   [`MAX_BLOCK_DEPTH`] deep and tags with more than [`MAX_OPERATORS`] operators or nested more than
//   [`MAX_NESTING`] brackets; `{% for %}` x 500, `{% if %}` x 500 and `{{ 1 + 1 + ... }}` with 20 000 terms
//   overflow a 2 MiB stack.
// - [`check_renderable`] walks the parsed template and refuses one whose blocks and macro calls stack more than
//   [`MAX_RENDER_DEPTH`] deep when rendered. A call costs two levels plus the blocks around it, and a call
//   inside the arguments of another counts one level deeper than that one. The walk does not recurse over the
//   macro graph ([`call_cycles`] is iterative): a template may define tens of thousands of macros.
// - A macro that calls itself, directly or through others, may terminate on its data (a tree, a factorial), so
//   it is allowed and bounded at run time: [`bound_recursion`] rewrites the parsed macros so each one on a call
//   cycle carries a hidden depth argument, and the render fails with an ordinary error once the depth reaches
//   the levels the budget leaves ([`plan_macros`]).
// - `include`, `import` and `extends` are refused: a template cannot name another (this module holds no
//   registry a name could resolve in, and a self-including template would overflow the stack).
// - A macro call as an operand of arithmetic or of `<` `>` `<=` `>=` reaches an `unreachable!` in Tera and
//   panics: it is refused where it can be seen. What cannot be seen (`i64::MIN % -1` from a context value) is a
//   panic inside the render, which [`render_caught`] turns into an error.
// ---------------------------------------------------------------------------

/// Block tags (`for`, `if`, `macro`, `block`, `filter`) that may be open at once.
const MAX_BLOCK_DEPTH: usize = 24;
/// Binary operators (`+ - * / %`, `== != < > <= >=`, `and`, `or`, `in`; each comparison counts once, a sign
/// or a lone `=` not at all) one tag may hold.
const MAX_OPERATORS: usize = 48;
/// Parentheses and brackets one tag may have open at once.
const MAX_NESTING: usize = 24;
/// The most blocks and macro calls that may be live at once while rendering (a macro call counts as two).
const MAX_RENDER_DEPTH: usize = 48;

/// What a recursive macro leaves free in the render budget for the code that calls it: the call itself (two
/// levels) and up to six blocks around it. A call nested deeper than that into a recursive macro is refused.
const RECURSION_HEADROOM: usize = 8;

/// The most elements `range()` may produce (16 Mi).
const MAX_RANGE_LEN: usize = 1 << 24;

/// Tera's `range(start, end, step_by)` with the two ways it kills a process closed: a zero step is an
/// error, and so is a range longer than [`MAX_RANGE_LEN`].
struct BoundedRange;

impl BoundedRange {
    fn arg(
        args: &HashMap<String, TeraValue>,
        key: &str,
        default: Option<usize>,
    ) -> tera::Result<usize> {
        match args.get(key) {
            Some(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| {
                    tera::Error::msg(format!(
                        "Function `range` received {key}={v} but `{key}` can only be a number"
                    ))
                }),
            None => default.ok_or_else(|| {
                tera::Error::msg(format!(
                    "Function `range` was called without a `{key}` argument"
                ))
            }),
        }
    }
}

impl TeraFunction for BoundedRange {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let start = Self::arg(args, "start", Some(0))?;
        let step_by = Self::arg(args, "step_by", Some(1))?;
        let end = Self::arg(args, "end", None)?;
        if start > end {
            return Err(tera::Error::msg(
                "Function `range` was called with a `start` argument greater than the `end` one",
            ));
        }
        if step_by == 0 {
            return Err(tera::Error::msg(
                "Function `range` was called with a `step_by` of 0",
            ));
        }
        let len = (end - start).div_ceil(step_by);
        if len > MAX_RANGE_LEN {
            return Err(tera::Error::msg(format!(
                "Function `range` would produce {len} values (the most it produces is {MAX_RANGE_LEN})"
            )));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len)
            .map_err(|e| tera::Error::msg(format!("Function `range`: {e}")))?;
        let mut i = start;
        while i < end {
            out.push(TeraValue::from(i));
            i += step_by;
        }
        Ok(TeraValue::Array(out))
    }
}

/// A macro call: the macro's name and how many blocks were open around the call.
struct Call {
    name: String,
    depth: usize,
}

/// What walking one template's syntax tree found.
#[derive(Default)]
struct Scan {
    /// An `include`, `import` or `extends` tag.
    names_another_template: bool,
    /// A macro call used as an operand of arithmetic or of `<` `>` `<=` `>=`, which Tera 1.20 cannot
    /// evaluate: it reaches an `unreachable!` and panics.
    macro_call_operand: bool,
    /// The macros the code being walked (the template's own, or one macro's) calls.
    calls: Vec<Call>,
    /// Blocks open around the node being walked, and the most that were open at once.
    depth: usize,
    peak: usize,
}

/// What the macros of a template call, and how deep their own blocks go.
struct MacroInfo {
    calls: Vec<Call>,
    peak: usize,
}

impl Scan {
    fn nodes(&mut self, nodes: &[Node], macros: &mut HashMap<String, MacroInfo>) {
        for node in nodes {
            self.node(node, macros);
        }
    }

    /// Walk the body of a block: one level deeper.
    fn block(&mut self, body: &[Node], macros: &mut HashMap<String, MacroInfo>) {
        self.depth += 1;
        self.peak = self.peak.max(self.depth);
        self.nodes(body, macros);
        self.depth -= 1;
    }

    fn node(&mut self, node: &Node, macros: &mut HashMap<String, MacroInfo>) {
        match node {
            Node::Extends(..) | Node::Include(..) | Node::ImportMacro(..) => {
                self.names_another_template = true;
            }
            Node::VariableBlock(_, e) => self.expr(e),
            Node::Set(_, set) => self.expr(&set.value),
            Node::FilterSection(_, section, _) => {
                self.call(&section.filter);
                self.block(&section.body, macros);
            }
            Node::Block(_, b, _) => self.block(&b.body, macros),
            Node::Forloop(_, l, _) => {
                self.expr(&l.container);
                self.block(&l.body, macros);
                if let Some(empty) = &l.empty_body {
                    self.block(empty, macros);
                }
            }
            Node::If(i, _) => {
                for (_, cond, body) in &i.conditions {
                    self.expr(cond);
                    self.block(body, macros);
                }
                if let Some((_, body)) = &i.otherwise {
                    self.block(body, macros);
                }
            }
            Node::MacroDefinition(_, def, _) => {
                // the calls inside a macro belong to that macro, not to the code around it, and its blocks
                // start again from zero
                let outer = (std::mem::take(&mut self.calls), self.depth, self.peak);
                (self.depth, self.peak) = (0, 0);
                for default in def.args.values().flatten() {
                    self.expr(default);
                }
                self.nodes(&def.body, macros);
                let info = macros.entry(def.name.clone()).or_insert(MacroInfo {
                    calls: Vec::new(),
                    peak: 0,
                });
                info.calls.append(&mut self.calls);
                info.peak = info.peak.max(self.peak);
                (self.calls, self.depth, self.peak) = outer;
            }
            Node::Super
            | Node::Text(_)
            | Node::Raw(..)
            | Node::Break(_)
            | Node::Continue(_)
            | Node::Comment(..) => {}
        }
    }

    fn call(&mut self, f: &FunctionCall) {
        for e in f.args.values() {
            self.expr(e);
        }
    }

    /// An operand of arithmetic or of an ordering comparison: a bare macro call there is refused. With a
    /// filter (`self::m() | int + 1`) Tera evaluates the call first and does not panic.
    fn operand(&mut self, e: &Expr) {
        if matches!(e.val, ExprVal::MacroCall(_)) && e.filters.is_empty() {
            self.macro_call_operand = true;
        }
        self.expr(e);
    }

    fn expr(&mut self, e: &Expr) {
        self.val(&e.val);
        for f in &e.filters {
            self.call(f);
        }
    }

    fn val(&mut self, v: &ExprVal) {
        match v {
            ExprVal::MacroCall(m) => {
                self.calls.push(Call {
                    name: m.name.clone(),
                    depth: self.depth,
                });
                // Tera evaluates the arguments before it enters the macro, but the calls inside them are
                // still nested expressions of this one: count each level of argument as a level of depth.
                self.depth += 1;
                for e in m.args.values() {
                    self.expr(e);
                }
                self.depth -= 1;
            }
            ExprVal::FunctionCall(f) => self.call(f),
            ExprVal::Math(m) => {
                self.operand(&m.lhs);
                self.operand(&m.rhs);
            }
            ExprVal::Logic(l) => {
                if matches!(
                    l.operator,
                    LogicOperator::Gt | LogicOperator::Gte | LogicOperator::Lt | LogicOperator::Lte
                ) {
                    self.operand(&l.lhs);
                    self.operand(&l.rhs);
                } else {
                    self.expr(&l.lhs);
                    self.expr(&l.rhs);
                }
            }
            ExprVal::In(i) => {
                self.expr(&i.lhs);
                self.expr(&i.rhs);
            }
            ExprVal::Test(t) => t.args.iter().for_each(|e| self.expr(e)),
            ExprVal::Array(items) => items.iter().for_each(|e| self.expr(e)),
            ExprVal::StringConcat(c) => c.values.iter().for_each(|v| self.val(v)),
            ExprVal::String(_)
            | ExprVal::Int(_)
            | ExprVal::Float(_)
            | ExprVal::Bool(_)
            | ExprVal::Ident(_) => {}
        }
    }
}

/// Which macros recurse, and how many levels deep each recursion may go ([`plan_macros`]).
#[derive(Default)]
struct Plan {
    /// For each macro on a call cycle: the cycle it belongs to and the levels it may nest.
    cycles: HashMap<String, (usize, usize)>,
}

/// Refuse a template that could overflow the stack when rendered, and say which macros need a run-time depth
/// bound (see the section comment above).
fn check_renderable(tera: &Tera, name: &str) -> Result<Plan, String> {
    let tpl = tera.get_template(name).map_err(|e| format_tera_error(&e))?;
    let mut scan = Scan::default();
    let mut macros: HashMap<String, MacroInfo> = HashMap::new();
    scan.nodes(&tpl.ast, &mut macros);
    if scan.names_another_template {
        return Err(
            "`include`, `import` and `extends` are not supported: a template cannot name another"
                .into(),
        );
    }
    if scan.macro_call_operand {
        return Err(
            "a macro call cannot be an operand of arithmetic or of `<` `>` `<=` `>=`: \
                    assign its result with `set` first"
                .into(),
        );
    }
    plan_macros(scan.peak, &scan.calls, &macros)
}

/// Strongly connected components of the macro call graph, callees before callers. Iterative: a template can
/// define tens of thousands of macros, and a recursive walk over a chain of them would overflow the stack.
fn call_cycles(edges: &[Vec<(usize, usize)>]) -> Vec<Vec<usize>> {
    let n = edges.len();
    let (mut index, mut low) = (vec![usize::MAX; n], vec![0; n]);
    let (mut on_stack, mut stack, mut next) = (vec![false; n], Vec::new(), 0);
    let mut components = Vec::new();
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut work = vec![(root, 0_usize)];
        while let Some(&(v, edge)) = work.last() {
            if let Some(&(w, _)) = edges[v].get(edge) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                work.pop();
                if low[v] == index[v] {
                    let mut members = Vec::new();
                    while let Some(m) = stack.pop() {
                        on_stack[m] = false;
                        members.push(m);
                        if m == v {
                            break;
                        }
                    }
                    components.push(members);
                }
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[v]);
                }
            }
        }
    }
    components
}

/// How deep rendering goes, in one pass over the call graph (callees first): a macro that is on no cycle
/// reaches the deepest of its own blocks and, for each call, the blocks around it, the call itself (two
/// levels) and the callee's own reach. Macros on a cycle reach as many levels as fit the budget less
/// [`RECURSION_HEADROOM`]: one level costs the blocks around the deepest call on the cycle plus two, and what
/// the cycle calls outside itself and its own deepest blocks come off the budget first. The levels become the
/// run-time bound of [`bound_recursion`].
fn plan_macros(
    top_peak: usize,
    top_calls: &[Call],
    macros: &HashMap<String, MacroInfo>,
) -> Result<Plan, String> {
    let names: Vec<&str> = macros.keys().map(String::as_str).collect();
    let index: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let edges: Vec<Vec<(usize, usize)>> = names
        .iter()
        .map(|n| {
            macros[*n]
                .calls
                .iter()
                .filter_map(|c| index.get(c.name.as_str()).map(|&t| (t, c.depth)))
                .collect()
        })
        .collect();
    let mut reach = vec![0_usize; names.len()];
    let mut plan = Plan::default();
    let components = call_cycles(&edges);
    let mut component = vec![0_usize; names.len()];
    for (id, members) in components.iter().enumerate() {
        for &v in members {
            component[v] = id;
        }
    }
    for (id, members) in components.iter().enumerate() {
        let recursive =
            members.len() > 1 || edges[members[0]].iter().any(|&(w, _)| w == members[0]);
        let (mut cost, mut tail) = (0_usize, 0_usize);
        for &v in members {
            let info = &macros[names[v]];
            tail = tail.max(info.peak);
            for &(w, depth) in &edges[v] {
                if component[w] == id {
                    cost = cost.max(depth + 2);
                } else {
                    tail = tail.max(depth + 2 + reach[w]);
                }
            }
        }
        if !recursive {
            reach[members[0]] = tail;
            continue;
        }
        if tail + cost + RECURSION_HEADROOM > MAX_RENDER_DEPTH {
            return Err(format!(
                "template too complex: macro `{}` recurses, and its blocks and calls leave no room for even one \
                 level (the most is {MAX_RENDER_DEPTH})",
                names[members[0]]
            ));
        }
        let levels = (MAX_RENDER_DEPTH - RECURSION_HEADROOM - tail) / cost;
        for &v in members {
            reach[v] = tail + levels * cost;
            plan.cycles.insert(names[v].to_owned(), (id, levels));
        }
    }
    let top = top_calls
        .iter()
        .filter_map(|c| index.get(c.name.as_str()).map(|&t| c.depth + 2 + reach[t]))
        .fold(top_peak, usize::max);
    if top > MAX_RENDER_DEPTH {
        return Err(format!(
            "template too complex: blocks and macro calls nest {top} deep when rendered (the most is {MAX_RENDER_DEPTH})"
        ));
    }
    Ok(plan)
}

/// The name of the hidden depth argument of a recursive macro. It contains a space, so no template can name,
/// read or shadow it.
const DEPTH_ARG: &str = "neoc depth";

/// The name of the function the depth guard calls. Also unreachable from a template (a space).
const DEPTH_GUARD: &str = "neoc macro recursion limit";

/// Raises the error that ends a macro recursion past its bound.
struct RecursionLimit;

impl TeraFunction for RecursionLimit {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let levels = args.get("levels").and_then(TeraValue::as_i64).unwrap_or(0);
        Err(tera::Error::msg(format!(
            "macro recursion is more than {levels} levels deep"
        )))
    }
}

fn int(n: i64) -> Expr {
    Expr::new(ExprVal::Int(n))
}

fn depth() -> Expr {
    Expr::new(ExprVal::Ident(DEPTH_ARG.into()))
}

/// Bound the recursion of the macros `plan` found on call cycles, at run time. Tera has no recursion limit, and a
/// macro that terminates on its data is fine; only an unbounded one aborts the host. Each such macro gets a
/// hidden depth argument (default 0), every call to a macro of its own cycle passes the depth plus one, and the
/// body starts by raising an error once the depth reaches the levels the stack budget allows.
fn bound_recursion(tera: &mut Tera, name: &str, plan: &Plan) {
    if plan.cycles.is_empty() {
        return;
    }
    let Some(template) = tera.templates.get_mut(name) else {
        return;
    };
    for (macro_name, def) in &mut template.macros {
        let Some(&(cycle, levels)) = plan.cycles.get(macro_name) else {
            continue;
        };
        for default in def.args.values_mut().flatten() {
            bump_calls_in_expr(default, plan, cycle);
        }
        bump_calls(&mut def.body, plan, cycle);
        def.args.insert(DEPTH_ARG.into(), Some(int(0)));
        let guard = Node::If(
            If {
                conditions: vec![(
                    WS::default(),
                    Expr::new(ExprVal::Logic(LogicExpr {
                        lhs: Box::new(depth()),
                        rhs: Box::new(int(i64::try_from(levels).unwrap_or(i64::MAX))),
                        operator: LogicOperator::Gte,
                    })),
                    vec![Node::VariableBlock(
                        WS::default(),
                        Expr::new(ExprVal::FunctionCall(FunctionCall {
                            name: DEPTH_GUARD.into(),
                            args: HashMap::from([(
                                "levels".to_owned(),
                                int(i64::try_from(levels).unwrap_or(i64::MAX)),
                            )]),
                        })),
                    )],
                )],
                otherwise: None,
            },
            WS::default(),
        );
        def.body.insert(0, guard);
    }
}

fn bump_calls(nodes: &mut [Node], plan: &Plan, cycle: usize) {
    for node in nodes {
        match node {
            Node::VariableBlock(_, e) => bump_calls_in_expr(e, plan, cycle),
            Node::Set(_, set) => bump_calls_in_expr(&mut set.value, plan, cycle),
            Node::FilterSection(_, section, _) => {
                for e in section.filter.args.values_mut() {
                    bump_calls_in_expr(e, plan, cycle);
                }
                bump_calls(&mut section.body, plan, cycle);
            }
            Node::Block(_, b, _) => bump_calls(&mut b.body, plan, cycle),
            Node::Forloop(_, l, _) => {
                bump_calls_in_expr(&mut l.container, plan, cycle);
                bump_calls(&mut l.body, plan, cycle);
                if let Some(empty) = &mut l.empty_body {
                    bump_calls(empty, plan, cycle);
                }
            }
            Node::If(i, _) => {
                for (_, cond, body) in &mut i.conditions {
                    bump_calls_in_expr(cond, plan, cycle);
                    bump_calls(body, plan, cycle);
                }
                if let Some((_, body)) = &mut i.otherwise {
                    bump_calls(body, plan, cycle);
                }
            }
            _ => {}
        }
    }
}

fn bump_calls_in_expr(e: &mut Expr, plan: &Plan, cycle: usize) {
    bump_calls_in_val(&mut e.val, plan, cycle);
    for f in &mut e.filters {
        for arg in f.args.values_mut() {
            bump_calls_in_expr(arg, plan, cycle);
        }
    }
}

fn bump_calls_in_val(v: &mut ExprVal, plan: &Plan, cycle: usize) {
    match v {
        ExprVal::MacroCall(m) => {
            for arg in m.args.values_mut() {
                bump_calls_in_expr(arg, plan, cycle);
            }
            if plan.cycles.get(&m.name).is_some_and(|&(c, _)| c == cycle) {
                m.args.insert(
                    DEPTH_ARG.into(),
                    Expr::new(ExprVal::Math(MathExpr {
                        lhs: Box::new(depth()),
                        rhs: Box::new(int(1)),
                        operator: MathOperator::Add,
                    })),
                );
            }
        }
        ExprVal::FunctionCall(f) => {
            for arg in f.args.values_mut() {
                bump_calls_in_expr(arg, plan, cycle);
            }
        }
        ExprVal::Math(m) => {
            bump_calls_in_expr(&mut m.lhs, plan, cycle);
            bump_calls_in_expr(&mut m.rhs, plan, cycle);
        }
        ExprVal::Logic(l) => {
            bump_calls_in_expr(&mut l.lhs, plan, cycle);
            bump_calls_in_expr(&mut l.rhs, plan, cycle);
        }
        ExprVal::In(i) => {
            bump_calls_in_expr(&mut i.lhs, plan, cycle);
            bump_calls_in_expr(&mut i.rhs, plan, cycle);
        }
        ExprVal::Test(t) => {
            for arg in &mut t.args {
                bump_calls_in_expr(arg, plan, cycle);
            }
        }
        ExprVal::Array(items) => {
            for item in items {
                bump_calls_in_expr(item, plan, cycle);
            }
        }
        ExprVal::StringConcat(c) => {
            for value in &mut c.values {
                bump_calls_in_val(value, plan, cycle);
            }
        }
        ExprVal::String(_)
        | ExprVal::Int(_)
        | ExprVal::Float(_)
        | ExprVal::Bool(_)
        | ExprVal::Ident(_) => {}
    }
}

/// Refuse source that nests too deeply for the recursive parser, walker and renderer (see the section comment
/// above). It reads tags the way Tera's grammar delimits them (`{{ }}`, `{% %}`, `{# #}`, quoted strings inside
/// a tag, the body of `{% raw %}` as text) and is a count, not a parse, so it runs before Tera does.
fn check_complexity(src: &str) -> Result<(), String> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut at = 0;
    while let Some(open) = find(bytes, at, b"{").filter(|&i| i + 1 < bytes.len()) {
        let kind = bytes[open + 1];
        if !matches!(kind, b'{' | b'%' | b'#') {
            at = open + 1;
            continue;
        }
        let body = open + 2;
        if kind == b'#' {
            at = find(bytes, body, b"#}").map_or(bytes.len(), |end| end + 2);
            continue;
        }
        let close: &[u8] = if kind == b'{' { b"}}" } else { b"%}" };
        let end = scan_tag(bytes, body, close)?;
        at = end;
        if kind != b'%' {
            continue;
        }
        match first_word(bytes, body) {
            b"for" | b"if" | b"macro" | b"block" | b"filter" => {
                depth += 1;
                if depth > MAX_BLOCK_DEPTH {
                    return Err(format!(
                        "template too complex: blocks are nested more than {MAX_BLOCK_DEPTH} deep"
                    ));
                }
            }
            b"endfor" | b"endif" | b"endmacro" | b"endblock" | b"endfilter" => {
                depth = depth.saturating_sub(1);
            }
            b"raw" => at = skip_raw(bytes, at),
            _ => {}
        }
    }
    Ok(())
}

/// Where `needle` first occurs in `bytes` at or after `from`.
fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// The first word of a tag body (after spaces and a whitespace-control `-`).
fn first_word(bytes: &[u8], body: usize) -> &[u8] {
    let rest = bytes.get(body..).unwrap_or_default();
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace() && *b != b'-')
        .unwrap_or(rest.len());
    let word = &rest[start..];
    let len = word
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
        .unwrap_or(word.len());
    &word[..len]
}

/// Walk one tag from `body` to its `close`, counting operators and nesting; returns where the tag ends. An
/// unterminated tag ends at the end of the source (Tera reports it), after the same checks.
fn scan_tag(bytes: &[u8], body: usize, close: &[u8]) -> Result<usize, String> {
    let (mut i, mut quote, mut operators, mut nesting) = (body, None, 0usize, 0usize);
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(close) {
            return Ok(i + close.len());
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'(' | b'[' => {
                nesting += 1;
                if nesting > MAX_NESTING {
                    return Err(format!(
                        "template too complex: more than {MAX_NESTING} parentheses or brackets nested in one tag"
                    ));
                }
            }
            b')' | b']' => nesting = nesting.saturating_sub(1),
            b'+' | b'*' | b'/' | b'%' | b'<' | b'>' => operators += 1,
            // `==` and `!=` are one operator each, a lone `=` (a `set` or a keyword argument) is none, and the
            // `=` of `<=` and `>=` belongs to the `<` or `>` already counted
            b'!' if bytes.get(i + 1) == Some(&b'=') => operators += 1,
            b'=' if bytes.get(i + 1) == Some(&b'=')
                && !matches!(
                    i.checked_sub(1).map(|p| bytes[p]),
                    Some(b'<' | b'>' | b'!' | b'=')
                ) =>
            {
                operators += 1;
            }
            // binary minus only: a sign (`-1`, `{%- ... -%}`, `(-x)`) follows no operand
            b'-' if follows_operand(bytes, body, i) => operators += 1,
            c if c.is_ascii_alphabetic() => {
                let word = &bytes[i..];
                let len = word
                    .iter()
                    .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                    .unwrap_or(word.len());
                if matches!(&word[..len], b"and" | b"or" | b"in") {
                    operators += 1;
                }
                // a word is read whole, so `android` is not `and`
                i += len;
                if operators > MAX_OPERATORS {
                    return Err(too_many_operators());
                }
                continue;
            }
            _ => {}
        }
        if operators > MAX_OPERATORS {
            return Err(too_many_operators());
        }
        i += 1;
    }
    Ok(bytes.len())
}

/// Whether the last non-space byte before `at` (not before `body`) ends an operand, so a `-` there is a
/// subtraction and not a sign.
fn follows_operand(bytes: &[u8], body: usize, at: usize) -> bool {
    bytes[body..at]
        .iter()
        .rev()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| {
            b.is_ascii_alphanumeric()
                || matches!(b, b'_' | b')' | b']' | b'"' | b'\'' | b'`' | b'.')
        })
}

fn too_many_operators() -> String {
    format!("template too complex: more than {MAX_OPERATORS} operators in one tag")
}

/// Where the text of a `{% raw %}` block ends: just past its `{% endraw %}` (or the end of the source).
fn skip_raw(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while let Some(open) = find(bytes, at, b"{%") {
        if first_word(bytes, open + 2) == b"endraw" {
            return find(bytes, open + 2, b"%}").map_or(bytes.len(), |end| end + 2);
        }
        at = open + 2;
    }
    bytes.len()
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
            // a filter is called inside Tera, which can only fail the render: `(nil, "...: <BUFFER_REFUSED>")`
            if contains_buffer(&out).map_err(|e| format!("return value: {e}"))? {
                return Err(format!("return value: {BUFFER_REFUSED}"));
            }
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
        self.engine.remove_template(&self.name);
    }
}

#[derive(Clone)]
pub struct TemplateHandle {
    inner: Arc<TemplateInner>,
}

#[neoc_macros::lua_userdata("lib:template", "Template")]
impl UserData for TemplateHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Render the compiled template with `context` as the variable map.
        #[lua("Result<string, string>", names(ctx = "context"))]
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
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:template` surface through the facade.
///
/// `render` and `add_filter` are fallible and routed through
/// [`crate::lua::facade::Module::try_func_raising`] and
/// [`crate::lua::facade::Module::try_func`]: each inner function returns the
/// bare failure reason and the facade prepends the `"template.<fn>: "` prefix;
/// `render` raises for a context holding a buffer.
///
/// `compile` is a *mixed* raise-plus-channel shape: a parse failure goes to the
/// `(value, error)` channel via [`crate::lua::sandbox::from_err`], but the
/// compiled `Template` handle's `into_lua` (done by `ok`) raises an
/// `mlua::Error` directly — a mix `try_func` cannot model — so it is registered
/// verbatim with [`crate::lua::facade::Module::set`] (keeping its
/// `template.compile` span and channel prefix) and recorded with
/// what `neoc_macros` reads from the implementation. The `(value, error)` shape, the
/// exact error strings, the raise on `into_lua`, and the `lua::lib::template`
/// spans are byte-identical to the hand-written registration this replaces
/// (which never saw a buffer: one raises from `render` and `Template:render`). The
/// shared `Engine` is cloned into each closure exactly as before. The compiled
/// `Template` userdata keeps its own `impl UserData` and is recorded here via
/// [`crate::lua::facade::Module::userdata`].
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let engine = Engine::new();
    let mut m = Module::new(lua, "lib:template")?;

    let eng = engine.clone();
    /// Parse `source` once and return a reusable `Template`.
    #[lua("Result<Template, string>", names(src = "source"))]
    m.set(
        "compile",
        lua.create_function(move |lua, src: String| compile_impl(lua, &eng, src))?,
    )?;

    let eng = engine.clone();
    /// One-shot parse and render of `source` against `context`; `options.autoescape` toggles HTML escaping.
    #[lua(names(src = "source", ctx = "context", opts = "options"))]
    m.try_func_raising(
        "render",
        move |lua, (src, ctx, opts): (String, Value, Option<Table>)| {
            refuse_buffer(&ctx, "template.render")?;
            Ok(render_impl(lua, &eng, src, ctx, opts))
        },
    )?;

    let eng = engine.clone();
    /// Register a Lua function as a custom tera filter, visible to every subsequent render.
    #[lua(names(func = "fn"))]
    m.try_func(
        "add_filter",
        move |lua, (name, func): (String, Function)| add_filter_impl(lua, &eng, name, func),
    )?;

    m.userdata("Template", |t| {
        TemplateHandle::lua(t);
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Compile / render implementations.
// ---------------------------------------------------------------------------

#[instrument(name = "template.compile", target = "lua::lib::template", skip_all)]
fn compile_impl(lua: &Lua, engine: &Engine, src: String) -> mlua::Result<(Value, Value)> {
    if let Err(e) = check_complexity(&src) {
        return from_err(lua, "template.compile", e);
    }
    let name = engine.next_template_name();
    let mut guard = engine.lock();
    if let Err(e) = guard.add_raw_template(&name, &src) {
        // Tera keeps a template whose `extends` or `import` it could not resolve, and then fails every later
        // `add_raw_template` while building the inheritance chains. The name is never handed out: drop it.
        guard.templates.remove(&name);
        return from_err(lua, "template.compile", format_tera_error(&e));
    }
    match check_renderable(&guard, &name) {
        Ok(plan) => bound_recursion(&mut guard, &name, &plan),
        Err(e) => {
            // drop the refused source: the name is never handed out, so nothing else would
            guard.templates.remove(&name);
            return from_err(lua, "template.compile", e);
        }
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

fn render_impl(
    lua: &Lua,
    engine: &Engine,
    src: String,
    ctx: Value,
    opts: Option<Table>,
) -> Result<String, String> {
    let Some(_render_guard) = RenderGuard::enter() else {
        return Err(RECURSIVE_RENDER_MSG.to_string());
    };
    let autoescape = read_autoescape(opts.as_ref())?;
    check_complexity(&src)?;
    let ctx_obj = build_context(lua, &ctx)?;
    let name = if autoescape {
        ONE_OFF_ESCAPED
    } else {
        ONE_OFF_PLAIN
    };

    let mut guard = engine.lock();
    if let Err(e) = guard.add_raw_template(name, &src) {
        // see `compile_impl`: a template Tera half-added would break every later one
        guard.templates.remove(name);
        return Err(format_tera_error(&e));
    }
    match check_renderable(&guard, name) {
        Ok(plan) => bound_recursion(&mut guard, name, &plan),
        Err(e) => {
            guard.templates.remove(name);
            return Err(e);
        }
    }
    let rendered = render_caught(lua, &guard, name, &ctx_obj);
    drop(guard);
    rendered
}

/// Render, turning a panic inside Tera into an error. Tera panics on a few inputs no check can see coming (an
/// `i64::MIN % -1` computed from a context value, for one); a panic must not take the host with it.
fn render_caught(lua: &Lua, tera: &Tera, name: &str, ctx: &Context) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| {
        with_active_lua(lua, || tera.render(name, ctx))
    })) {
        Ok(rendered) => rendered.map_err(|e| format_tera_error(&e)),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(format!(
                "the template engine failed on this template: {reason}"
            ))
        }
    }
}

fn template_render(lua: &Lua, this: &TemplateHandle, ctx: Value) -> mlua::Result<(Value, Value)> {
    refuse_buffer(&ctx, "Template.render")?;
    let Some(_render_guard) = RenderGuard::enter() else {
        return from_err(lua, "Template.render", RECURSIVE_RENDER_MSG);
    };
    let ctx_obj = match build_context(lua, &ctx) {
        Ok(c) => c,
        Err(e) => return from_err(lua, "Template.render", e),
    };
    let guard = this.inner.engine.lock();
    let rendered = render_caught(lua, &guard, &this.inner.name, &ctx_obj);
    drop(guard);
    match rendered {
        Ok(s) => ok(lua, s),
        Err(e) => from_err(lua, "Template.render", e),
    }
}

fn add_filter_impl(
    lua: &Lua,
    engine: &Engine,
    name: String,
    func: Function,
) -> Result<bool, String> {
    if name.is_empty() {
        return Err("filter name must be non-empty".to_string());
    }
    let key = lua.create_registry_value(func).map_err(|e| e.to_string())?;
    let mut guard = engine.lock();
    guard.register_filter(
        &name,
        LuaFilter {
            name: name.clone(),
            key,
        },
    );
    drop(guard);
    Ok(true)
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
    // the recursion guard is an implementation detail: its error reads "macro recursion is more than N levels
    // deep", without Tera's "Function call '...' failed" wrapper naming the guard
    let guard = format!("Function call '{DEPTH_GUARD}' failed");
    let mut parts = vec![e.to_string()];
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    parts.retain(|part| *part != guard);
    parts.join(": ")
}

// ---------------------------------------------------------------------------
// Benchmarks — three tiers per `lib:*` convention.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use std::hint::black_box;

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

    fn render_err(src: &str) -> String {
        let lua = engine();
        let (v, e): (Value, String) = lua
            .load("return template.render(...)")
            .call((src, lua.create_table().unwrap()))
            .unwrap_or_else(|e| panic!("{src}: threw {e}"));
        assert!(matches!(v, Value::Nil), "{src}: rendered {v:?}");
        e
    }

    /// `range(step_by=0)` looped forever growing a `Vec`; `range(end=1e18)` allocated until the allocator
    /// aborted.
    #[test]
    fn range_with_a_zero_step_or_an_absurd_length_is_an_error() {
        assert!(
            render_err("{% for i in range(end=10, step_by=0) %}{{ i }}{% endfor %}")
                .contains("step_by")
        );
        assert!(
            render_err("{% for i in range(end=1000000000000000000) %}x{% endfor %}")
                .contains("the most it produces")
        );
        assert!(
            render_err("{% for i in range(end=10, step_by=-1) %}{% endfor %}").contains("step_by")
        );
        // and range still works
        let lua = engine();
        let out: String = lua
            .load(r#"return template.render("{% for i in range(start=1, end=10, step_by=3) %}{{ i }},{% endfor %}", {})"#)
            .eval()
            .unwrap();
        assert_eq!(out, "1,4,7,");
    }

    /// Tera has no recursion limit: an unbounded macro overflowed the Rust stack (an abort). It is an ordinary
    /// render error now, on the 2 MiB stack of a test thread.
    #[test]
    fn unbounded_macro_recursion_and_self_including_templates_are_errors() {
        let direct = "{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}";
        let direct_err = render_err(direct);
        assert!(direct_err.contains("levels deep"));
        assert!(
            !direct_err.contains("neoc macro recursion limit"),
            "{direct_err}"
        );
        assert!(direct_err.ends_with("macro recursion is more than 20 levels deep"));
        let mutual = "{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}x{{ self::a() }}{% endmacro b %}{{ self::a() }}";
        assert!(render_err(mutual).contains("levels deep"));
        let nested = "{% macro f() %}{% if true %}{% for i in range(end=2) %}{{ self::f() | upper }}{% endfor %}{% endif %}{% endmacro f %}{{ self::f() }}";
        assert!(render_err(nested).contains("levels deep"));
        assert!(render_err(r#"{% include "__one_off" %}"#).contains("not supported"));
        assert!(!render_err(r#"{% import "t0" as m %}"#).is_empty());
        // a compiled template reports it when rendered, every time, and a plain macro still works
        let lua = engine();
        let (v, e): (Value, String) = lua
            .load("local t = template.compile(...) local a, b = t:render({}) return t:render({})")
            .call(direct)
            .unwrap();
        assert!(matches!(v, Value::Nil) && e.contains("levels deep"), "{e}");
        let out: String = lua
            .load(r#"return template.render("{% macro hi(n) %}hi {{ n }}{% endmacro hi %}{{ self::hi(n=1) }}", {})"#)
            .eval()
            .unwrap();
        assert_eq!(out, "hi 1");
    }

    /// A macro that terminates on its data renders, to the depth the stack budget allows.
    #[test]
    fn terminating_recursive_macros_render() {
        let fact = |n: i64| {
            format!(
                "{{% macro fact(n) %}}{{% if n <= 1 %}}1{{% else %}}{{{{ n }}}}*{{{{ self::fact(n=n-1) }}}}{{% endif %}}{{% endmacro fact %}}{{{{ self::fact(n={n}) }}}}"
            )
        };
        assert_eq!(render_ok(&fact(10)), "10*9*8*7*6*5*4*3*2*1");
        // the call is under one `if`: three units a level, and 40 of the 48 are the recursion's (8 are left for the
        // caller), so 13 levels fit and 14 do not
        assert!(render_ok(&fact(13)).starts_with("13*12*"));
        assert!(render_err(&fact(14)).contains("levels deep"));
        let tree = "{% macro node(t) %}<li>{{ t.name }}{% if t.children %}<ul>{% for c in t.children %}{{ self::node(t=c) }}{% endfor %}</ul>{% endif %}</li>{% endmacro node %}<ul>{{ self::node(t=tree) }}</ul>";
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return template.render(..., { tree = { name = "a", children = {
                    { name = "b", children = {} },
                    { name = "c", children = { { name = "d", children = {} } } } } } })
                "#,
            )
            .call(tree)
            .unwrap();
        assert_eq!(
            out,
            "<ul><li>a<ul><li>b</li><li>c<ul><li>d</li></ul></li></ul></li></ul>"
        );
    }

    /// Tera 1.20 panics (`unreachable!`) when a bare macro call is an operand of arithmetic or of an ordering
    /// comparison; nothing valid is lost by refusing it where it can be seen. An `==` or `and` operand is
    /// fine, and so is a filtered call (`self::m() | int + 1`).
    #[test]
    fn a_macro_call_as_an_arithmetic_or_ordering_operand_is_refused() {
        let m = "{% macro m() %}1{% endmacro m %}";
        for shape in [
            "{{ 1 + self::m() }}",
            "{{ self::m() * 2 }}",
            "{{ (self::m()) % 2 }}",
            "{% if self::m() > 0 %}a{% endif %}",
            "{% if 0 <= self::m() %}a{% endif %}",
        ] {
            let e = render_err(&format!("{m}{shape}"));
            assert!(e.contains("cannot be an operand"), "{shape}: {e}");
        }
        assert_eq!(
            render_ok(&format!("{m}{{% if self::m() == \"1\" %}}a{{% endif %}}")),
            "a"
        );
        assert_eq!(
            render_ok(&format!(
                "{m}{{% set x = self::m() %}}{{{{ x | int + 1 }}}}"
            )),
            "2"
        );
        // a filtered call is evaluated first and does not panic: it renders
        assert_eq!(render_ok(&format!("{m}{{{{ self::m() | int + 1 }}}}")), "2");
        assert_eq!(
            render_ok(&format!("{m}{{% if self::m() | int > 0 %}}a{{% endif %}}")),
            "a"
        );
    }

    /// `i64::MIN % -1` overflows inside Tera and panics; a value from the context cannot be seen by any check.
    /// It is a render error, not a host panic.
    #[test]
    fn a_panic_inside_the_renderer_is_a_render_error() {
        let lua = engine();
        let (out, e): (Option<String>, Option<String>) = lua
            .load("return template.render('{{ (0 - 9223372036854775807 - 1) % -1 }}', {})")
            .eval()
            .unwrap();
        assert!(out.is_none());
        assert!(e.unwrap().contains("template engine failed"));
        let (out, e): (Option<String>, Option<String>) = lua
            .load("local t = template.compile('{{ (0 - 9223372036854775807 - 1) % -1 }}') return t:render({})")
            .eval()
            .unwrap();
        assert!(out.is_none());
        assert!(e.unwrap().contains("template engine failed"));
        // the engine still renders afterwards
        let ok: String = lua
            .load("return template.render('{{ 1 + 1 }}', {})")
            .eval()
            .unwrap();
        assert_eq!(ok, "2");
    }

    fn render_ok(src: &str) -> String {
        let lua = engine();
        let (out, e): (Option<String>, Option<String>) = lua
            .load("return template.render(...)")
            .call((src, lua.create_table().unwrap()))
            .unwrap_or_else(|e| panic!("{src:.60}: threw {e}"));
        out.unwrap_or_else(|| panic!("{src:.60}: {e:?}"))
    }

    /// Tera recurses over the syntax tree with no limit. Each of these overflowed a 2 MiB stack (the default of
    /// a test thread and of a runtime worker) and aborted the host.
    #[test]
    fn templates_that_nest_or_chain_too_deeply_are_refused_not_overflowed() {
        let cases = [
            format!(
                "{}x{}",
                "{% for i in [1] %}".repeat(500),
                "{% endfor %}".repeat(500)
            ),
            format!(
                "{}x{}",
                "{% if true %}".repeat(500),
                "{% endif %}".repeat(500)
            ),
            format!("{{{{ 1{} }}}}", " + 1".repeat(20_000)),
            format!(
                "{{% if true{} %}}x{{% endif %}}",
                " and true".repeat(20_000)
            ),
            format!("{{{{ {}1{} }}}}", "(".repeat(5_000), ")".repeat(5_000)),
            format!("{{{{ {}1{} }}}}", "[".repeat(5_000), "]".repeat(5_000)),
            // an unterminated one is Tera's to report, but it must not recurse either
            "{{ 1".to_owned() + &" + 1".repeat(20_000),
        ];
        for src in &cases {
            let e = render_err(src);
            assert!(e.contains("too complex"), "{src:.60}: {e}");
            let lua = engine();
            let (v, e): (Value, String) = lua
                .load("return template.compile(...)")
                .call(src.as_str())
                .unwrap();
            assert!(
                matches!(v, Value::Nil) && e.contains("too complex"),
                "compile {src:.60}: {e}"
            );
        }
    }

    /// A chain of macros, none recursive, each calling the next: the render nests as deep as the chain is long.
    #[test]
    fn a_long_chain_of_macros_is_refused() {
        let mut src = String::new();
        for i in 0..1000 {
            src.push_str(&format!(
                "{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}",
                i + 1
            ));
        }
        src.push_str("{% macro m1000() %}x{% endmacro m1000 %}{{ self::m0() }}");
        assert!(render_err(&src).contains("nest"), "refused for its depth");
    }

    /// The limits are where the stack is still safe: a template at each of them renders on this 2 MiB thread.
    #[test]
    fn templates_at_the_limits_render() {
        let blocks = format!(
            "{}x{}",
            "{% for i in [1] %}".repeat(super::MAX_BLOCK_DEPTH),
            "{% endfor %}".repeat(super::MAX_BLOCK_DEPTH)
        );
        assert_eq!(render_ok(&blocks), "x");
        let sum = format!("{{{{ 1{} }}}}", " + 1".repeat(super::MAX_OPERATORS - 1));
        assert_eq!(render_ok(&sum), super::MAX_OPERATORS.to_string());
        let conditions = format!(
            "{{% if true{} %}}y{{% endif %}}",
            " and true".repeat(super::MAX_OPERATORS - 1)
        );
        assert_eq!(render_ok(&conditions), "y");
        let nested = format!(
            "{{{{ {}1{} }}}}",
            "(".repeat(super::MAX_NESTING),
            ")".repeat(super::MAX_NESTING)
        );
        assert_eq!(render_ok(&nested), "1");
        // macros and blocks together, down to the render depth limit: the worst case a template can hold
        // four macros, four calls (two levels each) and four runs of `half` blocks fill the limit exactly
        let half = (super::MAX_RENDER_DEPTH - 8) / 4;
        let (open, close) = ("{% if true %}".repeat(half), "{% endif %}".repeat(half));
        let mut chain = String::new();
        for i in 0..3 {
            chain.push_str(&format!(
                "{{% macro m{i}() %}}{open}{{{{ self::m{}() }}}}{close}{{% endmacro m{i} %}}",
                i + 1
            ));
        }
        chain.push_str(&format!(
            "{{% macro m3() %}}{open}z{close}{{% endmacro m3 %}}{{{{ self::m0() }}}}"
        ));
        assert_eq!(render_ok(&chain), "z");
    }

    /// The scan reads tags like Tera: text, strings and comments are not tags, raw blocks are text.
    #[test]
    fn the_complexity_scan_ignores_what_is_not_code() {
        let braces = "{{ \"{% for %}\" }}";
        assert_eq!(render_ok(braces), "{% for %}");
        let long_string = format!("{{{{ \"{}\" | length }}}}", "+ and ( ".repeat(5_000));
        assert_eq!(render_ok(&long_string), "40000");
        assert_eq!(
            render_ok(&format!("{{# {} #}}ok", "{% for i in x %}".repeat(100))),
            "ok"
        );
        let raw = format!(
            "{{% raw %}}{}{{% endraw %}}done",
            "{% for i in [1] %}".repeat(100)
        );
        assert!(render_ok(&raw).ends_with("done"));
        // `android` and `order` are words, not `and` and `or`
        let words = format!(
            "{{% set android = 1 %}}{{{{ android{} }}}}",
            " + android".repeat(40)
        );
        assert_eq!(render_ok(&words), "41");
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
    fn template_collected_during_render_does_not_deadlock() {
        // Reproducer for NEOC-27. Lua's GC can finalize a `Template` handle
        // while a filter runs inside `render`, on the thread holding the
        // non-reentrant engine lock. `TemplateInner::drop` then re-locked
        // and hung forever. Run on a worker so a regression fails the test
        // instead of stalling the suite.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let lua = engine();
            // The sandbox strips `collectgarbage`; expose a full GC for the test.
            let gc = lua
                .create_function(|lua, ()| {
                    lua.gc_collect()?;
                    lua.gc_collect()
                })
                .unwrap();
            lua.globals().set("gc", gc).unwrap();
            lua.load(
                r#"
                t = template.compile("x")
                template.add_filter("collect", function(v, _)
                    t = nil
                    gc()
                    return v
                end)
                "#,
            )
            .exec()
            .unwrap();
            let (out, _): (Option<String>, Value) = lua
                .load(r#"return template.render("{{ x | collect }}", { x = "y" })"#)
                .eval()
                .unwrap();
            let _ = tx.send(out);
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("render deadlocked: Template dropped under the engine lock");
        assert_eq!(out.as_deref(), Some("y"));
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
