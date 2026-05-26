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
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            self.at_span(value.span, |em| {
                let v = em.emit_expr(value);
                em.line(format!("return {v}"));
            });
        }
    }

    fn emit_stmt(&mut self, stmt: &ast::Stmt) {
        self.at_span(stmt.span, |em| match &stmt.kind {
            ast::StmtKind::Let(l) => em.emit_let(l),
            ast::StmtKind::Item(item) => em.emit_item_stmt(item),
            ast::StmtKind::Expr(e) => em.emit_expr_stmt(e),
        });
    }

    fn emit_item_stmt(&mut self, item: &ast::Item) {
        // Nested items are rare; only functions are meaningful inside a block.
        if let ast::ItemKind::Fn(f) = &item.kind {
            self.emit_fn_local(f);
        } else {
            self.error("only nested functions are supported inside a block");
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
            self.error(format!(
                "refutable pattern in `let` is not supported: {pattern:?}"
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
    fn emit_expr_stmt(&mut self, e: &ast::Expr) {
        match &e.kind {
            ast::ExprKind::If { .. } => self.emit_if_stmt(e),
            ast::ExprKind::While { cond, body } => {
                let c = self.emit_expr(cond);
                self.line(format!("while {c} do"));
                self.emit_loop_body(body);
                self.line("end");
            }
            ast::ExprKind::Loop { body } => {
                self.line("while true do");
                self.emit_loop_body(body);
                self.line("end");
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.emit_for(pattern, iter, body),
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_stmt(scrutinee, arms),
            ast::ExprKind::Block(b) => {
                self.line("do");
                self.indented(|em| em.emit_block_stmts(b));
                self.line("end");
            }
            ast::ExprKind::Return(value) => {
                let r = value
                    .as_ref()
                    .map_or_else(String::new, |v| format!(" {}", self.emit_expr(v)));
                self.line(format!("return{r}"));
            }
            ast::ExprKind::Break => self.emit_break(),
            ast::ExprKind::Continue => self.emit_continue(),
            ast::ExprKind::Assign { target, op, value } => {
                let t = self.emit_expr(target);
                let v = self.emit_expr(value);
                // Lua has no compound assignment; expand `a += b` to `a = a + b`.
                let rhs = match op {
                    ast::AssignOp::Assign => v,
                    ast::AssignOp::Add => format!("{t} + {v}"),
                    ast::AssignOp::Sub => format!("{t} - {v}"),
                    ast::AssignOp::Mul => format!("{t} * {v}"),
                    ast::AssignOp::Div => format!("{t} / {v}"),
                    ast::AssignOp::Rem => format!("{t} % {v}"),
                };
                self.line(format!("{t} = {rhs}"));
            }
            _ => {
                let s = self.emit_expr(e);
                self.line(s);
            }
        }
    }

    /// A block's statements without a trailing return (statement context).
    fn emit_block_stmts(&mut self, block: &ast::Block) {
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
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
    fn emit_break(&mut self) {
        match self.loops.last().and_then(|f| f.break_flag.clone()) {
            Some(flag) => self.line(format!("do {flag} = true; break end")),
            None => self.line("break"),
        }
    }

    /// Lower `continue`. Only reachable inside a loop restructured for it
    /// (`emit_loop_body` scans the body first), where it is a `break` of the
    /// inner `repeat`, skipping to the next iteration.
    fn emit_continue(&mut self) {
        if self.loops.last().is_some_and(|f| f.break_flag.is_some()) {
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
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
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
                            return format!("{name}.new({a})");
                        }
                    }
                }
                let f = self.emit_expr(callee);
                let a = self.emit_args(args);
                format!("{f}({a})")
            }
            ast::ExprKind::Field { base, field } => {
                let b = self.emit_expr(base);
                match field {
                    ast::FieldAccess::Named(n) => format!("{b}.{n}"),
                    // Tuple/newtype payload is stored positionally (Lua arrays
                    // are 1-based), so `.0` reads `[1]`. Matches the constructor
                    // (`N.new` stores at `[1]`) and the derive/pattern lowering.
                    ast::FieldAccess::Tuple(i) => format!("{b}[{}]", i + 1),
                }
            }
            ast::ExprKind::Index { base, index } => {
                let b = self.emit_expr(base);
                let i = self.emit_expr(index);
                format!("{b}[({i}) + 1]")
            }
            ast::ExprKind::StructLit { path, fields, base } => {
                self.emit_struct_lit(path, fields, base.as_deref())
            }
            ast::ExprKind::List(items) => {
                let parts: Vec<String> = items.iter().map(|i| self.emit_expr(i)).collect();
                format!("{{ {} }}", parts.join(", "))
            }
            ast::ExprKind::If { .. } | ast::ExprKind::Block(_) => self.emit_block_like_value(e),
            ast::ExprKind::Closure { params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let b = self.emit_expr(body);
                format!("function({}) return {b} end", ps.join(", "))
            }
            ast::ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv = self.emit_expr(receiver);
                let a = self.emit_args(args);
                format!("{recv}:{method}({a})")
            }
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_value(scrutinee, arms),
            ast::ExprKind::Try(inner) => self.emit_try(inner),
            ast::ExprKind::Range { .. } => {
                self.error("range expressions are only supported as a `for` iterable");
                "nil".to_string()
            }
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
                name
            }
            // `Type::member` reads the type's table — keep the qualifier so the
            // associated function/const resolves against `Type`, not a bare name.
            Some(PathKind::Assoc(ty, name)) => format!("{ty}.{name}"),
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
        args.iter()
            .map(|a| self.emit_expr(a))
            .collect::<Vec<_>>()
            .join(", ")
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
        if let Some(base) = base {
            return self.emit_struct_update(name, fields, base);
        }
        let body = self.emit_field_inits(fields);
        format!("{name}.new({{ {body} }})")
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
        let base_lua = self.emit_expr(base);
        let mut body = vec![
            "local __b = {}".to_string(),
            format!("for k, v in pairs({base_lua}) do __b[k] = v end"),
        ];
        for f in fields {
            let value = match &f.value {
                Some(v) => self.emit_expr(v),
                None => f.name.clone(),
            };
            body.push(format!("__b.{} = {value}", f.name));
        }
        body.push("return __b".to_string());
        let copy = Self::wrap_iife(body);
        format!("{name}.new({copy})")
    }

    /// Lower struct-literal / struct-variant fields to `name = value` entries,
    /// expanding field shorthand (`{ x }` → `x = x`).
    fn emit_field_inits(&mut self, fields: &[ast::FieldInit]) -> String {
        fields
            .iter()
            .map(|f| {
                let value = match &f.value {
                    Some(v) => self.emit_expr(v),
                    None => f.name.clone(),
                };
                format!("{} = {value}", f.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
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
            format!("setmetatable({table}, {en})")
        } else {
            table
        }
    }

    /// Wrap captured body lines in an IIFE that yields a value.
    fn wrap_iife(body: Vec<String>) -> String {
        let mut out = String::from("(function()\n");
        for l in body {
            if l.is_empty() {
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(&l);
                out.push('\n');
            }
        }
        out.push_str("end)()");
        out
    }

    /// Wrap a block-like expression in an IIFE so it yields a value.
    fn emit_block_like_value(&mut self, e: &ast::Expr) -> String {
        let body = self.render(|em| match &e.kind {
            ast::ExprKind::If { .. } => em.emit_if_return(e),
            ast::ExprKind::Block(b) => em.emit_block_as_body(b),
            _ => {}
        });
        Self::wrap_iife(body)
    }

    /// Lower a `match` in value position to an IIFE that binds the scrutinee
    /// once, tests arms in order, and returns the first match's body.
    fn emit_match_value(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) -> String {
        let scrut = self.emit_expr(scrutinee);
        let body = self.render(|em| {
            em.line(format!("local __s = {scrut}"));
            for arm in arms {
                em.emit_match_arm(arm, true);
            }
        });
        Self::wrap_iife(body)
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
            let v = self.emit_expr(body);
            self.line(format!("return {v}"));
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
        self.line(format!(
            "if {tmp}.__tag == \"Err\" or {tmp}.__tag == \"None\" then return {tmp} end"
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
                let v = self.emit_expr(other);
                self.line("else");
                self.indented(|em| em.line(format!("return {v}")));
            }
            None => {}
        }
        self.line("end");
    }

    fn emit_fn_local(&mut self, f: &ast::FnDecl) {
        let params: Vec<String> = f
            .sig
            .params
            .iter()
            .map(|p| match p {
                ast::Param::Named { name, .. } => name.clone(),
                ast::Param::Receiver => "self".to_string(),
            })
            .collect();
        self.line(format!(
            "local function {}({})",
            f.sig.name,
            params.join(", ")
        ));
        self.indented(|em| em.emit_block_as_body(&f.body));
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

fn bin_op(op: ast::BinOp) -> &'static str {
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
