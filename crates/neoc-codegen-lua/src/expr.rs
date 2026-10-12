//! Expression, statement, and block lowering.
//!
//! `emit_expr` returns a Lua expression string (block-like forms become IIFEs
//! so they carry a value). Statement-context lowering pushes lines via the
//! emitter's line buffer. Enum construction, `match`, and `?` consult the
//! [`crate::EnumIndex`] to resolve variant paths; method calls lower to Lua
//! colon calls (metatable dispatch). The one form still needing the type
//! checker is `+` on `string` (it lowers to `..` only after the checker
//! rewrites the operator to [`ast::BinOp::Concat`]).

use crate::Emitter;
use neoc_ast as ast;

/// What a `::`-joined path resolves to at a use site.
pub(crate) enum PathKind {
    /// The path is a value name (local, function, const, prelude ctor).
    Value(String),
    /// The path names an enum variant: `(enum name, variant name, shape)`.
    Variant(String, String, VariantShape),
    /// The path is an associated item on a type — `Type::member`, where `Type`
    /// is a local `impl` target or a `use`d type. Emits `Type.member`, keeping
    /// the qualifier so it reads the type's table rather than dropping to a
    /// bare (unbound) name.
    Assoc(String, String),
}

/// The runtime payload shape of an enum variant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariantShape {
    Unit,
    Tuple,
    Named,
}

impl Emitter {
    /// Emit a block as a function body: statements, then `return <value>` when
    /// the block has a trailing value.
    pub(crate) fn emit_block_as_body(&mut self, block: &ast::Block) {
        self.emit_stmts(block);
        if let Some(value) = &block.value {
            self.at_span(value.span, |em| em.emit_tail(value));
        }
    }

    pub(super) fn emit_stmt(&mut self, stmt: &ast::Stmt) {
        self.at_span(stmt.span, |em| match &stmt.kind {
            ast::StmtKind::Let(l) => em.emit_let(l),
            // Already emitted at the head of the block by `emit_hoisted_fns`.
            ast::StmtKind::Item(_) => {}
            ast::StmtKind::Expr(e) => em.emit_expr_stmt(e),
        });
    }

