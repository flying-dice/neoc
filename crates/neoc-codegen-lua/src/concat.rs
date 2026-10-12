//! Loop-carried string append lowering (`lang/DYNAMIC.md` §4.11).
//!
//! Lua interns every string, so `s = s .. x` in a loop hashes and copies the
//! whole intermediate result on each pass. When a loop only ever *appends* to a
//! local string, the appends go to an accumulator table and one `table.concat`
//! materialises the string after the loop. The rewrite is purely an
//! optimisation: it fires only where it provably cannot be observed, and every
//! other shape keeps the plain `..` lowering.
//!
//! The decision is made when a `let s = …` is emitted ([`Emitter::plan_concat`]):
//! the rest of the declaring block is scanned, and each *outermost* loop in it
//! that mentions `s` solely as an append target is recorded. When the emitter
//! reaches a recorded loop it wraps it ([`Emitter::emit_accumulating`]) and, for
//! the duration, lowers appends to `s` as accumulator pushes
//! ([`Emitter::emit_accumulated_append`]).

use crate::Emitter;
use neoc_ast as ast;
use std::collections::HashSet;

/// Where a node sits relative to the variable being analysed.
#[derive(Clone, Copy)]
struct Site {
    /// Inside a closure or nested function: a separate Lua function that can
    /// capture the variable.
    closure: bool,
    /// Inside a block, `if`, or `match` used for its value. That lowers to an
    /// IIFE, where a `return` (or the one hoisted by `?`) leaves only the
    /// wrapper — it would skip a fold placed after a loop in it.
    iife: bool,
}

const TOP: Site = Site {
    closure: false,
    iife: false,
};
const CLOSURE: Site = Site {
    closure: true,
    iife: false,
};

/// Callbacks for [`walk_expr`]. The walk is exhaustive over the AST so a new
/// expression form cannot silently escape the analysis.
trait Visitor<'a> {
    /// Visit an expression before its children; return `false` to skip them.
    fn expr(&mut self, e: &'a ast::Expr, site: Site) -> bool;
    /// A single-segment value reference to `name` (a path, or a struct-literal
    /// field shorthand).
    fn use_name(&mut self, name: &'a str, site: Site);
    /// A name bound by a pattern, closure parameter, or nested function.
    fn bind(&mut self, name: &'a str);
}

/// Walk a block. `valued` says whether its trailing expression is the block's
/// value (a function body, a valued block) or only run for effect.
fn walk_block<'a>(b: &'a ast::Block, site: Site, valued: bool, v: &mut impl Visitor<'a>) {
    for stmt in &b.stmts {
        walk_stmt(stmt, site, v);
    }
    match &b.value {
        Some(value) if valued => walk_expr(value, site, v),
        Some(value) => walk_effect(value, site, v),
        None => {}
    }
}

fn walk_stmt<'a>(stmt: &'a ast::Stmt, site: Site, v: &mut impl Visitor<'a>) {
    match &stmt.kind {
        ast::StmtKind::Let(l) => {
            walk_pattern(&l.pattern, v);
            if let Some(value) = &l.value {
                walk_expr(value, site, v);
            }
        }
        ast::StmtKind::Expr(e) => walk_effect(e, site, v),
        ast::StmtKind::Item(item) => {
            // Only nested functions lower (`emit_hoisted_fns`); each is a
            // separate Lua function.
            if let ast::ItemKind::Fn(f) = &item.kind {
                v.bind(&f.sig.name);
                for p in &f.sig.params {
                    if let ast::Param::Named { name, .. } = p {
                        v.bind(name);
                    }
                }
                walk_block(&f.body, CLOSURE, true, v);
            }
        }
    }
}

fn walk_pattern<'a>(p: &'a ast::Pattern, v: &mut impl Visitor<'a>) {
    match p {
        ast::Pattern::Binding { name, .. } => v.bind(name),
        ast::Pattern::Struct { fields, .. } => {
            for f in fields {
                match &f.pattern {
                    Some(inner) => walk_pattern(inner, v),
                    None => v.bind(&f.name),
                }
            }
        }
        ast::Pattern::Tuple { elems, .. } => {
            for e in elems {
                walk_pattern(e, v);
            }
        }
        ast::Pattern::Or(alts) => {
            for a in alts {
                walk_pattern(a, v);
            }
        }
        ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::Path(_) => {}
    }
}

