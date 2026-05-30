//! Expression, statement, and block lowering to idiomatic Python.
//!
//! neoc is expression-oriented; Python is statement-oriented and has no
//! multi-statement lambda. So a block-like expression used **for its value** —
//! an `if`/`match`/block — is hoisted: its statements are emitted into the
//! current suite and a fresh temp is bound to its result, then the temp is the
//! expression's value. This mirrors the `?` operator's hoist (which Python needs
//! anyway, since `?` early-returns the enclosing function). A simple two-armed
//! `if` with non-block arms collapses to a Python conditional expression
//! (`a if c else b`) so the common case stays clean.
//!
//! `match` lowers to Python's native structural `match`/`case` — the whole point
//! of the Python idioms: a `case Ok(v):` binds against the real `Ok` dataclass.

use crate::Emitter;
use crate::expr::PathKind::{Assoc, Value, Variant};
use neoc_ast as ast;

/// What a `::`-joined path resolves to at a use site.
pub(crate) enum PathKind {
    /// A value name (local, function, const, prelude ctor).
    Value(String),
    /// An enum variant: `(enum name, variant name, shape)`.
    Variant(String, String, crate::VariantShape),
    /// An associated function `Type::fn` — emitted `Type.fn`.
    Assoc(String, String),
}

impl Emitter {
    /// Emit a block as a function/suite body: statements, then `return <value>`
    /// when the block has a trailing **value**. An empty body becomes `pass`.
    ///
    /// A tail expression that lowers to a *statement* with no bound value — a
    /// unit-valued `if`-without-`else`, a `while`/`for`/`loop`, an assignment, a
    /// bare statement-block — is emitted for effect with no `return`: there is no
    /// temp to return (a statement-position `if` produces none), so `return _ifN`
    /// would reference an unbound name. Only a value-producing tail (a `match`, a
    /// value `if`/`else`, a call, an arithmetic expr — all of which hoist to a
    /// bound temp) gets `return`.
    pub(crate) fn emit_block_as_body(&mut self, block: &ast::Block) {
        let start = self.line_count();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            if tail_is_statement(value) {
                self.emit_expr_stmt(value);
            } else {
                let v = self.emit_expr(value);
                self.line(format!("return {v}"));
            }
        } else if self.line_count() == start {
            self.line("pass");
        }
    }

    /// The block's statements without a trailing return (statement context); a
    /// trailing value is run for effect.
    pub(crate) fn emit_block_stmts(&mut self, block: &ast::Block) {
        let start = self.line_count();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            self.emit_expr_stmt(value);
        } else if self.line_count() == start {
            self.line("pass");
        }
    }

    fn line_count(&self) -> usize {
        self.lines_len()
    }

    fn emit_stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Let(l) => self.emit_let(l),
            ast::StmtKind::Item(item) => self.emit_item_stmt(item),
            ast::StmtKind::Expr(e) => self.emit_expr_stmt(e),
        }
    }

    fn emit_item_stmt(&mut self, item: &ast::Item) {
        if let ast::ItemKind::Fn(f) = &item.kind {
            self.emit_fn_local(f);
        } else {
            self.error("only nested functions are supported inside a block");
        }
    }

    fn emit_let(&mut self, l: &ast::LetStmt) {
        match &l.pattern {
            ast::Pattern::Wildcard => {
                if let Some(v) = &l.value {
                    self.emit_expr_stmt(v);
                }
            }
            ast::Pattern::Binding { name, .. } => match &l.value {
                Some(v) => {
                    let value = self.emit_expr(v);
                    if let Some(ty) = &l.ty {
                        self.note_type(ty);
                        self.line(format!("{name}: {} = {value}", crate::py_type(ty)));
                    } else {
                        self.line(format!("{name} = {value}"));
                    }
                }
                None => self.line(format!("{name} = None")),
            },
            _ => self.emit_let_destructure(&l.pattern, l.value.as_ref()),
        }
    }

    /// Lower an irrefutable destructuring `let` to a Python `match` of a single
    /// `case`. A struct/newtype pattern is irrefutable, so the one `case`
    /// always matches and binds; a refutable pattern stays a diagnostic.
    fn emit_let_destructure(&mut self, pattern: &ast::Pattern, value: Option<&ast::Expr>) {
        if !self.is_irrefutable_let_pattern(pattern) {
            let why = match pattern {
                ast::Pattern::Literal(_) => "a literal pattern",
                ast::Pattern::Path(_) => "an enum-variant pattern",
                ast::Pattern::Or(_) => "an or-pattern",
                _ => "this pattern",
            };
            self.error(format!(
                "refutable pattern in `let` is not supported: {why} can fail to match; use `match` instead"
            ));
            return;
        }
        let Some(value) = value else {
            self.error("destructuring `let` requires an initialiser");
            return;
        };
        let init = self.emit_expr(value);
        let case = self.compile_case(pattern);
        self.line(format!("match {init}:"));
        self.indented(|e| {
            e.line(format!("case {case}:"));
            e.indented(|e| e.line("pass"));
        });
    }

    fn is_irrefutable_let_pattern(&self, pattern: &ast::Pattern) -> bool {
        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Binding { .. } => true,
            ast::Pattern::Struct { path, fields, .. } => {
                self.pattern_variant(path).is_none()
                    && fields.iter().all(|f| {
                        f.pattern
                            .as_ref()
                            .is_none_or(|p| self.is_irrefutable_let_pattern(p))
                    })
            }
            ast::Pattern::Tuple { path, elems } => {
                self.pattern_variant(path).is_none()
                    && elems.iter().all(|p| self.is_irrefutable_let_pattern(p))
            }
            ast::Pattern::Literal(_) | ast::Pattern::Path(_) | ast::Pattern::Or(_) => false,
        }
    }

    /// An expression evaluated for effect.
    fn emit_expr_stmt(&mut self, e: &ast::Expr) {
        match &e.kind {
            ast::ExprKind::If { .. } => self.emit_if_stmt(e, None),
            ast::ExprKind::While { cond, body } => {
                let c = self.emit_expr(cond);
                self.line(format!("while {c}:"));
                self.indented(|em| em.emit_block_stmts(body));
            }
            ast::ExprKind::Loop { body } => {
                self.line("while True:");
                self.indented(|em| em.emit_block_stmts(body));
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.emit_for(pattern, iter, body),
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_stmt(scrutinee, arms, None),
            ast::ExprKind::Block(b) => self.emit_block_stmts(b),
            ast::ExprKind::Return(value) => {
                let r = value
                    .as_ref()
                    .map_or_else(String::new, |v| format!(" {}", self.emit_expr(v)));
                self.line(format!("return{r}"));
            }
            ast::ExprKind::Break => self.line("break"),
            ast::ExprKind::Continue => self.line("continue"),
            ast::ExprKind::Assign { target, op, value } => {
                // A plain assignment to a list index past the end grows the
                // list, matching neoc's list semantics (`out[next] = x` where
                // `next == len(out)` appends — the way Lua tables and JS arrays
                // behave). Python lists raise `IndexError` instead, so grow the
                // list with placeholders up to the index before assigning. The
                // index and value are bound to temporaries first so each is
                // evaluated once even when the grow loop runs.
                if let (ast::ExprKind::Index { base, index }, ast::AssignOp::Assign) =
                    (&target.kind, op)
                {
                    let b = self.emit_expr(base);
                    let idx = self.emit_expr(index);
                    let v = self.emit_expr(value);
                    let it = self.fresh_tmp("_idx");
                    let vt = self.fresh_tmp("_val");
                    self.line(format!("{it} = {idx}"));
                    self.line(format!("{vt} = {v}"));
                    self.line(format!("while len({b}) <= {it}:"));
                    self.indented(|em| em.line(format!("{b}.append(None)")));
                    self.line(format!("{b}[{it}] = {vt}"));
                    return;
                }
                let t = self.emit_expr(target);
                let v = self.emit_expr(value);
                let sym = match op {
                    ast::AssignOp::Assign => "=",
                    ast::AssignOp::Add => "+=",
                    ast::AssignOp::Sub => "-=",
                    ast::AssignOp::Mul => "*=",
                    ast::AssignOp::Div => "/=",
                    ast::AssignOp::Rem => "%=",
                };
                self.line(format!("{t} {sym} {v}"));
            }
            _ => {
                let s = self.emit_expr(e);
                self.line(s);
            }
        }
    }

    fn emit_for(&mut self, pattern: &ast::Pattern, iter: &ast::Expr, body: &ast::Block) {
        let var = match pattern {
            ast::Pattern::Binding { name, .. } => name.clone(),
            ast::Pattern::Wildcard => "_".to_string(),
            _ => {
                self.error("only a binding pattern is supported in `for`");
                "_".to_string()
            }
        };
        if let ast::ExprKind::Range {
            start,
            end,
            inclusive,
        } = &iter.kind
        {
            let lo = self.emit_expr(start);
            let hi = self.emit_expr(end);
            // `range` is half-open; an inclusive range adds 1 to the bound.
            let hi = if *inclusive { format!("{hi} + 1") } else { hi };
            self.line(format!("for {var} in range({lo}, {hi}):"));
        } else {
            let it = self.emit_expr(iter);
            self.line(format!("for {var} in {it}:"));
        }
        self.indented(|em| em.emit_block_stmts(body));
    }

    /// Emit an `if` as a statement. When `assign` is `Some(name)`, each branch's
    /// trailing value is assigned to `name` (the hoist for a value-position
    /// `if`); otherwise branches run for effect.
    fn emit_if_stmt(&mut self, e: &ast::Expr, assign: Option<&str>) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if {c}:"));
        self.indented(|em| em.emit_branch(then, assign));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                // `elif` chain: re-emit the nested `if` under `else`.
                let lines = self.render(|em| em.emit_if_stmt(inner, assign));
                self.splice_elif(&lines);
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else:");
                self.indented(|em| em.emit_branch(b, assign));
            }
            Some(other) => {
                let b = ast::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(other.clone())),
                };
                self.line("else:");
                self.indented(|em| em.emit_branch(&b, assign));
            }
            None => {}
        }
    }

    /// Emit one `if`/`elif`/`else` branch body: statements, then either assign
    /// the trailing value to `assign` or run it for effect.
    fn emit_branch(&mut self, block: &ast::Block, assign: Option<&str>) {
        let start = self.lines_len();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        match (assign, &block.value) {
            (Some(name), Some(value)) => {
                let v = self.emit_expr(value);
                self.line(format!("{name} = {v}"));
            }
            (None, Some(value)) => self.emit_expr_stmt(value),
            (_, None) if self.lines_len() == start => self.line("pass"),
            _ => {}
        }
    }

    /// Splice a captured `if …:` suite onto an open branch as `elif …:`.
    fn splice_elif(&mut self, lines: &[String]) {
        if let Some((first, rest)) = lines.split_first() {
            // `first` is `if <c>:`; rewrite the leading `if` to `elif`.
            let elif = first
                .strip_prefix("if ")
                .map_or_else(|| format!("el{first}"), |c| format!("elif {c}"));
            self.line(elif);
            for l in rest {
                self.line(l);
            }
        }
    }

    /// Emit a Python expression for `e`. A block-like value form is hoisted into
    /// the current suite and replaced by a temp.
    pub(crate) fn emit_expr(&mut self, e: &ast::Expr) -> String {
        match &e.kind {
            ast::ExprKind::Int(s) | ast::ExprKind::Float(s) => normalize_number(s),
            ast::ExprKind::Str(s) => py_string(s),
            ast::ExprKind::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            ast::ExprKind::Unit => "None".to_string(),
            ast::ExprKind::Path(segments) => self.emit_path(segments),
            ast::ExprKind::Binary { op, lhs, rhs } => {
                // `&&`/`||` short-circuit: the rhs is evaluated only when the lhs
                // does not already decide the result. A rhs that hoists (a
                // `match`/`?`/block emitting statements before its value) must not
                // run those statements unconditionally, or the effect fires even
                // when short-circuited — so guard the hoist behind the lhs.
                if matches!(op, ast::BinOp::And | ast::BinOp::Or) && self.expr_hoists(rhs) {
                    return self.emit_short_circuit(*op, lhs, rhs);
                }
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
                format!("({l} {} {r})", bin_op(*op))
            }
            ast::ExprKind::Unary { op, operand } => {
                let o = self.emit_expr(operand);
                match op {
                    ast::UnOp::Not => format!("(not {o})"),
                    ast::UnOp::Neg => format!("(-{o})"),
                }
            }
            ast::ExprKind::Call { callee, args } => self.emit_call(callee, args),
            ast::ExprKind::Field { base, field } => {
                let b = self.emit_expr(base);
                match field {
                    ast::FieldAccess::Named(n, _) => format!("{b}.{n}"),
                    ast::FieldAccess::Tuple(i) => format!("{b}._{i}"),
                }
            }
            ast::ExprKind::Index { base, index } => {
                let b = self.emit_expr(base);
                let i = self.emit_expr(index);
                format!("{b}[{i}]")
            }
            ast::ExprKind::StructLit { path, fields, base } => {
                self.emit_struct_lit(path, fields, base.as_deref())
            }
            ast::ExprKind::List(items) => {
                let parts: Vec<String> = items.iter().map(|i| self.emit_expr(i)).collect();
                format!("[{}]", parts.join(", "))
            }
            // A two-armed `if` with simple (value-only) branches is a Python
            // conditional expression; anything richer hoists.
            ast::ExprKind::If { cond, then, else_ } => {
                if let Some(expr) = self.try_cond_expr(cond, then, else_.as_deref()) {
                    expr
                } else {
                    self.hoist_if(e)
                }
            }
            ast::ExprKind::Block(_) => self.hoist_block(e),
            ast::ExprKind::Match { scrutinee, arms } => self.hoist_match(scrutinee, arms),
            ast::ExprKind::Closure { params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let b = self.emit_expr(body);
                format!("(lambda {}: {b})", ps.join(", "))
            }
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span: _,
                args,
            } => {
                let recv = self.emit_expr(receiver);
                // `Eq` is the one derive whose generated method takes both
                // operands; the `==` rewrite produces `MethodCall { "eq", [rhs] }`
                // — lower it back to Python `==` (the dataclass `__eq__`).
                if method == "eq" && args.len() == 1 {
                    let rhs = self.emit_expr(&args[0]);
                    return format!("({recv} == {rhs})");
                }
                let a = self.emit_args(args);
                format!("{recv}.{method}({a})")
            }
            ast::ExprKind::Try(inner) => self.emit_try(inner),
            ast::ExprKind::Range { .. } => {
                self.error("range expressions are only supported as a `for` iterable");
                "None".to_string()
            }
            ast::ExprKind::Assign { .. } => {
                self.error("assignment is a statement, not an expression value");
                "None".to_string()
            }
            ast::ExprKind::While { .. }
            | ast::ExprKind::For { .. }
            | ast::ExprKind::Loop { .. } => {
                self.error("loops do not produce a value");
                "None".to_string()
            }
            ast::ExprKind::Return(_) | ast::ExprKind::Break | ast::ExprKind::Continue => {
                self.error("control-flow expression used in value position");
                "None".to_string()
            }
        }
    }

    /// A two-armed `if` with value-only (no-statement) branches collapses to a
    /// Python conditional expression `then if cond else else`. Returns `None`
    /// when either branch has statements or the `else` is missing/an `elif`.
    fn try_cond_expr(
        &mut self,
        cond: &ast::Expr,
        then: &ast::Block,
        else_: Option<&ast::Expr>,
    ) -> Option<String> {
        let then_val = block_value_only(then)?;
        let else_block = match else_ {
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => block_value_only(b)?,
            _ => return None,
        };
        let c = self.emit_expr(cond);
        let t = self.emit_expr(then_val);
        let e = self.emit_expr(else_block);
        Some(format!("({t} if {c} else {e})"))
    }

    /// Hoist a value-position `if`: bind a fresh temp, emit the `if` as a
    /// statement assigning each branch's value to it, yield the temp.
    fn hoist_if(&mut self, e: &ast::Expr) -> String {
        let tmp = self.fresh_tmp("_if");
        self.emit_if_stmt(e, Some(&tmp));
        tmp
    }

    /// Hoist a value-position block: emit its statements, bind a temp to its
    /// trailing value, yield the temp.
    fn hoist_block(&mut self, e: &ast::Expr) -> String {
        let ast::ExprKind::Block(b) = &e.kind else {
            return "None".to_string();
        };
        for stmt in &b.stmts {
            self.emit_stmt(stmt);
        }
        match &b.value {
            Some(value) => {
                let v = self.emit_expr(value);
                let tmp = self.fresh_tmp("_blk");
                self.line(format!("{tmp} = {v}"));
                tmp
            }
            None => "None".to_string(),
        }
    }

    /// Hoist a value-position `match`: bind a temp, emit a native `match`
    /// statement assigning each arm's value to it, yield the temp.
    fn hoist_match(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) -> String {
        let tmp = self.fresh_tmp("_m");
        self.emit_match_stmt(scrutinee, arms, Some(&tmp));
        tmp
    }

    /// Whether emitting `e` for its value would hoist statements into the current
    /// suite (a `match`/`?`/block, or anything containing a `?`). Such a value
    /// cannot sit inline on the rhs of a short-circuiting `&&`/`||` without its
    /// hoisted statements running unconditionally. Detected by emitting into a
    /// throwaway buffer and checking whether any line was produced.
    fn expr_hoists(&mut self, e: &ast::Expr) -> bool {
        let saved_seq = (self.try_seq, self.tmp_seq);
        let lines = self.render(|em| {
            let _ = em.emit_expr(e);
        });
        // Restore the temp counters: the throwaway emit must not perturb the
        // names the real emit will choose.
        self.try_seq = saved_seq.0;
        self.tmp_seq = saved_seq.1;
        !lines.is_empty()
    }

    /// Lower a short-circuiting `&&`/`||` whose rhs hoists. Seed a temp from the
    /// lhs, then guard the rhs (its hoisted statements and the temp's
    /// reassignment) so it runs only when the lhs has not already decided the
    /// result — matching the Lua/JS short-circuit semantics.
    fn emit_short_circuit(&mut self, op: ast::BinOp, lhs: &ast::Expr, rhs: &ast::Expr) -> String {
        let l = self.emit_expr(lhs);
        let tmp = self.fresh_tmp("_sc");
        self.line(format!("{tmp} = {l}"));
        // `||` evaluates the rhs only when the lhs is falsey; `&&` only when truthy.
        let guard = match op {
            ast::BinOp::Or => format!("if not {tmp}:"),
            _ => format!("if {tmp}:"),
        };
        self.line(guard);
        self.indented(|em| {
            let r = em.emit_expr(rhs);
            em.line(format!("{tmp} = {r}"));
        });
        tmp
    }

    /// Lower a `match` to Python's native structural `match`/`case`. When
    /// `assign` is `Some`, each arm assigns its value to that name; otherwise
    /// arm bodies run for effect. Exhaustiveness is guaranteed upstream, so no
    /// fallback case is emitted.
    pub(crate) fn emit_match_stmt(
        &mut self,
        scrutinee: &ast::Expr,
        arms: &[ast::MatchArm],
        assign: Option<&str>,
    ) {
        let scrut = self.emit_expr(scrutinee);
        self.line(format!("match {scrut}:"));
        self.indented(|em| {
            for arm in arms {
                let case = em.compile_case(&arm.pattern);
                let guard = arm
                    .guard
                    .as_ref()
                    .map(|g| em.emit_expr(g))
                    .map_or_else(String::new, |g| format!(" if {g}"));
                em.line(format!("case {case}{guard}:"));
                em.indented(|em| em.emit_arm_body(&arm.body, assign));
            }
        });
    }

    /// Emit a match-arm body: assign to the hoist temp in value context, run for
    /// effect otherwise.
    fn emit_arm_body(&mut self, body: &ast::Expr, assign: Option<&str>) {
        match assign {
            Some(name) => {
                let v = self.emit_expr(body);
                self.line(format!("{name} = {v}"));
            }
            None => self.emit_expr_stmt(body),
        }
    }

    /// Lower a call. A path callee may construct an enum variant or a
    /// tuple/newtype struct (both plain `Name(args)` in Python), or be an
    /// ordinary call.
    fn emit_call(&mut self, callee: &ast::Expr, args: &[ast::Expr]) -> String {
        if let ast::ExprKind::Path(segments) = &callee.kind
            && let Some(Variant(en, var, _)) = self.resolve_path(segments)
        {
            self.note_prelude(&en);
            let a = self.emit_args(args);
            return format!("{}({a})", crate::variant_class_name(&var));
        }
        let f = self.emit_expr(callee);
        let a = self.emit_args(args);
        format!("{f}({a})")
    }

    fn emit_path(&mut self, segments: &[ast::Ident]) -> String {
        match self.resolve_path(segments) {
            Some(Value(name)) => {
                self.note_prelude(&name);
                // A unit-variant constructor reference (`None`) constructs the
                // singleton; a bare `Ok`/`Some` etc. resolves to its class.
                if name == "None" {
                    return "None_()".to_string();
                }
                name
            }
            Some(Assoc(ty, name)) => format!("{ty}.{name}"),
            Some(Variant(en, var, crate::VariantShape::Unit)) => {
                self.note_prelude(&en);
                format!("{}()", crate::variant_class_name(&var))
            }
            Some(Variant(_, _, _)) => {
                self.error(format!(
                    "enum variant `{}` needs its payload to construct a value",
                    segments.join("::")
                ));
                "None".to_string()
            }
            None => {
                self.error(format!(
                    "unresolved path `{}`: not a value or enum variant",
                    segments.join("::")
                ));
                "None".to_string()
            }
        }
    }

    /// Resolve a `::`-joined path.
    pub(crate) fn resolve_path(&self, segments: &[ast::Ident]) -> Option<PathKind> {
        match segments {
            [] => None,
            [one] => Some(Value(one.clone())),
            [.., en, var] => self
                .variant_shape(en, var)
                .map(|shape| Variant(en.clone(), var.clone(), shape))
                .or_else(|| {
                    self.type_has_methods(en)
                        .then(|| Assoc(en.clone(), var.clone()))
                })
                .or_else(|| Some(Value(var.clone()))),
        }
    }

    pub(crate) fn type_has_methods(&self, name: &str) -> bool {
        self.method_types.contains(name)
    }

    pub(crate) fn variant_shape(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Option<crate::VariantShape> {
        self.enums.variant_shape(enum_name, variant)
    }

    /// Resolve a path *in pattern position* to the variant it matches.
    pub(crate) fn pattern_variant(
        &self,
        segments: &[ast::Ident],
    ) -> Option<(String, String, crate::VariantShape)> {
        match segments {
            [one] => self
                .enums
                .unqualified_variant(one)
                .map(|(en, shape)| (en, one.clone(), shape)),
            [.., en, var] => self
                .variant_shape(en, var)
                .map(|shape| (en.clone(), var.clone(), shape)),
            [] => None,
        }
    }

    fn emit_args(&mut self, args: &[ast::Expr]) -> String {
        args.iter()
            .map(|a| self.emit_expr(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Lower a struct literal `S { f: v, .. }` to a Python constructor call
    /// `S(f=v, …)`. A struct-shaped enum variant `E::V { x: a }` builds `V(x=a)`.
    /// A functional update `S { f, .. base }` uses `dataclasses.replace`.
    fn emit_struct_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        base: Option<&ast::Expr>,
    ) -> String {
        if let Some(Variant(en, var, crate::VariantShape::Named)) = self.resolve_path(path) {
            self.note_prelude(&en);
            let body = self.emit_field_kwargs(fields);
            return format!("{}({body})", crate::variant_class_name(&var));
        }
        let Some(name) = path.last() else {
            return "None".to_string();
        };
        if let Some(base) = base {
            let base_py = self.emit_expr(base);
            let body = self.emit_field_kwargs(fields);
            // `frozen=True` dataclasses are updated with `dataclasses.replace`.
            self.uses_replace = true;
            if body.is_empty() {
                return format!("replace({base_py})");
            }
            return format!("replace({base_py}, {body})");
        }
        let body = self.emit_field_kwargs(fields);
        format!("{name}({body})")
    }

    /// Render struct-literal / struct-variant fields as `name=value` keyword
    /// arguments, expanding field shorthand (`{ x }` → `x=x`).
    fn emit_field_kwargs(&mut self, fields: &[ast::FieldInit]) -> String {
        fields
            .iter()
            .map(|f| {
                let value = match &f.value {
                    Some(v) => self.emit_expr(v),
                    None => f.name.clone(),
                };
                format!("{}={value}", f.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Lower `e?`. The short-circuit returns the carrier from the enclosing
    /// function, so the guard is hoisted as statements: bind a temp, early-return
    /// it when it is `Err`/`None_`, and yield its payload `_0` otherwise.
    fn emit_try(&mut self, inner: &ast::Expr) -> String {
        let value = self.emit_expr(inner);
        let tmp = format!("_try{}", self.try_seq);
        self.try_seq += 1;
        self.note_prelude("Result");
        self.line(format!("{tmp} = {value}"));
        self.line(format!("if isinstance({tmp}, (Err, None_)):"));
        self.indented(|e| e.line(format!("return {tmp}")));
        format!("{tmp}._0")
    }

    fn emit_fn_local(&mut self, f: &ast::FnDecl) {
        let header = self.fn_header_local(&f.sig);
        self.line(header);
        self.indented(|em| em.emit_block_as_body(&f.body));
    }

    fn fn_header_local(&mut self, sig: &ast::FnSig) -> String {
        let mut params: Vec<String> = Vec::new();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => params.push("self".to_string()),
                ast::Param::Named { name, ty, .. } => {
                    self.note_type(ty);
                    params.push(format!("{name}: {}", crate::py_type(ty)));
                }
            }
        }
        if let Some(ret) = &sig.ret {
            self.note_type(ret);
        }
        let ret = sig
            .ret
            .as_ref()
            .map_or_else(|| "None".to_string(), crate::py_type);
        format!("def {}({}) -> {}:", sig.name, params.join(", "), ret)
    }
}

/// Whether a block-tail expression lowers to a pure statement with no bound
/// value — so it must be emitted for effect, never `return`ed. These are the
/// unit-valued statement forms: an `if` with no `else` (no value, no
/// hoist-temp), the loops, assignment, and the bare control-flow forms. A bare
/// block defers to its own tail. Everything else produces a value temp.
fn tail_is_statement(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::If { else_, .. } => else_.is_none(),
        ast::ExprKind::While { .. }
        | ast::ExprKind::For { .. }
        | ast::ExprKind::Loop { .. }
        | ast::ExprKind::Assign { .. }
        | ast::ExprKind::Return(_)
        | ast::ExprKind::Break
        | ast::ExprKind::Continue => true,
        ast::ExprKind::Block(b) => b.value.as_deref().is_some_and(tail_is_statement),
        _ => false,
    }
}

/// A block whose only content is a trailing value (no statements) — eligible to
/// collapse into a Python conditional expression.
fn block_value_only(block: &ast::Block) -> Option<&ast::Expr> {
    if block.stmts.is_empty() {
        block.value.as_deref()
    } else {
        None
    }
}

fn bin_op(op: ast::BinOp) -> &'static str {
    match op {
        ast::BinOp::Or => "or",
        ast::BinOp::And => "and",
        ast::BinOp::Eq => "==",
        ast::BinOp::Ne => "!=",
        ast::BinOp::Lt => "<",
        ast::BinOp::Gt => ">",
        ast::BinOp::Le => "<=",
        ast::BinOp::Ge => ">=",
        // Numeric `+`. String concatenation is `Concat` (rewritten by the
        // checker); Python `+` serves both.
        ast::BinOp::Add | ast::BinOp::Concat => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        // neoc `number` is a double; `/` is true division, matching Python `/`.
        ast::BinOp::Div => "/",
        ast::BinOp::Rem => "%",
    }
}

/// Normalise a neoc numeric literal to Python: strip digit separators. A neoc
/// `number` is a double, so an integer literal stays integer-shaped here and
/// participates in float arithmetic exactly as Python promotes it.
fn normalize_number(s: &str) -> String {
    s.replace('_', "")
}

/// Encode a string value as a Python double-quoted literal.
pub(crate) fn py_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
