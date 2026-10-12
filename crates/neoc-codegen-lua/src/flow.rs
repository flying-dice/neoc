//! Control flow out of value-position wrappers (`lang/DYNAMIC.md` §4.12).
//!
//! A block, `if`, or `match` used for its value lowers to an IIFE, and a Lua
//! `return` or `break` leaves only the function it is written in. So a neoc
//! `return`, `?`, `break`, or `continue` inside such a wrapper that targets
//! something *outside* it needs help. The wrapper then returns an exit code and
//! a value; its call site, hoisted ahead of the statement that uses the value,
//! acts on the code in the enclosing context:
//!
//! ```text
//! local __e1, __v1 = (function() … return 1, <v> … return false, <v> end)()
//! if __e1 then return __v1 end
//! ```
//!
//! Which exits a wrapper can take is decided up front by [`Exits`], so a
//! wrapper that cannot leave early keeps the plain single-value form.

use crate::Emitter;
use crate::concat::{Site, TOP, Visitor, walk_block, walk_expr};
use neoc_ast as ast;

/// Exit code of a `return` (or `?`) leaving the enclosing function.
pub(crate) const EXIT_RETURN: u8 = 1;
/// Exit code of a `break` leaving a loop outside the wrapper.
pub(crate) const EXIT_BREAK: u8 = 2;
/// Exit code of a `continue` of a loop outside the wrapper.
pub(crate) const EXIT_CONTINUE: u8 = 3;

/// A value-position wrapper being emitted.
pub(crate) struct IifeFrame {
    /// Depth of the loop stack at entry: a `break` or `continue` only binds
    /// locally when the stack is deeper than this.
    loops: usize,
    /// Whether the wrapper returns `(exit, value)` rather than a bare value.
    protocol: bool,
}

/// The ways code inside a wrapper can leave it.
#[derive(Clone, Copy, Default)]
pub(crate) struct Exits {
    ret: bool,
    brk: bool,
    cont: bool,
}

impl Exits {
    fn any(self) -> bool {
        self.ret || self.brk || self.cont
    }

    fn codes(self) -> Vec<u8> {
        [
            (EXIT_RETURN, self.ret),
            (EXIT_BREAK, self.brk),
            (EXIT_CONTINUE, self.cont),
        ]
        .into_iter()
        .filter_map(|(code, on)| on.then_some(code))
        .collect()
    }
}

/// Finds the exits of a region: every `return` and `?`, and every `break` and
/// `continue` not bound by a loop inside the region. Closures and nested
/// functions are separate Lua functions and are skipped.
#[derive(Default)]
struct ExitScan {
    /// Loops of the region enclosing the node being visited.
    depth: usize,
    exits: Exits,
}

impl<'a> Visitor<'a> for ExitScan {
    fn expr(&mut self, e: &'a ast::Expr, site: Site) -> bool {
        if site.closure {
            return false;
        }
        match &e.kind {
            ast::ExprKind::Return(_) | ast::ExprKind::Try(_) => self.exits.ret = true,
            ast::ExprKind::Break => self.exits.brk |= self.depth == 0,
            ast::ExprKind::Continue => self.exits.cont |= self.depth == 0,
            ast::ExprKind::While { cond, body } => {
                walk_expr(cond, site, self);
                self.in_loop(|scan| walk_block(body, site, false, scan));
                return false;
            }
            ast::ExprKind::For { iter, body, .. } => {
                walk_expr(iter, site, self);
                self.in_loop(|scan| walk_block(body, site, false, scan));
                return false;
            }
            ast::ExprKind::Loop { body } => {
                self.in_loop(|scan| walk_block(body, site, false, scan));
                return false;
            }
            _ => {}
        }
        true
    }