    /// Emit a block's item statements ahead of its other statements
    /// (`DYNAMIC.md` §2): the checker hoists them over the whole block, so
    /// each is a Lua local in scope from the block's first line. A lone `fn`
    /// is a `local function`, so it can recurse. Several are declared together
    /// first and assigned after, so siblings can call each other.
    pub(crate) fn emit_hoisted_fns(&mut self, block: &ast::Block) {
        let fns: Vec<(&ast::FnDecl, ast::Span)> = block
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                ast::StmtKind::Item(ast::Item {
                    kind: ast::ItemKind::Fn(f),
                    ..
                }) => Some((f, s.span)),
                _ => None,
            })
            .collect();
        if let [(f, span)] = fns.as_slice() {
            self.at_span(*span, |em| em.emit_fn_local(f, "local function"));
            return;
        }
        if fns.is_empty() {
            return;
        }
        let names: Vec<&str> = fns.iter().map(|(f, _)| f.sig.name.as_str()).collect();
        self.line(format!("local {}", names.join(", ")));
        for (f, span) in fns {
            self.at_span(span, |em| em.emit_fn_local(f, "function"));
        }
    }

    fn emit_let(&mut self, l: &ast::LetStmt) {
        // A simple binding or wildcard maps straight to a `local`.
        let name = match &l.pattern {
            ast::Pattern::Binding { name, .. } => Some(name.clone()),
            ast::Pattern::Wildcard => Some("_".to_string()),
            _ => None,
        };
        if let Some(name) = name {
            match &l.value {
                Some(v) => {
                    let value = self.emit_expr(v);
                    self.line(format!("local {name} = {value}"));
                }
                None => self.line(format!("local {name}")),
            }
            return;
        }
        self.emit_let_destructure(&l.pattern, l.value.as_ref());
    }

    /// Lower an irrefutable destructuring `let`: a struct (`let Point { x, y } =
    /// p`) or newtype/tuple-struct (`let Wrapper(inner) = w`) pattern. The
    /// initialiser is bound to a temp once, then each variable is bound from a
    /// projection of it, reusing the bind logic in [`crate::pat`]. A refutable
    /// pattern (an enum variant with siblings) stays a diagnostic.
    fn emit_let_destructure(&mut self, pattern: &ast::Pattern, value: Option<&ast::Expr>) {
        if !self.is_irrefutable_let_pattern(pattern) {
            let why = match pattern {
                ast::Pattern::Literal(_) => "a literal pattern",
                ast::Pattern::Path(_) => "an enum-variant pattern",
                ast::Pattern::Or(_) => "an or-pattern",
                // A variant-headed `Struct`/`Tuple`, or one with a refutable field.
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
        self.line(format!("local __d = {init}"));
        let compiled = self.compile_pattern(pattern, "__d");
        for bind in &compiled.binds {
            self.line(bind);
        }
    }

    /// Whether `pattern` is irrefutable in `let`: a wildcard, a binding, or a
    /// struct/newtype/tuple-struct destructure whose sub-patterns are all
    /// irrefutable. An enum-variant path is refutable (it can fail to match a
    /// sibling variant) and stays a diagnostic.
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

    /// An expression used for effect. Block-like forms lower as statements (no
    /// IIFE needed); everything else is a bare expression line.
    pub(crate) fn emit_expr_stmt(&mut self, e: &ast::Expr) {
        match &e.kind {
            ast::ExprKind::If { .. } => self.emit_if_stmt(e),
            ast::ExprKind::While { cond, body } => self.emit_accumulating(e, |em| {
                let mut c = String::new();
                let hoisted = em.render(|em| c = em.emit_expr(cond));
                if hoisted.is_empty() {
                    em.line(format!("while {c} do"));
                } else {
                    // The condition's hoisted statements run on every pass.
                    em.line("while true do");
                    em.indented(|em| {
                        for l in hoisted {
                            em.line(l);
                        }
                        em.line(format!("if not ({c}) then break end"));
                    });
                }
                em.emit_loop_body(body);
                em.line("end");
            }),
            ast::ExprKind::Loop { body } => self.emit_accumulating(e, |em| {
                em.line("while true do");
                em.emit_loop_body(body);
                em.line("end");
            }),
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.emit_accumulating(e, |em| em.emit_for(pattern, iter, body)),
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_stmt(scrutinee, arms),
            ast::ExprKind::Block(b) => {
                self.line("do");
                self.indented(|em| em.emit_block_stmts(b));
                self.line("end");
            }
            ast::ExprKind::Return(value) => {
                let v = value.as_ref().map(|v| {
                    self.accumulated_return(v)
                        .unwrap_or_else(|| self.emit_expr(v))
                });
                self.emit_return(v);
            }
            ast::ExprKind::Break => self.emit_break(),
            ast::ExprKind::Continue => self.emit_continue(),
            ast::ExprKind::Assign { target, op, value } => {
                if !self.emit_accumulated_append(target, *op, value) {
                    self.emit_assign(target, *op, value);
                }
            }
            _ => {
                let s = self.emit_expr(e);
                self.line(s);
            }
        }
    }

    /// Lower `target op= value`. Lua has no compound assignment, so `p op= v`
    /// expands to `p = p op v` with the place `p` evaluated once (`STATIC.md`
    /// §4.1): a field or index base, or an index, that is not a name or literal
    /// binds to a fresh local first, scoped by a `do … end`.
    fn emit_assign(&mut self, target: &ast::Expr, op: ast::AssignOp, value: &ast::Expr) {
        let seq = self.place_seq;
        let body = self.render(|em| {
            let t = if op == ast::AssignOp::Assign {
                em.emit_expr(target)
            } else {
                em.emit_place(target)
            };
            let v = em.emit_expr(value);
            let rhs = match op {
                ast::AssignOp::Assign => v,
                ast::AssignOp::Add => format!("{t} + {v}"),
                ast::AssignOp::Sub => format!("{t} - {v}"),
                ast::AssignOp::Mul => format!("{t} * {v}"),
                ast::AssignOp::Div => format!("{t} / {v}"),
                ast::AssignOp::Rem => format!("{t} % {v}"),
                ast::AssignOp::Concat => format!("({t} .. {v})"),
            };
            em.line(format!("{t} = {rhs}"));
        });
        let scoped = self.place_seq != seq;
        if scoped {
            self.line("do");
            self.indent += 1;
        }
        for l in body {
            self.line(l);
        }
        if scoped {
            self.indent -= 1;
            self.line("end");
        }
    }

    /// The Lua place for a compound-assignment target, with any base or index
    /// that is not a name or literal bound to a fresh local (see
    /// [`Self::emit_assign`]).
    fn emit_place(&mut self, target: &ast::Expr) -> String {
        match &target.kind {
            ast::ExprKind::Field { base, field } => {
                let b = self.emit_once(base);
                field_access(&b, field)
            }
            ast::ExprKind::Index { base, index } => {
                let b = self.emit_once(base);
                let i = self.emit_once(index);
                index_access(&b, &i)
            }
            _ => self.emit_expr(target),
        }
    }

    /// `e` as a Lua expression safe to repeat: `e` itself when it is a name or
    /// literal, else a fresh local bound to it.
    fn emit_once(&mut self, e: &ast::Expr) -> String {
        let v = self.emit_expr(e);
        if is_repeatable(e) {
            return v;
        }
        self.place_seq += 1;
        let tmp = format!("__place_{}", self.place_seq);
        self.line(format!("local {tmp} = {v}"));
        tmp
    }

    /// A block's statements without a trailing return (statement context).
    fn emit_block_stmts(&mut self, block: &ast::Block) {
        self.emit_stmts(block);
        if let Some(value) = &block.value {
            // A trailing value in statement context is evaluated for effect.
            self.emit_expr_stmt(value);
        }
    }

    /// Emit a loop body inside an already-opened `for`/`while ... do`, tracking
    /// the loop on the frame stack so `break`/`continue` lower against it. Lua
    /// 5.1 has neither `continue` nor `goto`, so a body that uses `continue` is
    /// restructured with the repeat/break idiom (§8): the body runs inside
    /// `repeat … until true`, `continue` becomes a `break` of that `repeat`, and
    /// a neoc `break` sets an outer-break flag and breaks the `repeat`, which an
    /// `if <flag> then break end` after the `repeat` turns into the real loop
    /// exit. A loop with no `continue` stays a plain body.
    fn emit_loop_body(&mut self, body: &ast::Block) {
        if body_uses_continue(body) {
            self.break_flag_seq += 1;
            let flag = format!("__brk_{}", self.break_flag_seq);
            self.indented(|em| {
                em.line(format!("local {flag} = false"));
                em.line("repeat");
                em.loops.push(crate::LoopFrame {
                    break_flag: Some(flag.clone()),
                });
                em.indented(|em| em.emit_block_stmts(body));
                em.loops.pop();
                em.line("until true");
                em.line(format!("if {flag} then break end"));
            });
        } else {
            self.loops.push(crate::LoopFrame { break_flag: None });
            self.indented(|em| em.emit_block_stmts(body));
            self.loops.pop();
        }
    }

    /// Lower a neoc `break`. In a plain loop it is Lua `break`; in a loop
    /// restructured for `continue` it sets the outer-break flag and breaks the
    /// inner `repeat`, so the `if <flag> then break end` after it exits the loop.
    pub(crate) fn emit_break(&mut self) {
        if self.loop_is_outside() {
            self.emit_loop_exit(crate::flow::EXIT_BREAK);
            return;
        }
        match self.loops.last().and_then(|f| f.break_flag.clone()) {
            Some(flag) => self.line(format!("do {flag} = true; break end")),
            None => self.line("break"),
        }
    }

    /// Lower `continue`. Only reachable inside a loop restructured for it
    /// (`emit_loop_body` scans the body first), where it is a `break` of the
    /// inner `repeat`, skipping to the next iteration.
    pub(crate) fn emit_continue(&mut self) {
        if self.loop_is_outside() {
            self.emit_loop_exit(crate::flow::EXIT_CONTINUE);
        } else if self.loops.last().is_some_and(|f| f.break_flag.is_some()) {
            self.line("break");
        } else {
            self.error("`continue` outside a loop");
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
            let hi = if *inclusive {
                hi
            } else {
                format!("({hi}) - 1")
            };
            self.line(format!("for {var} = {lo}, {hi} do"));
        } else {
            let it = self.emit_expr(iter);
            self.line(format!("for _, {var} in ipairs({it}) do"));
        }
        self.emit_loop_body(body);
        self.line("end");
    }

    /// Lower a range used as a value (outside a `for` head) to its list of
    /// numbers (§4.3). Lua has no comprehension, so build the table in an IIFE.
    /// The `for` head lowers to numeric bounds with no allocation (see
    /// `emit_for`); the inclusive/exclusive split mirrors it.
    fn emit_range_value(&mut self, start: &ast::Expr, end: &ast::Expr, inclusive: bool) -> String {
        let (lo, hi) = self.emit_pair(start, end);
        let hi = if inclusive { hi } else { format!("({hi}) - 1") };
        format!(
            "(function() local __r = {{}} for __i = {lo}, {hi} do __r[#__r + 1] = __i end return __r end)()"
        )
    }

    fn emit_if_stmt(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if {c} then"));
        self.indented(|em| em.emit_block_stmts(then));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                // `else if` chain.
                let inner = self.render(|em| em.emit_if_stmt(inner));
                // Re-emit as `elseif` by splicing; simplest: emit `else` + nested.
                self.line("else");
                for l in inner {
                    self.line(l);
                }
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else");
                self.indented(|em| em.emit_block_stmts(b));
            }
            Some(other) => {
                self.line("else");
                self.indented(|em| em.emit_expr_stmt(other));
            }
            None => {}
        }
        self.line("end");
    }

    /// Emit a Lua expression for `e`. Block-like forms become IIFEs.
    pub(crate) fn emit_expr(&mut self, e: &ast::Expr) -> String {
        match &e.kind {
            ast::ExprKind::Int(s) | ast::ExprKind::Float(s) => normalize_number(s),
            ast::ExprKind::Str(s) => lua_string(s),
            ast::ExprKind::Bool(b) => b.to_string(),
            ast::ExprKind::Unit => "nil".to_string(),
            ast::ExprKind::Path(segments) => self.emit_path(segments),
            ast::ExprKind::Binary { op, lhs, rhs } => {
                if matches!(op, ast::BinOp::And | ast::BinOp::Or) {
                    return self.emit_short_circuit(*op, lhs, rhs);
                }
                let (l, r) = self.emit_pair(lhs, rhs);
                format!("({l} {} {r})", bin_op(*op))
            }
            ast::ExprKind::Unary { op, operand } => {
                let o = self.emit_expr(operand);
                let sym = match op {
                    ast::UnOp::Not => "not ",
                    ast::UnOp::Neg => "-",
                };
                format!("({sym}{o})")
            }
            ast::ExprKind::Call { callee, args } => {
                if let ast::ExprKind::Path(segments) = &callee.kind {
                    if let Some(PathKind::Variant(en, var, _)) = self.resolve_path(segments) {
                        return self.emit_tuple_variant(&en, &var, args);
                    }
                    // A call to a single-segment tuple/newtype struct name
                    // constructs through its generated `.new` (§4.4).
                    if let [name] = segments.as_slice() {
                        if self.is_tuple_struct(name) {
                            let a = self.emit_args(args);
                            return format!("{}.new({a})", self.type_local(name));
                        }
                        // A bare call to a free extern `fn` declared inside an
                        // extern `mod` qualifies through the `mod`'s binding
                        // (`serve(opts)` → `Bun.serve(opts)`), so it reads the
                        // imported module table / global root rather than a nil
                        // bare global (`lang/EXTERN.md` §Lowering).
                        if let Some(qualified) = self.qualify_extern_fn(name) {
                            let a = self.emit_args(args);
                            return format!("{qualified}({a})");
                        }
                    }
                }
                let (f, a) = self.emit_call_operands(callee, args);
                format!("{f}({})", a.join(", "))
            }
            ast::ExprKind::Field { base, field } => {
                let b = self.emit_expr(base);
                field_access(&b, field)
            }
            ast::ExprKind::Index { base, index } => {
                let (b, i) = self.emit_pair(base, index);
                index_access(&b, &i)
            }
            ast::ExprKind::StructLit { path, fields, base } => {
                self.emit_struct_lit(path, fields, base.as_deref())
            }
            ast::ExprKind::List(items) => {
                let items: Vec<&ast::Expr> = items.iter().collect();
                format!("{{ {} }}", self.emit_operands(&items).join(", "))
            }
            ast::ExprKind::If { .. } | ast::ExprKind::Block(_) => self.emit_block_like_value(e),
            ast::ExprKind::Closure { params, body } => self.emit_closure(params, body),
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span: _,
                args,
            } => {
                let mut ops = vec![&**receiver];
                ops.extend(args);
                let mut lua = self.emit_operands(&ops).into_iter();
                let recv = lua.next().unwrap_or_default();
                format!("{recv}:{method}({})", lua.collect::<Vec<_>>().join(", "))
            }
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_value(scrutinee, arms),
            ast::ExprKind::Try(inner) => self.emit_try(inner),
            ast::ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.emit_range_value(start, end, *inclusive),
            ast::ExprKind::Assign { .. } => {
                self.error("assignment is a statement, not an expression value");
                "nil".to_string()
            }
            ast::ExprKind::While { .. }
            | ast::ExprKind::For { .. }
            | ast::ExprKind::Loop { .. } => {
                self.error("loops do not produce a value");
                "nil".to_string()
            }
            ast::ExprKind::Return(_) | ast::ExprKind::Break | ast::ExprKind::Continue => {
                self.error("control-flow expression used in value position");
                "nil".to_string()
            }
        }
    }

    fn emit_path(&mut self, segments: &[ast::Ident]) -> String {
        match self.resolve_path(segments) {
            Some(PathKind::Value(name)) => {
                self.note_prelude(&name);
                // A free extern `fn` used as a value reads its qualified target
                // identifier, as a call to it does.
                self.qualify_extern_fn(&name).unwrap_or(name)
            }
            // `Type::member` reads the type's table — keep the qualifier so the
            // associated function/const resolves against `Type`, not a bare name.
            // An extern struct inside a `mod` qualifies through the `mod`'s
            // `require` binding (`sqlite.Database.new`); a local impl type or a
            // `mod` head passes through unchanged.
            Some(PathKind::Assoc(ty, name)) => {
                // `mod::fn` maps through the fn's `name` override.
                let member = self
                    .extern_target_names
                    .get(&format!("{ty}::{name}"))
                    .unwrap_or(&name);
                format!("{}.{member}", self.qualify_extern_value(&ty))
            }
            Some(PathKind::Variant(en, var, VariantShape::Unit)) => {
                self.note_prelude(&en);
                self.variant_table(&en, format!("{{ __enum = \"{en}\", __tag = \"{var}\" }}"))
            }
            // A tuple/struct variant in path position (no payload applied) is a
            // constructor function. Without first-class enum constructors in the
            // emitter there is nothing to bind it to, so the type checker should
            // have rewritten or rejected it.
            Some(PathKind::Variant(_, _, _)) => {
                self.error(format!(
                    "enum variant `{}` needs its payload to construct a value",
                    segments.join("::")
                ));
                "nil".to_string()
            }
            None => {
                self.error(format!(
                    "unresolved path `{}`: not a value or enum variant",
                    segments.join("::")
                ));
                "nil".to_string()
            }
        }
    }

    /// Resolve a `::`-joined path against the symbol table. A single segment is
    /// a value name unless a two-segment `Enum::Variant` matches a collected
    /// enum; longer paths treat the final two segments as `Enum::Variant` and
    /// fall back to the last segment as a value otherwise.
    pub(crate) fn resolve_path(&self, segments: &[ast::Ident]) -> Option<PathKind> {
        match segments {
            [] => None,
            // A single segment is a value name. Prelude constructors
            // (`Ok`/`Err`/`Some`) stay function calls and `None` a value, per
            // DYNAMIC.md §4.4/§7 — construction is not inlined here.
            [one] => Some(PathKind::Value(one.clone())),
            [.., en, var] => self
                .variant_shape(en, var)
                .map(|shape| PathKind::Variant(en.clone(), var.clone(), shape))
                .or_else(|| {
                    self.assoc_types
                        .contains(en)
                        .then(|| PathKind::Assoc(en.clone(), var.clone()))
                })
                .or_else(|| Some(PathKind::Value(var.clone()))),
        }
    }

    /// The payload shape of `Enum::Variant`, or `None` if `enum_name` is not a
    /// known enum with a variant named `variant`.
    pub(crate) fn variant_shape(&self, enum_name: &str, variant: &str) -> Option<VariantShape> {
        self.enums.variant_shape(enum_name, variant)
    }

    /// Resolve a path *in pattern position* to the variant it matches:
    /// `(enum, variant, shape)`. Unlike [`resolve_path`], a single-segment
    /// unqualified prelude variant (`Ok`/`Err`/`Some`/`None`) resolves here, so
    /// `match` tests its `__tag`. A qualified `Enum::Variant` resolves via the
    /// enum index. Returns `None` when the path is not a known variant (then the
    /// caller treats it as a struct pattern or a fresh binding).
    pub(crate) fn pattern_variant(
        &self,
        segments: &[ast::Ident],
    ) -> Option<(String, String, VariantShape)> {
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

    /// Mark the Result/Option prelude as needed when `name` is one of its
    /// constructors or carrier enums.
    pub(crate) fn note_prelude(&mut self, name: &str) {
        if matches!(name, "Ok" | "Err" | "Some" | "None" | "Result" | "Option") {
            self.uses_result = true;
        }
    }

    fn emit_args(&mut self, args: &[ast::Expr]) -> String {
        let args: Vec<&ast::Expr> = args.iter().collect();
        self.emit_operands(&args).join(", ")
    }

    /// Lower a closure as a Lua function. Its body is a function body: a
    /// `return` or `?` in it leaves the closure, not an enclosing function
    /// (`lang/DYNAMIC.md` §4.9).
    fn emit_closure(&mut self, params: &[ast::ClosureParam], body: &ast::Expr) -> String {
        let ps: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let open = format!("function({})", ps.join(", "));
        let lines = self.in_function(|em| {
            em.render(|em| match &body.kind {
                ast::ExprKind::Block(b) => em.emit_block_as_body(b),
                _ => em.emit_tail(body),
            })
        });
        match lines.as_slice() {
            [one] if !matches!(body.kind, ast::ExprKind::Block(_)) => format!("{open} {one} end"),
            _ => Self::wrap_lines(&open, lines, "end"),
        }
    }

    fn emit_struct_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        base: Option<&ast::Expr>,
    ) -> String {
        // An `Enum::Variant { … }` path names a struct-shaped enum variant.
        if let Some(PathKind::Variant(en, var, VariantShape::Named)) = self.resolve_path(path) {
            self.note_prelude(&en);
            let body = self.emit_field_inits(fields);
            return self.variant_table(
                &en,
                format!("{{ __enum = \"{en}\", __tag = \"{var}\", {body} }}"),
            );
        }
        let Some(name) = path.last() else {
            return "nil".to_string();
        };
        // An extern struct literal *is* the target object: a bare table, no
        // `__struct` tag and no `.new` constructor (`lang/EXTERN.md` §Lowering).
        if self.is_extern_struct(name) {
            let body = self.emit_field_inits(fields);
            return format!("{{ {body} }}");
        }
        if let Some(base) = base {
            return self.emit_struct_update(name, fields, base);
        }
        let body = self.emit_field_inits(fields);
        let local = self.type_local(name);
        format!("{local}.new({{ {body} }})")
    }

    /// Whether `name` is a struct declared in an `extern` block.
    pub(crate) fn is_extern_struct(&self, name: &str) -> bool {
        self.extern_structs.contains(name)
    }

    /// Qualify an extern struct's value reference through its enclosing `mod`'s
    /// `require` binding (`Database` in `mod sqlite` → `sqlite.Database`), so the
    /// reference reads the imported module table rather than a nil bare global
    /// (`lang/EXTERN.md` §Lowering). Any other type reads its local
    /// ([`type_local`](Self::type_local)): its bare name, or a shadowed type
    /// import's fresh local (`lang/DYNAMIC.md` §3.7).
    pub(crate) fn qualify_extern_value(&self, ty: &str) -> String {
        match self.extern_struct_ns.get(ty) {
            Some(ns) => format!("{ns}.{}", self.extern_target(ty)),
            None => match self.extern_target_names.get(ty) {
                Some(target) => target.clone(),
                None => self.type_local(ty).to_string(),
            },
        }
    }

    /// The local holding `ty`'s runtime table: a shadowed type import's fresh
    /// local, else `ty` (`lang/DYNAMIC.md` §3.7).
    pub(crate) fn type_local<'a>(&'a self, ty: &'a str) -> &'a str {
        self.type_renames.get(ty).map_or(ty, |r| r.local.as_str())
    }

    /// Qualify a free extern `fn`'s call through its enclosing `mod`'s binding
    /// (`serve` in `mod Bun` → `Bun.serve`), so the bare call reads the imported
    /// module table / global root rather than a nil bare global (`lang/EXTERN.md`
    /// §Lowering). Returns `None` for a fn with no enclosing `mod` (top-level),
    /// which keeps its bare name.
    pub(crate) fn qualify_extern_fn(&self, name: &str) -> Option<String> {
        match self.extern_fn_ns.get(name) {
            Some(ns) => Some(format!("{ns}.{}", self.extern_target(name))),
            None => self.extern_target_names.get(name).cloned(),
        }
    }

    /// The target identifier of an extern item: its `#[extern(name = "...")]`
    /// override, else its neoc name.
    fn extern_target<'a>(&'a self, name: &'a str) -> &'a str {
        self.extern_target_names
            .get(name)
            .map_or(name, String::as_str)
    }

    /// Lower `S { f: e, .. base }` (functional update). The emitter has no field
    /// list, so it copies `base`'s keys at runtime into a fresh table, overrides
    /// the listed fields, then constructs through `S.new` so the value re-stamps
    /// its `__struct` tag and metatable (§4.4).
    fn emit_struct_update(
        &mut self,
        name: &str,
        fields: &[ast::FieldInit],
        base: &ast::Expr,
    ) -> String {
        let mut values = self.emit_field_values(fields, Some(base)).into_iter();
        let base_lua = values.next().unwrap_or_default();
        let mut body = vec![
            "local __b = {}".to_string(),
            format!("for k, v in pairs({base_lua}) do __b[k] = v end"),
        ];
        for (f, value) in fields.iter().zip(values) {
            body.push(format!("__b.{} = {value}", f.name));
        }
        body.push("return __b".to_string());
        let copy = Self::wrap_lines("(function()", body, "end)()");
        format!("{}.new({copy})", self.type_local(name))
    }

    /// Lower struct-literal / struct-variant fields to `name = value` entries,
    /// expanding field shorthand (`{ x }` → `x = x`).
    fn emit_field_inits(&mut self, fields: &[ast::FieldInit]) -> String {
        let values = self.emit_field_values(fields, None);
        fields
            .iter()
            .zip(values)
            .map(|(f, value)| format!("{} = {value}", f.name))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The Lua value of each field initialiser, in order. `base` (the `..base`
    /// of a functional update) evaluates first; its value is returned ahead of
    /// the fields'.
    fn emit_field_values(
        &mut self,
        fields: &[ast::FieldInit],
        base: Option<&ast::Expr>,
    ) -> Vec<String> {
        let mut ops: Vec<&ast::Expr> = base.into_iter().collect();
        ops.extend(fields.iter().filter_map(|f| f.value.as_ref()));
        let mut lua = self.emit_operands(&ops).into_iter();
        let base = base.map(|_| lua.next().unwrap_or_default());
        let mut values: Vec<String> = base.into_iter().collect();
        for f in fields {
            values.push(match f.value {
                Some(_) => lua.next().unwrap_or_default(),
                None => f.name.clone(),
            });
        }
        values
    }

    /// Lower a tuple-variant construction `E::V(a, …)` to its tagged table.
    fn emit_tuple_variant(&mut self, en: &str, var: &str, args: &[ast::Expr]) -> String {
        self.note_prelude(en);
        let payload = self.emit_args(args);
        let table = if payload.is_empty() {
            format!("{{ __enum = \"{en}\", __tag = \"{var}\" }}")
        } else {
            format!("{{ __enum = \"{en}\", __tag = \"{var}\", {payload} }}")
        };
        self.variant_table(en, table)
    }

    /// Wrap a variant table literal in `setmetatable(<table>, E)` when enum `en`
    /// carries a method table (§3.2), so `:method` dispatch resolves. A plain
    /// enum returns the bare table.
    fn variant_table(&self, en: &str, table: String) -> String {
        if self.enum_has_methods(en) {
            format!("setmetatable({table}, {})", self.type_local(en))
        } else {
            table
        }
    }

    /// Wrap a block-like expression in an IIFE so it yields a value.
    fn emit_block_like_value(&mut self, e: &ast::Expr) -> String {
        self.emit_wrapped(e, |em| match &e.kind {
            ast::ExprKind::If { .. } => em.emit_if_return(e),
            ast::ExprKind::Block(b) => em.emit_block_as_body(b),
            _ => {}
        })
    }

    /// Lower a `match` in value position to an IIFE that binds the scrutinee
    /// once, tests arms in order, and returns the first match's body.
    fn emit_match_value(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) -> String {
        let scrut = self.emit_expr(scrutinee);
        self.emit_wrapped_arms(arms, |em| {
            em.line(format!("local __s = {scrut}"));
            for arm in arms {
                em.emit_match_arm(arm, true);
            }
        })
    }

    /// Lower a `match` in statement position: the scrutinee bound once, arm
    /// bodies lowered for effect, no wrapping IIFE.
    pub(crate) fn emit_match_stmt(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) {
        let scrut = self.emit_expr(scrutinee);
        self.line("do");
        self.indented(|em| {
            em.line(format!("local __s = {scrut}"));
            for arm in arms {
                em.emit_match_arm(arm, false);
            }
        });
        self.line("end");
    }

    /// Emit one `match` arm. The pattern test gates the arm; bindings are
    /// introduced before a guard is evaluated, so the guard sees them:
    ///
    /// ```text
    /// if <pattern test> then
    ///   <binds>
    ///   if <guard> then <body> end   -- guard nested only when present
    /// end
    /// ```
    ///
    /// In value context the body is `return`ed; in statement context it is
    /// lowered for effect.
    fn emit_match_arm(&mut self, arm: &ast::MatchArm, value: bool) {
        let compiled = self.compile_pattern(&arm.pattern, "__s");
        self.line(format!("if {} then", compiled.test()));
        self.indented(|em| {
            for bind in &compiled.binds {
                em.line(bind);
            }
            match &arm.guard {
                Some(guard) => {
                    let g = em.emit_expr(guard);
                    em.line(format!("if {g} then"));
                    em.indented(|em| em.emit_arm_body(&arm.body, value));
                    em.line("end");
                }
                None => em.emit_arm_body(&arm.body, value),
            }
        });
        self.line("end");
    }

    /// Emit a match-arm body: `return <v>` in value context, lowered for effect
    /// in statement context.
    fn emit_arm_body(&mut self, body: &ast::Expr, value: bool) {
        if value {
            self.emit_tail(body);
        } else {
            self.emit_expr_stmt(body);
        }
    }

    /// Lower `e?` (§4.10). The short-circuit must return the carrier from the
    /// *enclosing function*, which a self-contained IIFE cannot do (its `return`
    /// would only exit the wrapper). So the guard is hoisted as statements into
    /// the current buffer — `local __try_N = e; if __try_N.__tag == "Err" or
    /// __try_N.__tag == "None" then return __try_N end` — and the expression value
    /// of `e?` is the unwrapped payload `__try_N[1]`. The `return` then belongs
    /// to the surrounding function body, exactly as the operator requires.
    fn emit_try(&mut self, inner: &ast::Expr) -> String {
        let value = self.emit_expr(inner);
        let tmp = format!("__try_{}", self.try_seq);
        self.try_seq += 1;
        self.line(format!("local {tmp} = {value}"));
        let ret = self.return_stmt(Some(tmp.clone()));
        self.line(format!(
            "if {tmp}.__tag == \"Err\" or {tmp}.__tag == \"None\" then {ret} end"
        ));
        format!("{tmp}[1]")
    }

    /// Emit an `if` whose branches `return` their value (for IIFE bodies).
    fn emit_if_return(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if {c} then"));
        self.indented(|em| em.emit_block_as_body(then));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                self.line("else");
                self.indented(|em| em.emit_if_return(inner));
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else");
                self.indented(|em| em.emit_block_as_body(b));
            }
            Some(other) => {
                self.line("else");
                self.indented(|em| em.emit_tail(other));
            }
            None => {}
        }
        self.line("end");
    }

    fn emit_fn_local(&mut self, f: &ast::FnDecl, keyword: &str) {
        let params: Vec<String> = f
            .sig
            .params
            .iter()
            .map(|p| match p {
                ast::Param::Named { name, .. } => name.clone(),
                ast::Param::Receiver => "self".to_string(),
            })
            .collect();
        self.line(format!("{keyword} {}({})", f.sig.name, params.join(", ")));
        self.in_function(|em| em.indented(|em| em.emit_block_as_body(&f.body)));
        self.line("end");
    }

    /// Run `f`, capturing the lines it emits into a separate buffer (used to
    /// build IIFE bodies and nested blocks). Indentation resets to zero inside.
    pub(crate) fn render<F: FnOnce(&mut Self)>(&mut self, f: F) -> Vec<String> {
        let saved_lines = std::mem::take(&mut self.lines);
        let saved_origins = std::mem::take(&mut self.line_origins);
        let saved_indent = std::mem::replace(&mut self.indent, 0);
        f(self);
        let captured = std::mem::replace(&mut self.lines, saved_lines);
        // Captured origins are discarded: callers re-emit the captured lines via
        // `line()` (which re-tags with the then-current origin) or fold them into
        // an IIFE string passed through one `line()` call. Either way the origin
        // is reassigned at re-emit, so the buffer here just needs restoring.
        self.line_origins = saved_origins;
        self.indent = saved_indent;
        captured
    }
}

