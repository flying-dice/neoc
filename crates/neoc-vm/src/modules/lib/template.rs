//! String templating: `lib:template` (the `tera` crate, 1.x).
//!
//! Jinja-style syntax (`{{ var }}`, `{% for %}`, `{% if %}`, `{%- -%}` whitespace control). `compile` parses a
//! template once and `Template:render` renders it many times; `render` does both for a template used once;
//! `add_filter` makes a Lua function a filter (`{{ name | shout }}`) of every later render in this VM.
//!
//! ## Limits and refusals
//!
//! Tera has no limits of its own and a deeply nested or recursive template overflows the native stack, which
//! aborts the process, so templates are bounded (the rationale and measurements are in the section comment
//! further down):
//!
//! - `include`, `import` and `extends` are refused (`unsupported`): a template cannot name another.
//! - At most 24 nested blocks (`for`, `if`, `macro`, `block`, `filter`), 48 operators and 24 nested brackets in
//!   one tag, and 48 levels of blocks and macro calls when rendered (a call is two levels plus the blocks
//!   around it, and a call inside another call's arguments counts one deeper); past that, `too_complex`.
//! - A macro may call itself, directly or through others. The depth is bounded at run time: what is left of
//!   the 48 levels after the cycle's own call overhead (a factorial whose call sits in one `if` gets 13
//!   frames, a tree walk with the call under an `if` and a `for` 9), and past that the render fails with
//!   `recursion_limit` (`macro recursion is more than N levels deep`).
//! - A bare macro call may not be an operand of arithmetic or of `<` `>` `<=` `>=` (Tera panics evaluating
//!   it); `set` its result first, or filter it (`self::m() | int + 1` is fine).
//! - `range()` yields at most 16 Mi elements and rejects a zero step.
//! - A panic inside Tera (`i64::MIN % -1` computed from a context value, say) is the render error
//!   `engine_panic`. That relies on `catch_unwind`, so it holds only under `panic = "unwind"` (neoc's profile):
//!   an embedder built with `panic = "abort"` still aborts on these panics, and a caught panic still prints the
//!   panic hook's message to stderr.
//!
//! ## Design decisions
//!
//! - **Strict variables.** A variable the context does not have is a render error; there is no opt-out.
//! - **Autoescape off by default.** Opt in per call with `template.render(src, ctx, { autoescape = true })`. A
//!   compiled template is plain. Tera chooses by the template's name, so a one-off render that escapes is
//!   registered under a name with the `__esc` suffix.
//! - **A filter's output is escaped under autoescape.** A template that wants it raw chains Tera's `| safe`.
//! - **One engine per VM.** Filters and compiled templates belong to the VM that made them (a `std:thread` worker
//!   has its own). The engine is behind a lock, and a `Template` may move between `std:task` coroutines.
//!
//! ## A filter may not render
//!
//! The engine's lock is held across a render and is not reentrant: a filter that called `template.render` or
//! `Template:render` would deadlock its thread. `IN_RENDER` is set for the length of a render, and a render inside
//! one fails with `recursive_render` instead.
//!
//! Ported from the old VM's `lib:template`. What changed on purpose:
//! - Errors are typed: `CompileError` (`syntax`, `too_complex`, `unsupported`), `RenderError` (those three for
//!   the one-shot `render`, and `failed`, `recursion_limit`, `recursive_render`, `engine_panic`) and `FilterError`
//!   (`empty_name`); the reason no longer starts with `template too complex: `, which is the kind.
//! - A context that is not a table of names to plain data, and an option that does not exist or is not a boolean,
//!   are refused as arguments with their place (they were returned errors).
//! - A filter is given `nil` for a null and plain tables for lists (no `null` sentinel, no array metatable).
//! - `add_filter`'s second parameter is named `filter` (it was `fn`).

use std::cell::Cell;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use mlua::Lua;
use mlua_ir::{Callback, Unknown};
use parking_lot::Mutex;
use tera::ast::{
    Expr, ExprVal, FunctionCall, If, LogicExpr, LogicOperator, MathExpr, MathOperator, Node, WS,
};
use tera::{Context, Filter as TeraFilter, Function as TeraFunction, Tera, Value as TeraValue};

use template::{CompileError, Data, RenderError, Variables};

/// Tera autoescapes by the suffix of a template's name. A one-off render that asked for `autoescape` is registered
/// under this one.
const ESCAPE_SUFFIX: &str = "__esc";
const ONE_OFF_PLAIN: &str = "__one_off";
const ONE_OFF_ESCAPED: &str = "__one_off.__esc";

thread_local! {
    static IN_RENDER: Cell<bool> = const { Cell::new(false) };
}