    fn use_name(&mut self, _name: &'a str, _site: Site) {}

    fn bind(&mut self, _name: &'a str) {}
}

impl ExitScan {
    fn in_loop(&mut self, f: impl FnOnce(&mut Self)) {
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }
}

/// The exits of the value-position `if` or block `e`.
fn exits_of(e: &ast::Expr) -> Exits {
    let mut scan = ExitScan::default();
    walk_expr(e, TOP, &mut scan);
    scan.exits
}

/// The exits of a value-position `match`'s arms. The scrutinee is evaluated
/// outside the wrapper and is not part of it.
fn exits_of_arms(arms: &[ast::MatchArm]) -> Exits {
    let mut scan = ExitScan::default();
    for arm in arms {
        if let Some(guard) = &arm.guard {
            walk_expr(guard, TOP, &mut scan);
        }
        walk_expr(&arm.body, TOP, &mut scan);
    }
    scan.exits
}

/// Whether `e` ends the current path instead of producing a value.
pub(crate) fn diverges(e: &ast::Expr) -> bool {
    matches!(
        e.kind,
        ast::ExprKind::Return(_) | ast::ExprKind::Break | ast::ExprKind::Continue
    )
}

/// Whether `e` lowers to a literal that no statement can change.
fn is_literal(e: &ast::Expr) -> bool {
    matches!(
        e.kind,
        ast::ExprKind::Int(_)
            | ast::ExprKind::Float(_)
            | ast::ExprKind::Str(_)
            | ast::ExprKind::Bool(_)
            | ast::ExprKind::Unit
    )
}

impl Emitter {
    /// Emit the value-position `if` or block `e` as a Lua expression.
    pub(crate) fn emit_wrapped(&mut self, e: &ast::Expr, body: impl FnOnce(&mut Self)) -> String {
        self.emit_iife(exits_of(e), body)
    }

    /// Emit a value-position `match` as a Lua expression.
    pub(crate) fn emit_wrapped_arms(
        &mut self,
        arms: &[ast::MatchArm],
        body: impl FnOnce(&mut Self),
    ) -> String {
        self.emit_iife(exits_of_arms(arms), body)
    }

    /// Run `body` as the contents of an IIFE and return the Lua expression for
    /// its value. A wrapper with `exits` hoists its call and the dispatch on the
    /// exit code into the current buffer (§4.12).
    fn emit_iife(&mut self, exits: Exits, body: impl FnOnce(&mut Self)) -> String {
        self.iifes.push(IifeFrame {
            loops: self.loops.len(),
            protocol: exits.any(),
        });
        let lines = self.render(body);
        self.iifes.pop();
        let iife = Self::wrap_lines("(function()", lines, "end)()");
        if !exits.any() {
            return iife;
        }
        self.flow_seq += 1;
        let (exit, value) = (
            format!("__exit_{}", self.flow_seq),
            format!("__val_{}", self.flow_seq),
        );
        self.line(format!("local {exit}, {value} = {iife}"));
        let codes = exits.codes();
        for (i, code) in codes.iter().enumerate() {
            let test = if codes.len() == 1 {
                exit.clone()
            } else {
                format!("{exit} == {code}")
            };
            let kw = if i == 0 { "if" } else { "elseif" };
            self.line(format!("{kw} {test} then"));
            self.indented(|em| match *code {
                EXIT_RETURN => em.emit_return(Some(value.clone())),
                EXIT_BREAK => em.emit_break(),
                _ => em.emit_continue(),
            });
        }
        self.line("end");
        value
    }

    /// Wrap captured `body` lines between `open` and `close`.
    pub(crate) fn wrap_lines(open: &str, body: Vec<String>, close: &str) -> String {
        let mut out = format!("{open}\n");
        for l in body {
            if !l.is_empty() {
                out.push_str("  ");
                out.push_str(&l);
            }
            out.push('\n');
        }
        out.push_str(close);
        out
    }

    /// The innermost wrapper being emitted, if the code being emitted sits in
    /// one rather than directly in a function body.
    fn wrapper(&self) -> Option<&IifeFrame> {
        self.iifes.last()
    }

    /// Whether a `break` or `continue` here leaves the wrapper it is in, because
    /// the loop it targets is outside.
    pub(crate) fn loop_is_outside(&self) -> bool {
        self.wrapper().is_some_and(|f| self.loops.len() <= f.loops)
    }

    /// The Lua statement that returns `value` from the neoc function: a plain
    /// `return` in the function body, the return exit from a wrapper.
    pub(crate) fn return_stmt(&self, value: Option<String>) -> String {
        let v = value.map_or_else(String::new, |v| format!(" {v}"));
        if self.wrapper().is_none() {
            return format!("return{v}");
        }
        let sep = if v.is_empty() { "" } else { "," };
        format!("return {EXIT_RETURN}{sep}{v}")
    }

    /// Emit a `return` of the Lua expression `value` from the neoc function.
    pub(crate) fn emit_return(&mut self, value: Option<String>) {
        let stmt = self.return_stmt(value);
        self.line(stmt);
    }

    /// Emit a `break` or `continue` that leaves the wrapper.
    pub(crate) fn emit_loop_exit(&mut self, code: u8) {
        self.line(format!("return {code}"));
    }

    /// Emit the value a wrapper or function body ends with.
    pub(crate) fn emit_tail(&mut self, value: &ast::Expr) {
        if diverges(value) {
            self.emit_expr_stmt(value);
            return;
        }
        let v = self.emit_expr(value);
        if self.wrapper().is_some_and(|f| f.protocol) {
            self.line(format!("return false, {v}"));
        } else {
            self.line(format!("return {v}"));
        }
    }

    /// Run `f` for the body of a function — a `fn` item or a closure — where
    /// `return` and loops are the function's own and nothing outside is visible.
    pub(crate) fn in_function<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let iifes = std::mem::take(&mut self.iifes);
        let loops = std::mem::take(&mut self.loops);
        let r = f(self);
        self.iifes = iifes;
        self.loops = loops;
        r
    }

    /// Emit `es` left to right as Lua operands. An operand that hoists
    /// statements (§4.10, §4.12) runs them before the operands to its left have
    /// been evaluated, so each earlier operand that is not a literal is bound to
    /// a fresh local first, keeping neoc's left-to-right evaluation order.
    pub(crate) fn emit_operands(&mut self, es: &[&ast::Expr]) -> Vec<String> {
        self.emit_operands_from(es, false)
    }

    /// Emit a call's callee and arguments as [`Self::emit_operands`]. A callee
    /// that is a bare name stays as written: only a value could be changed by
    /// the statements an argument hoists.
    pub(crate) fn emit_call_operands(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
    ) -> (String, Vec<String>) {
        let mut es = vec![callee];
        es.extend(args);
        let mut lua = self
            .emit_operands_from(&es, matches!(callee.kind, ast::ExprKind::Path(_)))
            .into_iter();
        let callee = lua.next().unwrap_or_default();
        (callee, lua.collect())
    }

    fn emit_operands_from(&mut self, es: &[&ast::Expr], first_is_name: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(es.len());
        for e in es {
            let mut text = String::new();
            let hoisted = self.render(|em| text = em.emit_expr(e));
            if !hoisted.is_empty() {
                for (i, (prev, lua)) in es.iter().zip(out.iter_mut()).enumerate() {
                    if !(is_literal(prev) || i == 0 && first_is_name) {
                        self.place_seq += 1;
                        let tmp = format!("__op_{}", self.place_seq);
                        self.line(format!("local {tmp} = {lua}"));
                        *lua = tmp;
                    }
                }
                for l in hoisted {
                    self.line(l);
                }
            }
            out.push(text);
        }
        out
    }

    /// Emit two operands left to right, as [`Self::emit_operands`].
    pub(crate) fn emit_pair(&mut self, a: &ast::Expr, b: &ast::Expr) -> (String, String) {
        let mut lua = self.emit_operands(&[a, b]).into_iter();
        let a = lua.next().unwrap_or_default();
        (a, lua.next().unwrap_or_default())
    }

    /// Emit `lhs && rhs` or `lhs || rhs`. The right operand runs only when the
    /// left does not decide the result, so statements it hoists run under that
    /// same condition: the result binds to a local and the right operand
    /// assigns it inside an `if`.
    pub(crate) fn emit_short_circuit(
        &mut self,
        op: ast::BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
    ) -> String {
        let l = self.emit_expr(lhs);
        let mut r = String::new();
        let hoisted = self.render(|em| r = em.emit_expr(rhs));
        let sym = crate::expr::bin_op(op);
        if hoisted.is_empty() {
            return format!("({l} {sym} {r})");
        }
        self.place_seq += 1;
        let tmp = format!("__op_{}", self.place_seq);
        self.line(format!("local {tmp} = {l}"));
        let guard = if op == ast::BinOp::And {
            tmp.clone()
        } else {
            format!("not {tmp}")
        };
        self.line(format!("if {guard} then"));
        self.indented(|em| {
            for l in hoisted {
                em.line(l);
            }
            em.line(format!("{tmp} = {r}"));
        });
        self.line("end");
        tmp
    }
}