/// Walk an expression used for its value.
fn walk_expr<'a>(e: &'a ast::Expr, site: Site, v: &mut impl Visitor<'a>) {
    if !v.expr(e, site) {
        return;
    }
    match &e.kind {
        ast::ExprKind::Path(segs) => {
            if let [only] = segs.as_slice() {
                v.use_name(only, site);
            }
        }
        ast::ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, site, v);
            walk_expr(rhs, site, v);
        }
        ast::ExprKind::Unary { operand, .. } | ast::ExprKind::Try(operand) => {
            walk_expr(operand, site, v);
        }
        ast::ExprKind::Range { start, end, .. } => {
            walk_expr(start, site, v);
            walk_expr(end, site, v);
        }
        ast::ExprKind::Call { callee, args } => {
            walk_expr(callee, site, v);
            for a in args {
                walk_expr(a, site, v);
            }
        }
        ast::ExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(receiver, site, v);
            for a in args {
                walk_expr(a, site, v);
            }
        }
        ast::ExprKind::Field { base, .. } => walk_expr(base, site, v),
        ast::ExprKind::Index { base, index } => {
            walk_expr(base, site, v);
            walk_expr(index, site, v);
        }
        ast::ExprKind::StructLit { fields, base, .. } => {
            for f in fields {
                match &f.value {
                    Some(value) => walk_expr(value, site, v),
                    None => v.use_name(&f.name, site),
                }
            }
            if let Some(base) = base {
                walk_expr(base, site, v);
            }
        }
        ast::ExprKind::List(items) => {
            for i in items {
                walk_expr(i, site, v);
            }
        }
        ast::ExprKind::Assign { target, value, .. } => {
            walk_expr(target, site, v);
            walk_expr(value, site, v);
        }
        ast::ExprKind::If { .. }
        | ast::ExprKind::Match { .. }
        | ast::ExprKind::While { .. }
        | ast::ExprKind::For { .. }
        | ast::ExprKind::Loop { .. }
        | ast::ExprKind::Closure { .. }
        | ast::ExprKind::Block(_)
        | ast::ExprKind::Return(_) => walk_control(e, site, true, v),
        ast::ExprKind::Int(_)
        | ast::ExprKind::Float(_)
        | ast::ExprKind::Str(_)
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Unit
        | ast::ExprKind::Break
        | ast::ExprKind::Continue => {}
    }
}

/// Walk an expression used for effect (a statement, a loop body's or
/// statement block's trailing expression). Block-like forms here lower as Lua
/// statements, not IIFEs.
fn walk_effect<'a>(e: &'a ast::Expr, site: Site, v: &mut impl Visitor<'a>) {
    let block_like = matches!(
        e.kind,
        ast::ExprKind::If { .. } | ast::ExprKind::Match { .. } | ast::ExprKind::Block(_)
    );
    if !block_like {
        walk_expr(e, site, v);
    } else if v.expr(e, site) {
        walk_control(e, site, false, v);
    }
}

/// The control-flow and scoping forms: the ones that own blocks, bind names,
/// or open a closure. `valued` says whether `e` is used for its value; a valued
/// block, `if`, or `match` is an IIFE for everything it contains.
fn walk_control<'a>(e: &'a ast::Expr, site: Site, valued: bool, v: &mut impl Visitor<'a>) {
    let inner = Site {
        iife: site.iife || valued,
        ..site
    };
    let branch = |body: &'a ast::Expr, v: &mut _| {
        if valued {
            walk_expr(body, inner, v);
        } else {
            walk_effect(body, inner, v);
        }
    };
    match &e.kind {
        ast::ExprKind::If { cond, then, else_ } => {
            walk_expr(cond, inner, v);
            walk_block(then, inner, valued, v);
            if let Some(else_) = else_ {
                branch(else_, v);
            }
        }
        ast::ExprKind::Match { scrutinee, arms } => {
            walk_expr(scrutinee, site, v);
            for arm in arms {
                walk_pattern(&arm.pattern, v);
                if let Some(guard) = &arm.guard {
                    walk_expr(guard, inner, v);
                }
                branch(&arm.body, v);
            }
        }
        ast::ExprKind::While { cond, body } => {
            walk_expr(cond, site, v);
            walk_block(body, site, false, v);
        }
        ast::ExprKind::For {
            pattern,
            iter,
            body,
        } => {
            walk_pattern(pattern, v);
            walk_expr(iter, site, v);
            walk_block(body, site, false, v);
        }
        ast::ExprKind::Loop { body } => walk_block(body, site, false, v),
        ast::ExprKind::Closure { params, body } => {
            for p in params {
                v.bind(&p.name);
            }
            walk_expr(body, CLOSURE, v);
        }
        ast::ExprKind::Block(b) => walk_block(b, inner, valued, v),
        ast::ExprKind::Return(Some(value)) => walk_expr(value, site, v),
        _ => {}
    }
}