/// Whether `block` contains a `continue` that belongs to the loop it heads —
/// i.e. one not nested inside another loop (which owns its own `continue`) or a
/// closure (a separate function). Used to decide whether a loop body needs the
/// repeat/break restructuring (§8).
fn body_uses_continue(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_uses_continue)
        || block.value.as_deref().is_some_and(expr_uses_continue)
}

fn stmt_uses_continue(stmt: &ast::Stmt) -> bool {
    match &stmt.kind {
        ast::StmtKind::Let(l) => l.value.as_ref().is_some_and(expr_uses_continue),
        ast::StmtKind::Expr(e) => expr_uses_continue(e),
        // A nested item (e.g. a local fn) is its own scope; ignore it.
        ast::StmtKind::Item(_) => false,
    }
}

fn expr_uses_continue(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Continue => true,
        // Leaves, plus nested loops and closures, which own any `continue`
        // inside them rather than the loop being scanned.
        ast::ExprKind::While { .. }
        | ast::ExprKind::For { .. }
        | ast::ExprKind::Loop { .. }
        | ast::ExprKind::Closure { .. }
        | ast::ExprKind::Int(_)
        | ast::ExprKind::Float(_)
        | ast::ExprKind::Str(_)
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Unit
        | ast::ExprKind::Path(_)
        | ast::ExprKind::Break
        | ast::ExprKind::Return(None) => false,
        ast::ExprKind::Binary { lhs, rhs, .. } => {
            expr_uses_continue(lhs) || expr_uses_continue(rhs)
        }
        ast::ExprKind::Unary { operand, .. } => expr_uses_continue(operand),
        ast::ExprKind::Range { start, end, .. } => {
            expr_uses_continue(start) || expr_uses_continue(end)
        }
        ast::ExprKind::Try(inner) | ast::ExprKind::Return(Some(inner)) => expr_uses_continue(inner),
        ast::ExprKind::Call { callee, args } => {
            expr_uses_continue(callee) || args.iter().any(expr_uses_continue)
        }
        ast::ExprKind::MethodCall { receiver, args, .. } => {
            expr_uses_continue(receiver) || args.iter().any(expr_uses_continue)
        }
        ast::ExprKind::Field { base, .. } => expr_uses_continue(base),
        ast::ExprKind::Index { base, index } => {
            expr_uses_continue(base) || expr_uses_continue(index)
        }
        ast::ExprKind::StructLit { fields, base, .. } => {
            fields
                .iter()
                .any(|f| f.value.as_ref().is_some_and(expr_uses_continue))
                || base.as_deref().is_some_and(expr_uses_continue)
        }
        ast::ExprKind::List(items) => items.iter().any(expr_uses_continue),
        ast::ExprKind::Assign { target, value, .. } => {
            expr_uses_continue(target) || expr_uses_continue(value)
        }
        ast::ExprKind::If { cond, then, else_ } => {
            expr_uses_continue(cond)
                || body_uses_continue(then)
                || else_.as_deref().is_some_and(expr_uses_continue)
        }
        ast::ExprKind::Match { scrutinee, arms } => {
            expr_uses_continue(scrutinee)
                || arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(expr_uses_continue) || expr_uses_continue(&a.body)
                })
        }
        ast::ExprKind::Block(b) => body_uses_continue(b),
    }
}