/// Held for the length of a render; a second one on the same thread is not given (see the module docs).
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
    fn arg(args: &HashMap<String, TeraValue>, key: &str, default: Option<usize>) -> tera::Result<usize> {
        match args.get(key) {
            Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| {
                tera::Error::msg(format!(
                    "Function `range` received {key}={v} but `{key}` can only be a number"
                ))
            }),
            None => default.ok_or_else(|| {
                tera::Error::msg(format!("Function `range` was called without a `{key}` argument"))
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
            return Err(tera::Error::msg("Function `range` was called with a `step_by` of 0"));
        }
        let len = (end - start).div_ceil(step_by);
        if len > MAX_RANGE_LEN {
            return Err(tera::Error::msg(format!(
                "Function `range` would produce {len} values (the most it produces is {MAX_RANGE_LEN})"
            )));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(len).map_err(|e| tera::Error::msg(format!("Function `range`: {e}")))?;
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
                let info = macros.entry(def.name.clone()).or_insert(MacroInfo { calls: Vec::new(), peak: 0 });
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
                self.calls.push(Call { name: m.name.clone(), depth: self.depth });
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
fn check_renderable(tera: &Tera, name: &str) -> Result<Plan, CompileError> {
    let tpl =
        tera.get_template(name).map_err(|e| CompileError::Syntax { reason: format_tera_error(&e).0 })?;
    let mut scan = Scan::default();
    let mut macros: HashMap<String, MacroInfo> = HashMap::new();
    scan.nodes(&tpl.ast, &mut macros);
    if scan.names_another_template {
        return Err(CompileError::Unsupported {
            reason: "`include`, `import` and `extends` are not supported: a template cannot name another"
                .into(),
        });
    }
    if scan.macro_call_operand {
        return Err(CompileError::Unsupported {
            reason: "a macro call cannot be an operand of arithmetic or of `<` `>` `<=` `>=`: \
                     assign its result with `set` first"
                .into(),
        });
    }
    plan_macros(scan.peak, &scan.calls, &macros).map_err(|reason| CompileError::TooComplex { reason })
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
        let recursive = members.len() > 1 || edges[members[0]].iter().any(|&(w, _)| w == members[0]);
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
                "macro `{}` recurses, and its blocks and calls leave no room for even one \
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
            "blocks and macro calls nest {top} deep when rendered (the most is {MAX_RENDER_DEPTH})"
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
        Err(tera::Error::msg(format!("macro recursion is more than {levels} levels deep")))
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
        ExprVal::String(_) | ExprVal::Int(_) | ExprVal::Float(_) | ExprVal::Bool(_) | ExprVal::Ident(_) => {}
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
                    return Err(format!("blocks are nested more than {MAX_BLOCK_DEPTH} deep"));
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
    bytes.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// The first word of a tag body (after spaces and a whitespace-control `-`).
fn first_word(bytes: &[u8], body: usize) -> &[u8] {
    let rest = bytes.get(body..).unwrap_or_default();
    let start = rest.iter().position(|b| !b.is_ascii_whitespace() && *b != b'-').unwrap_or(rest.len());
    let word = &rest[start..];
    let len = word.iter().position(|b| !(b.is_ascii_alphanumeric() || *b == b'_')).unwrap_or(word.len());
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
                    return Err(format!("more than {MAX_NESTING} parentheses or brackets nested in one tag"));
                }
            }
            b')' | b']' => nesting = nesting.saturating_sub(1),
            b'+' | b'*' | b'/' | b'%' | b'<' | b'>' => operators += 1,
            // `==` and `!=` are one operator each, a lone `=` (a `set` or a keyword argument) is none, and the
            // `=` of `<=` and `>=` belongs to the `<` or `>` already counted
            b'!' if bytes.get(i + 1) == Some(&b'=') => operators += 1,
            b'=' if bytes.get(i + 1) == Some(&b'=')
                && !matches!(i.checked_sub(1).map(|p| bytes[p]), Some(b'<' | b'>' | b'!' | b'=')) =>
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
    bytes[body..at].iter().rev().find(|b| !b.is_ascii_whitespace()).is_some_and(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b')' | b']' | b'"' | b'\'' | b'`' | b'.')
    })
}

fn too_many_operators() -> String {
    format!("more than {MAX_OPERATORS} operators in one tag")
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
// A script's function as a Tera filter.
// ---------------------------------------------------------------------------

/// A filter's function: the value being filtered and the filter's named arguments in, the filtered value out.
type FilterFn = Callback<fn(Unknown<Data>, Variables) -> Unknown<Data>>;

struct LuaFilter {
    name: String,
    callback: FilterFn,
}

impl TeraFilter for LuaFilter {
    fn filter(&self, value: &TeraValue, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        let args = args.iter().map(|(name, arg)| (name.clone(), Unknown::new(arg.clone()))).collect();
        // Tera can only fail the render: what the function raised, or returned that is not plain data, is its reason
        self.callback
            .call(Unknown::new(value.clone()), args)
            .map(|filtered| filtered.value)
            .map_err(|e| tera::Error::msg(format!("filter '{}' failed: {e}", self.name)))
    }
}

// ---------------------------------------------------------------------------
// A compiled template: a name in the engine, taken out again when the last handle to it goes.
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

/// This VM's engine, made the first time the module is used.
fn engine_of(lua: &Lua) -> Engine {
    if let Some(engine) = lua.app_data_ref::<Engine>() {
        return engine.clone();
    }
    let engine = Engine::new();
    lua.set_app_data(engine.clone());
    engine
}

/// Add `src` to the engine under `name`, checked and with its recursive macros bounded. A refused template is
/// taken out again.
fn add_template(tera: &mut Tera, name: &str, src: &str) -> Result<(), CompileError> {
    if let Err(e) = tera.add_raw_template(name, src) {
        // Tera keeps a template whose `extends` or `import` it could not resolve, and then fails every later
        // `add_raw_template` while building the inheritance chains. Kept, it can still be read: what Tera could
        // not resolve is what this module refuses anyway, and the error says so.
        let refused = match check_renderable(tera, name) {
            Err(unsupported @ CompileError::Unsupported { .. }) => unsupported,
            _ => CompileError::Syntax { reason: format_tera_error(&e).0 },
        };
        tera.templates.remove(name);
        return Err(refused);
    }
    match check_renderable(tera, name) {
        Ok(plan) => {
            bound_recursion(tera, name, &plan);
            Ok(())
        }
        Err(e) => {
            tera.templates.remove(name);
            Err(e)
        }
    }
}

/// Render, turning a panic inside Tera into an error. Tera panics on a few inputs no check can see coming (an
/// `i64::MIN % -1` computed from a context value, for one); a panic must not take the host with it.
fn render_caught(tera: &Tera, name: &str, ctx: &Context) -> Result<String, RenderError> {
    match catch_unwind(AssertUnwindSafe(|| tera.render(name, ctx))) {
        Ok(Ok(rendered)) => Ok(rendered),
        Ok(Err(e)) => Err(match format_tera_error(&e) {
            (reason, true) => RenderError::RecursionLimit { reason },
            (reason, false) => RenderError::Failed { reason },
        }),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(RenderError::EnginePanic { reason: reason.to_owned() })
        }
    }
}