fn is_name(e: &ast::Expr, name: &str) -> bool {
    matches!(&e.kind, ast::ExprKind::Path(segs) if segs.len() == 1 && segs[0] == name)
}

/// The appended parts of `name = name + p₁ + … + pₙ`, in evaluation order: the
/// value is a left-nested chain of string concatenations whose leftmost operand
/// is `name`. `None` for any other shape (`name = p + name` prepends, `name =
/// name` and `name = other` reassign).
fn append_parts<'a>(value: &'a ast::Expr, name: &str) -> Option<Vec<&'a ast::Expr>> {
    fn spine<'a>(e: &'a ast::Expr, name: &str, out: &mut Vec<&'a ast::Expr>) -> bool {
        match &e.kind {
            ast::ExprKind::Binary {
                op: ast::BinOp::Concat,
                lhs,
                rhs,
            } => {
                let leads = spine(lhs, name, out);
                if leads {
                    out.push(rhs);
                }
                leads
            }
            _ => is_name(e, name),
        }
    }
    let mut parts = Vec::new();
    let is_append = matches!(
        &value.kind,
        ast::ExprKind::Binary {
            op: ast::BinOp::Concat,
            ..
        }
    ) && spine(value, name, &mut parts);
    is_append.then_some(parts)
}

/// The parts appended by `target op= value` when it is an append to `name`:
/// `name = name + p₁ + … + pₙ`, or the elaborated string `name += value`.
fn append_to<'a>(
    target: &ast::Expr,
    op: ast::AssignOp,
    value: &'a ast::Expr,
    name: &str,
) -> Option<Vec<&'a ast::Expr>> {
    if !is_name(target, name) {
        return None;
    }
    match op {
        ast::AssignOp::Assign => append_parts(value, name),
        ast::AssignOp::Concat => Some(vec![value]),
        _ => None,
    }
}

/// Tallies how a region uses one variable.
struct NameScan<'n> {
    name: &'n str,
    /// References that are not an append target (reads, other writes).
    plain: usize,
    /// Appends outside any closure.
    appends: usize,
    /// References of any kind from inside a closure or nested function.
    captured: usize,
    /// Pattern, parameter, or nested-function bindings of the same name.
    shadows: usize,
}

impl<'n> NameScan<'n> {
    fn new(name: &'n str) -> Self {
        Self {
            name,
            plain: 0,
            appends: 0,
            captured: 0,
            shadows: 0,
        }
    }
}

impl<'a> Visitor<'a> for NameScan<'_> {
    fn expr(&mut self, e: &'a ast::Expr, site: Site) -> bool {
        // `return name` is the one read a loop may make: the accumulator can
        // fold itself into the returned value, and the function is leaving.
        if let ast::ExprKind::Return(Some(value)) = &e.kind
            && is_name(value, self.name)
        {
            if site.closure {
                self.captured += 1;
            }
            return false;
        }
        let ast::ExprKind::Assign { target, op, value } = &e.kind else {
            return true;
        };
        let Some(parts) = append_to(target, *op, value, self.name) else {
            return true;
        };
        if site.closure {
            self.captured += 1;
        } else {
            self.appends += 1;
        }
        for part in parts {
            walk_expr(part, site, self);
        }
        false
    }

    fn use_name(&mut self, name: &'a str, site: Site) {
        if name != self.name {
            return;
        }
        if site.closure {
            self.captured += 1;
        } else {
            self.plain += 1;
        }
    }

    fn bind(&mut self, name: &'a str) {
        if name == self.name {
            self.shadows += 1;
        }
    }
}