/// Whether evaluating `e` twice is indistinguishable from evaluating it once:
/// a name or a literal. A field read is not: on an `extern` struct it may be a
/// host getter.
fn is_repeatable(e: &ast::Expr) -> bool {
    matches!(
        e.kind,
        ast::ExprKind::Path(_)
            | ast::ExprKind::Int(_)
            | ast::ExprKind::Float(_)
            | ast::ExprKind::Str(_)
            | ast::ExprKind::Bool(_)
    )
}

/// Lua for field `field` of the Lua value `base`.
fn field_access(base: &str, field: &ast::FieldAccess) -> String {
    match field {
        ast::FieldAccess::Named(n, _) => format!("{base}.{n}"),
        // Tuple/newtype payload is stored positionally (Lua arrays are 1-based),
        // so `.0` reads `[1]`. Matches the constructor (`N.new` stores at `[1]`)
        // and the derive/pattern lowering.
        ast::FieldAccess::Tuple(i) => format!("{base}[{}]", i + 1),
    }
}

/// Lua for element `index` of the Lua list `base` (neoc lists are 0-based).
fn index_access(base: &str, index: &str) -> String {
    format!("{base}[({index}) + 1]")
}

pub(crate) fn bin_op(op: ast::BinOp) -> &'static str {
    match op {
        ast::BinOp::Or => "or",
        ast::BinOp::And => "and",
        ast::BinOp::Eq => "==",
        ast::BinOp::Ne => "~=",
        ast::BinOp::Lt => "<",
        ast::BinOp::Gt => ">",
        ast::BinOp::Le => "<=",
        ast::BinOp::Ge => ">=",
        // `+` is emitted as numeric addition. String concatenation also uses
        // `+` in neoc but lowers to Lua `..`; distinguishing them needs the
        // type checker, so the concat case is handled there, not here.
        ast::BinOp::Add => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        ast::BinOp::Div => "/",
        ast::BinOp::Rem => "%",
        // Produced by the type checker's elaboration for string-typed `+`.
        ast::BinOp::Concat => "..",
    }
}

/// Normalise a neoc numeric literal to Lua: strip digit separators.
fn normalize_number(s: &str) -> String {
    s.replace('_', "")
}

/// Encode a string value as a Lua double-quoted literal.
pub(crate) fn lua_string(s: &str) -> String {
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