fn build_context(context: Variables) -> Context {
    let mut ctx = Context::new();
    for (name, data) in context {
        ctx.insert(name, &data.value);
    }
    ctx
}

/// The whole chain of a Tera error (its own `Display` shows one layer: the parser's line and column and a failing
/// filter's reason are further down), and whether it is the recursion guard's.
fn format_tera_error(e: &tera::Error) -> (String, bool) {
    // the guard is an implementation detail: its error reads "macro recursion is more than N levels deep",
    // without Tera's "Function call '...' failed" wrapper naming it
    let guard = format!("Function call '{DEPTH_GUARD}' failed");
    let mut parts = vec![e.to_string()];
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    let total = parts.len();
    parts.retain(|part| *part != guard);
    (parts.join(": "), parts.len() != total)
}

// `add_filter`'s callback type is written in its signature, not behind an alias: the declarations take the
// callback's parameter names from there, and the code the macro writes around it names the type again.
#[allow(clippy::type_complexity)]
#[mlua_ir::module("lib:template")]
pub mod template {
    use std::collections::HashMap;
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{Callback, LuaError, Object, Options, Unknown, unknown_reason};
    use serde::{Deserialize, Serialize};

    use super::{
        LuaFilter, ONE_OFF_ESCAPED, ONE_OFF_PLAIN, RenderGuard, TemplateInner, add_template, build_context,
        check_complexity, engine_of, render_caught,
    };

    unknown_reason!(
        /// What a template is given to render, and what its filters pass on.
        Data = "plain data a script gives its own template (a variable, a filter's value or argument): its shape is the template's business"
    );

    /// Data by name: a template's variables, a filter's named arguments.
    pub type Variables = HashMap<String, Unknown<Data>>;