/// Collects the names assigned by an append anywhere in a region.
#[derive(Default)]
struct AppendTargets<'a> {
    names: HashSet<&'a str>,
}

impl<'a> Visitor<'a> for AppendTargets<'a> {
    fn expr(&mut self, e: &'a ast::Expr, _site: Site) -> bool {
        if let ast::ExprKind::Assign { target, op, value } = &e.kind
            && let ast::ExprKind::Path(segs) = &target.kind
            && let [name] = segs.as_slice()
            && append_to(target, *op, value, name).is_some()
        {
            self.names.insert(name);
        }
        true
    }

    fn use_name(&mut self, _name: &'a str, _site: Site) {}

    fn bind(&mut self, _name: &'a str) {}
}

/// Finds the outermost loops of a region that use `name` only as an append
/// target. Loops inside closures are skipped: they run in another function.
/// Loops inside an IIFE are skipped too: a `return` or `?` there leaves only
/// the IIFE, past the fold (`lang/DYNAMIC.md` §4.11, condition 5).
struct LoopFinder<'n> {
    name: &'n str,
    found: Vec<usize>,
}

impl<'a> Visitor<'a> for LoopFinder<'_> {
    fn expr(&mut self, e: &'a ast::Expr, site: Site) -> bool {
        let is_loop = matches!(
            e.kind,
            ast::ExprKind::While { .. } | ast::ExprKind::For { .. } | ast::ExprKind::Loop { .. }
        );
        if !is_loop || site.closure || site.iife {
            return true;
        }
        let mut scan = NameScan::new(self.name);
        walk_expr(e, site, &mut scan);
        if scan.plain == 0 && scan.appends > 0 {
            self.found.push(addr(e));
            return false;
        }
        true
    }

    fn use_name(&mut self, _name: &'a str, _site: Site) {}

    fn bind(&mut self, _name: &'a str) {}
}

fn addr(e: &ast::Expr) -> usize {
    std::ptr::from_ref(e) as usize
}

fn binds_name(pattern: &ast::Pattern, name: &str) -> bool {
    struct Binds<'n>(NameScan<'n>);
    impl<'a> Visitor<'a> for Binds<'_> {
        fn expr(&mut self, _e: &'a ast::Expr, _site: Site) -> bool {
            false
        }
        fn use_name(&mut self, _name: &'a str, _site: Site) {}
        fn bind(&mut self, name: &'a str) {
            self.0.bind(name);
        }
    }
    let mut v = Binds(NameScan::new(name));
    walk_pattern(pattern, &mut v);
    v.0.shadows > 0
}

/// Walk what a `let name` in a block can see: the statements after it and the
/// block's value, stopping where a later sibling `let` shadows `name` (that
/// `let`'s own initialiser still sees the old binding, so it is walked).
fn walk_scope<'a>(
    rest: &'a [ast::Stmt],
    value: Option<&'a ast::Expr>,
    name: &str,
    v: &mut impl Visitor<'a>,
) {
    let site = TOP;
    for stmt in rest {
        if let ast::StmtKind::Let(l) = &stmt.kind
            && binds_name(&l.pattern, name)
        {
            if let Some(init) = &l.value {
                walk_expr(init, site, v);
            }
            return;
        }
        walk_stmt(stmt, site, v);
    }
    // The declaring block's own trailing value: taken as valued, which can
    // only decline a rewrite (an IIFE) that a statement block would allow.
    if let Some(value) = value {
        walk_expr(value, site, v);
    }
}

impl Emitter {
    /// Emit a block's statements, planning loop-carried string appends for each
    /// `let` that a later statement appends to.
    pub(crate) fn emit_stmts(&mut self, block: &ast::Block) {
        self.emit_hoisted_fns(block);
        let mut targets = AppendTargets::default();
        for stmt in &block.stmts {
            walk_stmt(stmt, TOP, &mut targets);
        }
        for (i, stmt) in block.stmts.iter().enumerate() {
            if let ast::StmtKind::Let(l) = &stmt.kind
                && let ast::Pattern::Binding { name, .. } = &l.pattern
                && l.value.is_some()
                && targets.names.contains(name.as_str())
            {
                self.plan_concat(name, &block.stmts[i + 1..], block.value.as_deref());
            }
            self.emit_stmt(stmt);
        }
    }

    /// Record the loops after `let name = …` that may accumulate their appends
    /// (§4.11). Declines when `name` is captured by a closure or nested
    /// function, or rebound, anywhere it is visible: a capture could read the
    /// string while the loop still holds it in the accumulator.
    fn plan_concat<'a>(&mut self, name: &str, rest: &'a [ast::Stmt], value: Option<&'a ast::Expr>) {
        let mut scope = NameScan::new(name);
        walk_scope(rest, value, name, &mut scope);
        if scope.captured > 0 || scope.shadows > 0 {
            return;
        }
        let mut finder = LoopFinder {
            name,
            found: Vec::new(),
        };
        walk_scope(rest, value, name, &mut finder);
        for loop_addr in finder.found {
            self.concat_plan
                .entry(loop_addr)
                .or_default()
                .push(name.to_string());
        }
    }

    /// Emit `emit_loop` (the loop `loop_expr`), accumulating the planned
    /// variables' appends: each gets a table seeded with the string's current
    /// value (checked as a `..` operand, see [`string_operand`]), and is folded back with `table.concat` right after the loop — so
    /// a `break` (which only leaves the loop) still lands on the folded string,
    /// and a `return` leaves a string nothing can read.
    pub(crate) fn emit_accumulating(
        &mut self,
        loop_expr: &ast::Expr,
        emit_loop: impl FnOnce(&mut Self),
    ) {
        let Some(names) = self.concat_plan.remove(&addr(loop_expr)) else {
            emit_loop(self);
            return;
        };
        self.line("do");
        self.indented(|em| {
            let mark = em.accumulators.len();
            for name in names {
                em.concat_seq += 1;
                let acc = format!("__sb_{}", em.concat_seq);
                em.line(format!("local {acc} = {{ {} }}", checked(&name)));
                em.accumulators.push((name, acc));
            }
            emit_loop(em);
            for (name, acc) in em.accumulators.split_off(mark) {
                em.line(format!("{name} = table.concat({acc})"));
            }
        });
        self.line("end");
    }

    /// Lower `return name` as the folded accumulator when `name` accumulates in
    /// the enclosing loop. Returns the Lua expression to return, if so.
    pub(crate) fn accumulated_return(&self, value: &ast::Expr) -> Option<String> {
        let (_, acc) = self
            .accumulators
            .iter()
            .rev()
            .find(|(name, _)| is_name(value, name))?;
        Some(format!("table.concat({acc})"))
    }

    /// Lower `target op= value` as accumulator pushes when it appends to a
    /// variable accumulating in the enclosing loop. Returns whether it did.
    pub(crate) fn emit_accumulated_append(
        &mut self,
        target: &ast::Expr,
        op: ast::AssignOp,
        value: &ast::Expr,
    ) -> bool {
        let Some((name, acc)) = self
            .accumulators
            .iter()
            .rev()
            .find(|(name, _)| is_name(target, name))
            .cloned()
        else {
            return false;
        };
        let Some(parts) = append_to(target, op, value, &name) else {
            return false;
        };
        for part in parts {
            let part = string_operand(part, self.emit_expr(part));
            self.line(format!("{acc}[#{acc} + 1] = {part}"));
        }
        true
    }
}

/// `lua` (the lowering of `part`) as a value `..` would accept, raising where
/// `..` would. A host can break an `ext fn`'s `string` contract; plain `s .. x`
/// then raises at the append, while `table.concat` would skip a `nil` and
/// report anything else only at the fold. `x .. ""` is the same check, and Lua
/// returns a string left operand of it unchanged. A string literal needs none.
fn string_operand(part: &ast::Expr, lua: String) -> String {
    if matches!(part.kind, ast::ExprKind::Str(_)) {
        lua
    } else {
        checked(&lua)
    }
}

/// `lua .. ""`: `lua` itself if it is a string, else what `..` does with it.
fn checked(lua: &str) -> String {
    format!("({lua} .. \"\")")
}