    /// Why a template was not compiled.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CompileError {
        /// The source is not a template: an unclosed tag or block, a word that is not the language's.
        #[error("{reason}")]
        Syntax {
            /// What the parser said, with the line and column.
            reason: String,
        },
        /// The template nests deeper than can be rendered safely: blocks, brackets or operators in one tag, or
        /// the macro calls it makes.
        #[error("{reason}")]
        TooComplex {
            /// Which limit, and what it is.
            reason: String,
        },
        /// The template uses what this module refuses: `include`, `import` or `extends`, or a macro call as an
        /// operand of arithmetic or of an ordering comparison.
        #[error("{reason}")]
        Unsupported {
            /// What is refused, and what to write instead.
            reason: String,
        },
    }

    /// Why a template was not rendered. `syntax`, `too_complex` and `unsupported` come from the one-shot `render`
    /// alone: a compiled template is past them.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum RenderError {
        /// The source is not a template.
        #[error("{reason}")]
        Syntax {
            /// What the parser said, with the line and column.
            reason: String,
        },
        /// The template nests deeper than can be rendered safely.
        #[error("{reason}")]
        TooComplex {
            /// Which limit, and what it is.
            reason: String,
        },
        /// The template uses what this module refuses.
        #[error("{reason}")]
        Unsupported {
            /// What is refused, and what to write instead.
            reason: String,
        },
        /// The render failed on its data: a variable the context does not have, a filter that does not exist or
        /// that failed, a value of the wrong type.
        #[error("{reason}")]
        Failed {
            /// What the engine said, down to the filter's own reason.
            reason: String,
        },
        /// A macro called itself deeper than the stack allows.
        #[error("{reason}")]
        RecursionLimit {
            /// How deep the macro may go.
            reason: String,
        },
        /// A filter rendered a template while one was being rendered.
        #[error("recursive render not supported: a filter must not call template.render or Template:render")]
        RecursiveRender,
        /// The engine panicked on this template and its data.
        #[error("the template engine failed on this template: {reason}")]
        EnginePanic {
            /// The panic's message.
            reason: String,
        },
    }

    impl From<CompileError> for RenderError {
        fn from(error: CompileError) -> Self {
            match error {
                CompileError::Syntax { reason } => RenderError::Syntax { reason },
                CompileError::TooComplex { reason } => RenderError::TooComplex { reason },
                CompileError::Unsupported { reason } => RenderError::Unsupported { reason },
            }
        }
    }

    /// Why a filter was not added.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum FilterError {
        /// The name is empty.
        #[error("filter name must be non-empty")]
        EmptyName,
    }

    /// How a one-shot render writes its values.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct RenderOptions {
        /// Escape HTML in every value written (`<` as `&lt;`), a filter's output included; `| safe` leaves one raw.
        pub autoescape: bool,
    }

    /// A compiled template. `tostring` names it.
    #[derive(Clone, Object)]
    pub struct Template {
        inner: Arc<TemplateInner>,
    }

    #[mlua_ir::object]
    impl Template {
        /// Render the template with `context` as its variables.
        ///
        /// ```luau
        /// local greeting = assert(template.compile("Hi {{ who }}"))
        /// assert(greeting:render({ who = "Skywarp" }) == "Hi Skywarp")
        /// ```
        pub fn render(&self, context: Variables) -> Result<String, RenderError> {
            let _rendering = RenderGuard::enter().ok_or(RenderError::RecursiveRender)?;
            let context = build_context(context);
            let tera = self.inner.engine.lock();
            render_caught(&tera, &self.inner.name, &context)
        }

        /// `Template(<its name in the engine>)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("Template({})", self.inner.name)
        }
    }

    /// Parse `source` once, for a template that is rendered many times.
    pub fn compile(lua: &Lua, source: String) -> Result<Template, CompileError> {
        check_complexity(&source).map_err(|reason| CompileError::TooComplex { reason })?;
        let engine = engine_of(lua);
        let name = engine.next_template_name();
        add_template(&mut engine.lock(), &name, &source)?;
        Ok(Template { inner: Arc::new(TemplateInner { engine, name }) })
    }

    /// Parse `source` and render it with `context` as its variables.
    ///
    /// ```luau
    /// assert(template.render("Hello, {{ name }}!", { name = "world" }) == "Hello, world!")
    /// assert(template.render("{{ s }}", { s = "<b>" }, { autoescape = true }) == "&lt;b&gt;")
    /// local _, err = template.render("{{ missing }}", {})
    /// assert(err ~= nil and err.kind == "failed")
    /// ```
    pub fn render(
        lua: &Lua,
        source: String,
        context: Variables,
        options: RenderOptions,
    ) -> Result<String, RenderError> {
        let _rendering = RenderGuard::enter().ok_or(RenderError::RecursiveRender)?;
        check_complexity(&source).map_err(|reason| CompileError::TooComplex { reason })?;
        let context = build_context(context);
        let name = if options.autoescape { ONE_OFF_ESCAPED } else { ONE_OFF_PLAIN };
        let engine = engine_of(lua);
        let mut tera = engine.lock();
        add_template(&mut tera, name, &source)?;
        render_caught(&tera, name, &context)
    }

    /// Make `filter` the filter `name` of every later render in this VM, a compiled template's included. It is
    /// given the value being filtered and the filter's named arguments (`{{ price | money(digits=2) }}`), and
    /// returns the value to write; what it raises fails the render.
    ///
    /// ```luau
    /// template.add_filter("shout", function(value)
    ///     return string.upper(tostring(value))
    /// end)
    /// assert(template.render("{{ s | shout }}", { s = "loud" }) == "LOUD")
    /// ```
    pub fn add_filter(
        lua: &Lua,
        name: String,
        filter: Callback<fn(value: Unknown<Data>, args: Variables) -> Unknown<Data>>,
    ) -> Result<(), FilterError> {
        if name.is_empty() {
            return Err(FilterError::EmptyName);
        }
        let engine = engine_of(lua);
        engine.lock().register_filter(&name, LuaFilter { name: name.clone(), callback: filter });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:template")"#).eval().unwrap();
        lua.globals().set("template", m).unwrap();
        lua
    }

    /// The kind and the text of the error `code` returns second, given `arg` as `...`.
    fn failure_in(lua: &Lua, code: &str, arg: &str) -> (String, String) {
        lua.load(format!(
            "local value, err = (function(...) {code} end)(...)
             assert(value == nil and err ~= nil, 'expected an error')
             return err.kind, tostring(err)"
        ))
        .call(arg)
        .unwrap_or_else(|e| panic!("{arg:.60}: {e}"))
    }

    fn render_failure(src: &str) -> (String, String) {
        failure_in(&engine(), "return template.render(..., {})", src)
    }

    fn render_err(src: &str) -> String {
        render_failure(src).1
    }

    fn render_ok(src: &str) -> String {
        let lua = engine();
        lua.load("return (assert(template.render(..., {})))")
            .call(src)
            .unwrap_or_else(|e| panic!("{src:.60}: {e}"))
    }

    /// `range(step_by=0)` looped forever growing a `Vec`; `range(end=1e18)` allocated until the allocator
    /// aborted.
    #[test]
    fn range_with_a_zero_step_or_an_absurd_length_is_an_error() {
        assert!(render_err("{% for i in range(end=10, step_by=0) %}{{ i }}{% endfor %}").contains("step_by"));
        assert!(
            render_err("{% for i in range(end=1000000000000000000) %}x{% endfor %}")
                .contains("the most it produces")
        );
        assert!(render_err("{% for i in range(end=10, step_by=-1) %}{% endfor %}").contains("step_by"));
        // and range still works
        assert_eq!(
            render_ok("{% for i in range(start=1, end=10, step_by=3) %}{{ i }},{% endfor %}"),
            "1,4,7,"
        );
    }

    /// Tera has no recursion limit: an unbounded macro overflowed the Rust stack (an abort). It is an ordinary
    /// render error now, on the 2 MiB stack of a test thread.
    #[test]
    fn unbounded_macro_recursion_and_self_including_templates_are_errors() {
        let direct = "{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}";
        let (kind, direct_err) = render_failure(direct);
        assert_eq!(kind, "recursion_limit");
        assert!(!direct_err.contains("neoc macro recursion limit"), "{direct_err}");
        assert!(direct_err.ends_with("macro recursion is more than 20 levels deep"), "{direct_err}");
        let mutual = "{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}x{{ self::a() }}{% endmacro b %}{{ self::a() }}";
        assert_eq!(render_failure(mutual).0, "recursion_limit");
        let nested = "{% macro f() %}{% if true %}{% for i in range(end=2) %}{{ self::f() | upper }}{% endfor %}{% endif %}{% endmacro f %}{{ self::f() }}";
        assert!(render_err(nested).contains("levels deep"));
        let (kind, text) = render_failure(r#"{% include "__one_off" %}"#);
        assert!(kind == "unsupported" && text.contains("not supported"), "{text}");
        for names_another in [r#"{% import "t0" as m %}"#, r#"{% extends "base" %}"#] {
            assert_eq!(render_failure(names_another).0, "unsupported", "{names_another}");
        }
        // a compiled template reports it when rendered, every time, and a plain macro still works
        let lua = engine();
        let (kind, text) = failure_in(
            &lua,
            "local t = assert(template.compile(...)) local a, b = t:render({}) return t:render({})",
            direct,
        );
        assert!(kind == "recursion_limit" && text.starts_with("lib:template.Template:render: "), "{text}");
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
        assert_eq!(out, "<ul><li>a<ul><li>b</li><li>c<ul><li>d</li></ul></li></ul></li></ul>");
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
            let (kind, e) = render_failure(&format!("{m}{shape}"));
            assert!(kind == "unsupported" && e.contains("cannot be an operand"), "{shape}: {e}");
        }
        assert_eq!(render_ok(&format!("{m}{{% if self::m() == \"1\" %}}a{{% endif %}}")), "a");
        assert_eq!(render_ok(&format!("{m}{{% set x = self::m() %}}{{{{ x | int + 1 }}}}")), "2");
        // a filtered call is evaluated first and does not panic: it renders
        assert_eq!(render_ok(&format!("{m}{{{{ self::m() | int + 1 }}}}")), "2");
        assert_eq!(render_ok(&format!("{m}{{% if self::m() | int > 0 %}}a{{% endif %}}")), "a");
    }

    /// `i64::MIN % -1` overflows inside Tera and panics; a value from the context cannot be seen by any check.
    /// It is a render error, not a host panic.
    #[test]
    fn a_panic_inside_the_renderer_is_a_render_error() {
        let lua = engine();
        let overflow = "{{ (0 - 9223372036854775807 - 1) % -1 }}";
        let (kind, text) = failure_in(&lua, "return template.render(..., {})", overflow);
        assert!(kind == "engine_panic" && text.contains("template engine failed"), "{text}");
        let (kind, text) =
            failure_in(&lua, "local t = assert(template.compile(...)) return t:render({})", overflow);
        assert!(kind == "engine_panic" && text.contains("template engine failed"), "{text}");
        // the engine still renders afterwards
        let ok: String = lua.load("return template.render('{{ 1 + 1 }}', {})").eval().unwrap();
        assert_eq!(ok, "2");
    }

    /// Tera recurses over the syntax tree with no limit. Each of these overflowed a 2 MiB stack (the default of
    /// a test thread and of a runtime worker) and aborted the host.
    #[test]
    fn templates_that_nest_or_chain_too_deeply_are_refused_not_overflowed() {
        let cases = [
            format!("{}x{}", "{% for i in [1] %}".repeat(500), "{% endfor %}".repeat(500)),
            format!("{}x{}", "{% if true %}".repeat(500), "{% endif %}".repeat(500)),
            format!("{{{{ 1{} }}}}", " + 1".repeat(20_000)),
            format!("{{% if true{} %}}x{{% endif %}}", " and true".repeat(20_000)),
            format!("{{{{ {}1{} }}}}", "(".repeat(5_000), ")".repeat(5_000)),
            format!("{{{{ {}1{} }}}}", "[".repeat(5_000), "]".repeat(5_000)),
            // an unterminated one is Tera's to report, but it must not recurse either
            "{{ 1".to_owned() + &" + 1".repeat(20_000),
        ];
        for src in &cases {
            let (kind, e) = render_failure(src);
            assert_eq!(kind, "too_complex", "{src:.60}: {e}");
            let (kind, e) = failure_in(&engine(), "return template.compile(...)", src);
            assert_eq!(kind, "too_complex", "compile {src:.60}: {e}");
        }
    }

    /// A chain of macros, none recursive, each calling the next: the render nests as deep as the chain is long.
    #[test]
    fn a_long_chain_of_macros_is_refused() {
        let mut src = String::new();
        for i in 0..1000 {
            src.push_str(&format!("{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}", i + 1));
        }
        src.push_str("{% macro m1000() %}x{% endmacro m1000 %}{{ self::m0() }}");
        let (kind, text) = render_failure(&src);
        assert!(kind == "too_complex" && text.contains("nest"), "refused for its depth: {text}");
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
        let conditions =
            format!("{{% if true{} %}}y{{% endif %}}", " and true".repeat(super::MAX_OPERATORS - 1));
        assert_eq!(render_ok(&conditions), "y");
        let nested =
            format!("{{{{ {}1{} }}}}", "(".repeat(super::MAX_NESTING), ")".repeat(super::MAX_NESTING));
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
        chain.push_str(&format!("{{% macro m3() %}}{open}z{close}{{% endmacro m3 %}}{{{{ self::m0() }}}}"));
        assert_eq!(render_ok(&chain), "z");
    }

    /// The scan reads tags like Tera: text, strings and comments are not tags, raw blocks are text.
    #[test]
    fn the_complexity_scan_ignores_what_is_not_code() {
        let braces = "{{ \"{% for %}\" }}";
        assert_eq!(render_ok(braces), "{% for %}");
        let long_string = format!("{{{{ \"{}\" | length }}}}", "+ and ( ".repeat(5_000));
        assert_eq!(render_ok(&long_string), "40000");
        assert_eq!(render_ok(&format!("{{# {} #}}ok", "{% for i in x %}".repeat(100))), "ok");
        let raw = format!("{{% raw %}}{}{{% endraw %}}done", "{% for i in [1] %}".repeat(100));
        assert!(render_ok(&raw).ends_with("done"));
        // `android` and `order` are words, not `and` and `or`
        let words = format!("{{% set android = 1 %}}{{{{ android{} }}}}", " + android".repeat(40));
        assert_eq!(render_ok(&words), "41");
    }

    #[test]
    fn flat_render_substitutes_variables() {
        let lua = engine();
        let (out, e): (String, Value) =
            lua.load(r#"return template.render("Hello, {{ name }}!", { name = "world" })"#).eval().unwrap();
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
        let name: String = lua.load(r#"return tostring(template.compile("x"))"#).eval().unwrap();
        assert!(name.starts_with("Template("), "got: {name}");
    }

    #[test]
    fn flat_render_loops_conditionals_and_built_in_filters() {
        let lua = engine();
        let out: String = lua
            .load(r#"return template.render("{% for n in nums %}{{ n }},{% endfor %}", { nums = { 1, 2, 3 } })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "1,2,3,");
        let out: String = lua
            .load(r#"return template.render("{% if flag %}YES{% else %}NO{% endif %}", { flag = true })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "YES");
        let out: String =
            lua.load(r#"return template.render("{{ s | upper }}", { s = "shout" })"#).eval().unwrap();
        assert_eq!(out, "SHOUT");
    }

    #[test]
    fn missing_variable_is_strict_error() {
        let lua = engine();
        let (kind, e) = failure_in(&lua, r#"return template.render("{{ missing }}", { other = 1 })"#, "");
        assert_eq!(kind, "failed");
        assert!(e.starts_with("lib:template.render: failed: "), "got: {e}");
        assert!(e.contains("missing"), "got: {e}");
    }

    #[test]
    fn parse_error_is_reported_with_line_info() {
        let lua = engine();
        let (kind, e) = failure_in(&lua, r#"return template.render("{% for x in xs %}", {})"#, "");
        assert_eq!(kind, "syntax");
        assert!(e.contains("--> 1:"), "the parser's place: {e}");
    }

    #[test]
    fn invalid_syntax_on_compile_returns_error_tuple() {
        let lua = engine();
        let (kind, e) = failure_in(&lua, r#"return template.compile("{% if %}")"#, "");
        assert_eq!(kind, "syntax");
        assert!(e.starts_with("lib:template.compile: syntax: "), "got: {e}");
        // a refused source leaves nothing behind in the engine
        let engine = super::engine_of(&lua);
        assert!(engine.tera.lock().templates.is_empty());
    }

    #[test]
    fn autoescape_is_off_by_default_and_opt_in() {
        let lua = engine();
        let out: String = lua.load(r#"return template.render("{{ s }}", { s = "<b>" })"#).eval().unwrap();
        assert_eq!(out, "<b>");
        let out: String = lua
            .load(r#"return template.render("{{ s }}", { s = "<b>" }, { autoescape = true })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "&lt;b&gt;");
        let out: String = lua
            .load(r#"return template.render("{{ s }}", { s = "<b>" }, { autoescape = false })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "<b>");
    }

    #[test]
    fn whitespace_control_is_preserved() {
        let lua = engine();
        let out: String = lua
            .load(r#"return template.render("A {%- if x %} B {%- endif %}", { x = true })"#)
            .eval()
            .unwrap();
        assert!(out.contains('B'), "got: {out}");
        assert!(!out.contains('\n'), "got: {out}");
        let out: String =
            lua.load(r#"return template.render("A{%- if x %}B{%- endif %}", { x = true })"#).eval().unwrap();
        assert_eq!(out, "AB");
    }

    #[test]
    fn custom_filter_visible_after_registration() {
        let lua = engine();
        lua.load(
            r#"
            assert(template.add_filter("shout", function(value, _args)
                return string.upper(tostring(value))
            end) == true)
            "#,
        )
        .exec()
        .unwrap();
        let out: String =
            lua.load(r#"return template.render("{{ s | shout }}", { s = "loud" })"#).eval().unwrap();
        assert_eq!(out, "LOUD");
    }

    #[test]
    fn a_filter_is_given_its_named_arguments_and_plain_data() {
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("pad", function(value, args)
                return string.rep(args.with, args.times) .. value.name .. #value.tags
            end)
            "#,
        )
        .exec()
        .unwrap();
        let out: String = lua
            .load(r#"return template.render("{{ v | pad(with='-', times=3) }}", { v = { name = "x", tags = { "a", "b" } } })"#)
            .eval()
            .unwrap();
        assert_eq!(out, "---x2");
    }

    #[test]
    fn custom_filter_seen_by_pre_existing_compiled_template() {
        // Filters live on the engine, not frozen at compile time. A template
        // compiled before the filter exists must see it on later renders.
        let lua = engine();
        lua.load(r#"t = template.compile("{{ s | shout }}")"#).exec().unwrap();
        // Render before registering should fail (filter unknown).
        let (kind, e) = failure_in(&lua, r#"return t:render({ s = "loud" })"#, "");
        assert!(kind == "failed" && e.contains("shout"), "{e}");

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
        let out: String = lua.load(r#"return t:render({ s = "loud" })"#).eval().unwrap();
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
            template.add_filter("bytes", function(_, _)
                return buffer.create(2)
            end)
            "#,
        )
        .exec()
        .unwrap();
        let (kind, e) = failure_in(&lua, r#"return template.render("{{ x | kaboom }}", { x = 1 })"#, "");
        assert_eq!(kind, "failed");
        assert!(e.starts_with("lib:template.render: failed: "), "got: {e}");
        assert!(e.contains("kaboom"), "got: {e}");
        assert!(e.contains("filter exploded"), "got: {e}");
        // what a filter returns is plain data: a buffer fails the render, it is not written as numbers
        let (kind, e) = failure_in(&lua, r#"return template.render("{{ x | bytes }}", { x = 1 })"#, "");
        assert!(kind == "failed" && e.contains("filter 'bytes' failed"), "got: {e}");
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
            .load(r#"return template.render("{{ s | wrap }}", { s = "x" }, { autoescape = true })"#)
            .eval()
            .unwrap();
        // Tera escapes `/` as `&#x2F;` (HTML-safe variant).
        assert_eq!(out, "&lt;b&gt;x&lt;&#x2F;b&gt;");
    }

    #[test]
    fn add_filter_empty_name_rejected() {
        let lua = engine();
        let (kind, e) = failure_in(&lua, r#"return template.add_filter("", function() return "" end)"#, "");
        assert_eq!(kind, "empty_name");
        assert!(e.contains("non-empty"), "got: {e}");
    }

    /// What was a returned error in the old VM is a refused argument, with its place.
    #[test]
    fn arguments_of_the_wrong_shape_are_refused() {
        let lua = engine();
        for (code, place) in [
            (r#"return template.render("{{ x }}", 42)"#, "lib:template.render: argument #2 'context'"),
            (r#"return template.render("{{ x }}", { 1, 2 })"#, "lib:template.render: argument #2 'context'"),
            (
                r#"return template.render("x", { b = buffer.create(2) })"#,
                "lib:template.render: argument #2 'context'",
            ),
            (r#"return template.render("x", { deep = { b = buffer.create(2) } })"#, "argument #2 'context'"),
            (
                r#"return template.render("x", {}, { autoescape = "yes" })"#,
                "lib:template.render: argument #3 'options'",
            ),
            (
                r#"return template.render("x", {}, { escape = true })"#,
                "lib:template.render: argument #3 'options'",
            ),
            (
                r#"return template.compile("x"):render(42)"#,
                "lib:template.Template:render: argument #1 'context'",
            ),
            (r#"return template.compile("x"):render({ b = buffer.create(2) })"#, "argument #1 'context'"),
            (r#"return template.add_filter("f", "upper")"#, "lib:template.add_filter: argument #2 'filter'"),
        ] {
            let err = lua.load(code).eval::<Value>().expect_err(code).to_string();
            assert!(err.contains(place), "{code}: {err}");
        }
        // an empty table is a context with no variables
        let out: String = lua.load(r#"return template.render("x", {})"#).eval().unwrap();
        assert_eq!(out, "x");
    }

    #[test]
    fn template_handle_is_send_for_task_coroutines() {
        fn assert_send<T: Send>() {}
        assert_send::<super::template::Template>();
    }

    #[test]
    fn filter_recursive_render_rejected_not_deadlocked() {
        // The tera engine lock is non-reentrant, so a filter that calls `template.render` from inside an outer
        // render must be rejected explicitly rather than blocking the thread forever.
        let lua = engine();
        lua.load(
            r#"
            template.add_filter("recurse", function(_, _)
                local out, e = template.render("inner:{{ s }}", { s = "x" })
                if not out then
                    assert(e.kind == "recursive_render")
                    error(tostring(e))
                end
                return out
            end)
            "#,
        )
        .exec()
        .unwrap();
        let (kind, e) = failure_in(&lua, r#"return template.render("{{ x | recurse }}", { x = "y" })"#, "");
        assert_eq!(kind, "failed");
        assert!(e.contains("recursive render"), "got: {e}");
        assert!(e.contains("recurse"), "got: {e}"); // filter name surfaces
        // and the guard is released: the next render is not refused
        let out: String = lua.load(r#"return template.render("{{ 1 }}", {})"#).eval().unwrap();
        assert_eq!(out, "1");
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
                if not out then error(tostring(e)) end
                return out
            end)
            outer = template.compile("{{ x | recurse2 }}")
            "#,
        )
        .exec()
        .unwrap();
        let (_, e) = failure_in(&lua, r#"return outer:render({ x = "y" })"#, "");
        assert!(e.contains("recursive render"), "got: {e}");
    }

    #[test]
    fn template_collected_during_render_does_not_deadlock() {
        // Lua's GC can finalize a `Template` handle while a filter runs inside `render`, on the thread holding the
        // non-reentrant engine lock. `TemplateInner::drop` then re-locked and hung forever. Run on a worker so a
        // regression fails the test instead of stalling the suite.
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
            let (out, _): (Option<String>, Value) =
                lua.load(r#"return template.render("{{ x | collect }}", { x = "y" })"#).eval().unwrap();
            let _ = tx.send(out);
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("render deadlocked: Template dropped under the engine lock");
        assert_eq!(out.as_deref(), Some("y"));
    }

    fn handle(engine: &super::Engine, name: &str) -> std::sync::Arc<super::TemplateInner> {
        std::sync::Arc::new(super::TemplateInner { engine: engine.clone(), name: name.to_owned() })
    }

    #[test]
    fn dropping_template_handle_removes_engine_entry() {
        // The tera engine's `templates` map grew unbounded when nothing removed entries as the userdata went away.
        let engine = super::Engine::new();
        let baseline = engine.tera.lock().templates.len();
        engine.tera.lock().add_raw_template("drop_probe", "hello {{ x }}").unwrap();
        let handle = handle(&engine, "drop_probe");
        assert_eq!(engine.tera.lock().templates.len(), baseline + 1);
        drop(handle);
        assert_eq!(
            engine.tera.lock().templates.len(),
            baseline,
            "templates map must shrink back to baseline once the last handle is dropped"
        );
    }

    #[test]
    fn cloned_template_handle_keeps_entry_until_last_drop() {
        // Removing the entry waits for the last clone: a premature removal would turn a second `:render` on a
        // clone into a missing-template error.
        let engine = super::Engine::new();
        let baseline = engine.tera.lock().templates.len();
        engine.tera.lock().add_raw_template("clone_probe", "x={{ x }}").unwrap();
        let h1 = handle(&engine, "clone_probe");
        let h2 = h1.clone();
        drop(h1);
        assert_eq!(engine.tera.lock().templates.len(), baseline + 1);
        drop(h2);
        assert_eq!(engine.tera.lock().templates.len(), baseline);
    }

    /// Every VM has its own engine: a filter added in one is not a filter of another.
    #[test]
    fn filters_belong_to_their_vm() {
        let (one, two) = (engine(), engine());
        one.load(r#"template.add_filter("mine", function(v) return v end)"#).exec().unwrap();
        let out: String = one.load(r#"return template.render("{{ 1 | mine }}", {})"#).eval().unwrap();
        assert_eq!(out, "1");
        let (kind, _) = failure_in(&two, r#"return template.render("{{ 1 | mine }}", {})"#, "");
        assert_eq!(kind, "failed");
    }

    // -- from the old VM's `tests/hostile_inputs.rs`: a stack overflow aborts the test binary, so reaching the
    // -- assertion is the proof

    /// The kind and text of the error rendering `source` gives on a 2 MiB thread.
    fn failure_on_small_stack(source: String) -> (String, String) {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || render_failure(&source))
            .unwrap()
            .join()
            .expect("the call panicked")
    }

    /// A chain of twenty thousand macros, none recursive, once overflowed the guard's own walk (it recursed once
    /// per macro), and a cycle check that restarted at every macro took minutes.
    #[test]
    fn a_chain_of_twenty_thousand_macros_is_refused_without_overflowing() {
        let mut src = String::new();
        for i in 0..20_000 {
            src.push_str(&format!("{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}", i + 1));
        }
        src.push_str("{% macro m20000() %}end{% endmacro m20000 %}{{ self::m0() }}");
        let started = std::time::Instant::now();
        let (kind, err) = failure_on_small_stack(src);
        assert_eq!(kind, "too_complex", "{err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(60), "took {:?}", started.elapsed());
    }

    /// Tera evaluates the arguments of a macro call before it enters the macro, but a call inside an argument is
    /// still nested in the call around it: eight levels of arguments in each of 23 macros went through the guard as
    /// siblings and aborted a debug build on 2 MiB.
    #[test]
    fn macro_calls_nested_in_arguments_are_counted_as_nested() {
        let nested = |callee: usize| format!("{}1{}", format!("self::m{callee}(x=").repeat(8), ")".repeat(8));
        let mut src = String::new();
        for i in 0..23 {
            src.push_str(&format!("{{% macro m{i}(x) %}}{{{{ {} }}}}{{% endmacro m{i} %}}", nested(i + 1)));
        }
        src.push_str("{% macro m23(x) %}{{ nope }}{% endmacro m23 %}{{ self::m0(x=1) }}");
        assert_eq!(failure_on_small_stack(src).0, "too_complex");
    }

    /// A macro that never stops calling itself, directly, through blocks, through another macro and through
    /// argument nesting, ends in an ordinary render error on 2 MiB.
    #[test]
    fn unbounded_macro_recursion_is_a_render_error_on_a_small_stack() {
        for src in [
            "{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}",
            "{% macro f(n) %}{% for i in [1] %}{% if true %}{% for j in [1] %}{{ self::f(n=n) }}{% endfor %}{% endif %}{% endfor %}{% endmacro f %}{{ self::f(n=1) }}",
            "{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}{{ self::a() }}{% endmacro b %}{{ self::a() }}",
            "{% macro f(x) %}{{ self::f(x=self::f(x=self::f(x=1))) }}{% endmacro f %}{{ self::f(x=1) }}",
        ] {
            let (kind, err) = failure_on_small_stack(src.to_owned());
            assert!(matches!(kind.as_str(), "recursion_limit" | "too_complex"), "{src:.50}: {err}");
        }
    }

    /// One large call cycle: a ring of sixty thousand macros. The cost of an edge must not grow with the size of
    /// the cycle (a membership scan per edge took 12.6 s in debug); the recursion is bounded at run time, so the
    /// render is an ordinary error.
    #[test]
    #[ignore = "slow: a wall-clock bound over a 60 000-macro ring, seconds in a debug build and flaky under load"]
    fn a_ring_of_sixty_thousand_macros_is_planned_in_linear_time() {
        let n = 60_000;
        let mut src = String::new();
        for i in 0..n {
            src.push_str(&format!(
                "{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}",
                (i + 1) % n
            ));
        }
        src.push_str("{{ self::m0() }}");
        let started = std::time::Instant::now();
        let (kind, err) = failure_on_small_stack(src);
        assert_eq!(kind, "recursion_limit", "{err:.200}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "took {:?}", started.elapsed());
    }
}
