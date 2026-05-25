//! Expression, statement, and block lowering for the Python backend.
//!
//! `emit_expr` returns a Python *expression* string. neoc is expression-oriented
//! but Python statements (`if`, loops, `match`) cannot appear in expression
//! position, so a block-like form used for its value is hoisted to a nested
//! helper `def` whose body `return`s the value, and `emit_expr` yields a call to
//! it (the analogue of codegen-lua's IIFE). Statement-context lowering pushes
//! lines via the emitter's line buffer.
//!
//! Enum construction, `match`, and `?` consult the [`crate::EnumIndex`] to
//! resolve variant paths; method calls lower to ordinary Python method calls.
//! The one form needing the type checker is `+` on `string` — it lowers to
//! Python `+` only after the checker rewrites the operator to
//! [`ast::BinOp::Concat`] (which Python `+` already concatenates).

use crate::Emitter;
use neoc_ast as ast;

/// What a `::`-joined path resolves to at a use site.
pub(crate) enum PathKind {
    /// The path is a value name (local, function, const, prelude ctor).
    Value(String),
    /// The path names an enum variant: `(enum name, variant name, shape)`.
    Variant(String, String, VariantShape),
}

/// The runtime payload shape of an enum variant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariantShape {
    Unit,
    Tuple,
    Named,
}

impl Emitter {
    /// Emit a block as a function/method body: statements, then `return <value>`
    /// when the block has a trailing value. An empty body emits `pass`.
    pub(crate) fn emit_block_as_body(&mut self, block: &ast::Block) {
        let start = self.line_count();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            let v = self.emit_expr(value);
            self.line(format!("return {v}"));
        }
        if self.line_count() == start {
            self.line("pass");
        }
    }

    /// The number of lines emitted so far (used to detect an empty body).
    fn line_count(&self) -> usize {
        self.lines.len()
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
        let name = match &l.pattern {
            ast::Pattern::Binding(name) => Some(name.clone()),
            ast::Pattern::Wildcard => Some("_".to_string()),
            _ => None,
        };
        if let Some(name) = name {
            // An uninitialised binding has no Python statement; later assignment
            // introduces it.
            if let Some(v) = &l.value {
                let value = self.emit_expr(v);
                self.line(format!("{name} = {value}"));
            }
            return;
        }
        self.emit_let_destructure(&l.pattern, l.value.as_ref());
    }

    /// Lower an irrefutable destructuring `let`: a struct (`let Point { x, y } =
    /// p`) or newtype/tuple-struct (`let Wrapper(inner) = w`) pattern. The
    /// initialiser is bound to a temp once, then each variable is bound from a
    /// projection of it, reusing the bind logic in [`crate::pat`].
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
        let tmp = self.fresh_tmp();
        self.line(format!("{tmp} = {init}"));
        let compiled = self.compile_pattern(pattern, &tmp);
        for bind in &compiled.binds {
            self.line(bind);
        }
    }

    /// Whether `pattern` is irrefutable in `let`: a wildcard, a binding, or a
    /// struct/newtype/tuple-struct destructure whose sub-patterns are all
    /// irrefutable.
    fn is_irrefutable_let_pattern(&self, pattern: &ast::Pattern) -> bool {
        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Binding(_) => true,
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

    /// An expression used for effect. Block-like forms lower as statements;
    /// everything else is a bare expression line.
    fn emit_expr_stmt(&mut self, e: &ast::Expr) {
        match &e.kind {
            ast::ExprKind::If { .. } => self.emit_if_stmt(e),
            ast::ExprKind::While { cond, body } => {
                let c = self.emit_expr(cond);
                self.line(format!("while {c}:"));
                self.emit_loop_body(body);
            }
            ast::ExprKind::Loop { body } => {
                self.line("while True:");
                self.emit_loop_body(body);
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.emit_for(pattern, iter, body),
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_stmt(scrutinee, arms),
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

    /// A block's statements without a trailing return (statement context).
    fn emit_block_stmts(&mut self, block: &ast::Block) {
        let start = self.line_count();
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            self.emit_expr_stmt(value);
        }
        if self.line_count() == start {
            self.line("pass");
        }
    }

    /// Emit a loop body. Python has native `break`/`continue`, so the body
    /// lowers directly with no restructuring.
    fn emit_loop_body(&mut self, body: &ast::Block) {
        self.indented(|em| em.emit_block_stmts(body));
    }

    fn emit_for(&mut self, pattern: &ast::Pattern, iter: &ast::Expr, body: &ast::Block) {
        let var = match pattern {
            ast::Pattern::Binding(name) => name.clone(),
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
                format!("({hi}) + 1")
            } else {
                hi
            };
            self.line(format!("for {var} in range({lo}, {hi}):"));
        } else {
            let it = self.emit_expr(iter);
            self.line(format!("for {var} in {it}:"));
        }
        self.emit_loop_body(body);
    }

    fn emit_if_stmt(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if {c}:"));
        self.indented(|em| em.emit_block_stmts(then));
        match else_.as_deref() {
            Some(ast::Expr {
                kind:
                    ast::ExprKind::If {
                        cond: ec,
                        then: et,
                        else_: ee,
                    },
                ..
            }) => {
                // Collapse `else: if …` into `elif …`.
                self.emit_elif_chain(ec, et, ee.as_deref());
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else:");
                self.indented(|em| em.emit_block_stmts(b));
            }
            Some(other) => {
                self.line("else:");
                self.indented(|em| em.emit_expr_stmt(other));
            }
            None => {}
        }
    }

    /// Emit `elif <cond>:` and its branch, recursing into a further `else`.
    fn emit_elif_chain(&mut self, cond: &ast::Expr, then: &ast::Block, else_: Option<&ast::Expr>) {
        let c = self.emit_expr(cond);
        self.line(format!("elif {c}:"));
        self.indented(|em| em.emit_block_stmts(then));
        match else_ {
            Some(ast::Expr {
                kind:
                    ast::ExprKind::If {
                        cond: ec,
                        then: et,
                        else_: ee,
                    },
                ..
            }) => self.emit_elif_chain(ec, et, ee.as_deref()),
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else:");
                self.indented(|em| em.emit_block_stmts(b));
            }
            Some(other) => {
                self.line("else:");
                self.indented(|em| em.emit_expr_stmt(other));
            }
            None => {}
        }
    }

    /// Emit a Python expression for `e`. Block-like forms become helper calls.
    pub(crate) fn emit_expr(&mut self, e: &ast::Expr) -> String {
        match &e.kind {
            ast::ExprKind::Int(s) | ast::ExprKind::Float(s) => normalize_number(s),
            ast::ExprKind::Str(s) => py_string(s),
            ast::ExprKind::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            ast::ExprKind::Unit => "None".to_string(),
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
                    // Tuple/newtype payload is stored under `_<i>`.
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
            ast::ExprKind::If { .. } | ast::ExprKind::Block(_) | ast::ExprKind::Match { .. } => {
                self.emit_block_like_value(e)
            }
            ast::ExprKind::Closure { params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let b = self.emit_expr(body);
                format!("(lambda {}: {b})", ps.join(", "))
            }
            ast::ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv = self.emit_expr(receiver);
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

    fn emit_path(&mut self, segments: &[ast::Ident]) -> String {
        match self.resolve_path(segments) {
            Some(PathKind::Value(name)) => self.value_name(&name),
            Some(PathKind::Variant(en, var, VariantShape::Unit)) => {
                self.note_runtime(&en);
                self.unit_variant(&en, &var)
            }
            Some(PathKind::Variant(_, _, _)) => {
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

    /// Map a value name to its Python spelling. `None` is the prelude `None_`
    /// (Python's `None` is the unit value, distinct from the `Option` variant).
    fn value_name(&mut self, name: &str) -> String {
        self.note_runtime(name);
        if name == "None" {
            "None_".to_string()
        } else {
            name.to_string()
        }
    }

    /// Resolve a `::`-joined path against the symbol table. A single segment is
    /// a value name unless a two-segment `Enum::Variant` matches a collected
    /// enum; longer paths treat the final two segments as `Enum::Variant` and
    /// fall back to the last segment as a value otherwise.
    pub(crate) fn resolve_path(&self, segments: &[ast::Ident]) -> Option<PathKind> {
        match segments {
            [] => None,
            [one] => Some(PathKind::Value(one.clone())),
            [.., en, var] => self
                .variant_shape(en, var)
                .map(|shape| PathKind::Variant(en.clone(), var.clone(), shape))
                .or_else(|| Some(PathKind::Value(var.clone()))),
        }
    }

    /// The payload shape of `Enum::Variant`, or `None` if `enum_name` is not a
    /// known enum with a variant named `variant`.
    pub(crate) fn variant_shape(&self, enum_name: &str, variant: &str) -> Option<VariantShape> {
        self.enums.variant_shape(enum_name, variant)
    }

    /// Resolve a path *in pattern position* to the variant it matches:
    /// `(enum, variant, shape)`. A single-segment unqualified prelude variant
    /// (`Ok`/`Err`/`Some`/`None`) resolves here; a qualified `Enum::Variant`
    /// resolves via the enum index.
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

    /// Mark the runtime prelude as needed when `name` is a `Result`/`Option`
    /// constructor or carrier enum.
    pub(crate) fn note_runtime(&mut self, name: &str) {
        if matches!(name, "Ok" | "Err" | "Some" | "None" | "Result" | "Option") {
            self.uses_runtime = true;
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
            self.note_runtime(&en);
            let body = self.emit_keyword_fields(fields);
            return self.named_variant(&en, &var, &body);
        }
        let Some(name) = path.last() else {
            return "None".to_string();
        };
        if let Some(base) = base {
            return self.emit_struct_update(name, fields, base);
        }
        let body = self.emit_keyword_fields(fields);
        format!("{name}({body})")
    }

    /// Lower `S { f: e, .. base }` (functional update) to a helper that copies
    /// the base's fields, overrides the listed ones, then constructs `S`.
    fn emit_struct_update(
        &mut self,
        name: &str,
        fields: &[ast::FieldInit],
        base: &ast::Expr,
    ) -> String {
        let base_py = self.emit_expr(base);
        let tmp = self.fresh_tmp();
        let mut overrides: Vec<String> = Vec::new();
        for f in fields {
            let value = match &f.value {
                Some(v) => self.emit_expr(v),
                None => f.name.clone(),
            };
            overrides.push(format!("\"{}\": {value}", f.name));
        }
        // Copy the base instance's fields, override the listed ones, construct.
        let body = vec![
            format!("{tmp} = dict(vars({base_py}))"),
            format!("{tmp}.update({{{}}})", overrides.join(", ")),
            format!("return {name}(**{tmp})"),
        ];
        self.emit_helper_def(body)
    }

    /// Lower struct-literal / struct-variant fields to `name=value` keyword
    /// arguments, expanding field shorthand (`{ x }` → `x=x`).
    fn emit_keyword_fields(&mut self, fields: &[ast::FieldInit]) -> String {
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

    /// Lower a tuple-variant construction `E::V(a, …)`.
    fn emit_tuple_variant(&mut self, en: &str, var: &str, args: &[ast::Expr]) -> String {
        self.note_runtime(en);
        let payload = self.emit_args(args);
        let inner = if payload.is_empty() {
            format!("\"{var}\"")
        } else {
            format!("\"{var}\", {payload}")
        };
        self.wrap_variant(en, &inner)
    }

    /// A unit variant value `E::V`.
    fn unit_variant(&mut self, en: &str, var: &str) -> String {
        self.wrap_variant(en, &format!("\"{var}\""))
    }

    /// A struct-shaped variant value `E::V { … }`.
    fn named_variant(&mut self, en: &str, var: &str, fields: &str) -> String {
        let inner = if fields.is_empty() {
            format!("\"{var}\"")
        } else {
            format!("\"{var}\", {fields}")
        };
        self.wrap_variant(en, &inner)
    }

    /// Build a variant value. A method-carrying enum instantiates its subclass
    /// so `value.method()` dispatches; a plain enum uses the runtime `_Enum`.
    fn wrap_variant(&mut self, en: &str, inner: &str) -> String {
        self.uses_runtime = true;
        if self.enum_has_methods(en) {
            format!("{en}({inner})")
        } else {
            format!("_Enum({inner})")
        }
    }

    /// Lower a block-like expression to a helper `def` returning its value.
    fn emit_block_like_value(&mut self, e: &ast::Expr) -> String {
        let body = self.render(|em| match &e.kind {
            ast::ExprKind::If { .. } => em.emit_if_return(e),
            ast::ExprKind::Block(b) => em.emit_block_as_body(b),
            ast::ExprKind::Match { scrutinee, arms } => em.emit_match_body(scrutinee, arms),
            _ => {}
        });
        self.emit_helper_def(body)
    }

    /// Emit a helper `def` from already-rendered body lines and yield a call.
    fn emit_helper_def(&mut self, body: Vec<String>) -> String {
        self.helper_seq += 1;
        let name = format!("_v{}", self.helper_seq);
        self.line(format!("def {name}():"));
        self.indented(|em| {
            if body.is_empty() {
                em.line("pass");
            } else {
                for l in body {
                    em.line(l);
                }
            }
        });
        format!("{name}()")
    }

    /// Render a `match` body for value context: bind the scrutinee, test arms
    /// in order, `return` the first match's body.
    fn emit_match_body(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) {
        let scrut = self.emit_expr(scrutinee);
        let tmp = self.fresh_tmp();
        self.line(format!("{tmp} = {scrut}"));
        for arm in arms {
            self.emit_match_arm(arm, &tmp, true);
        }
    }

    /// Lower a `match` in statement position: bind the scrutinee, lower each arm
    /// body for effect.
    pub(crate) fn emit_match_stmt(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) {
        let scrut = self.emit_expr(scrutinee);
        let tmp = self.fresh_tmp();
        self.line(format!("{tmp} = {scrut}"));
        for arm in arms {
            self.emit_match_arm(arm, &tmp, false);
        }
    }

    /// Emit one `match` arm against subject `tmp`:
    ///
    /// ```text
    /// if <pattern test>:
    ///     <binds>
    ///     if <guard>:        # only when present
    ///         <body>
    /// ```
    fn emit_match_arm(&mut self, arm: &ast::MatchArm, tmp: &str, value: bool) {
        let compiled = self.compile_pattern(&arm.pattern, tmp);
        self.line(format!("if {}:", compiled.test()));
        self.indented(|em| {
            for bind in &compiled.binds {
                em.line(bind);
            }
            match &arm.guard {
                Some(guard) => {
                    let g = em.emit_expr(guard);
                    em.line(format!("if {g}:"));
                    em.indented(|em| em.emit_arm_body(&arm.body, value));
                }
                None => em.emit_arm_body(&arm.body, value),
            }
        });
    }

    fn emit_arm_body(&mut self, body: &ast::Expr, value: bool) {
        if value {
            let v = self.emit_expr(body);
            self.line(format!("return {v}"));
        } else {
            self.emit_expr_stmt(body);
        }
    }

    /// Lower `e?` (§4.10). The short-circuit must return the carrier from the
    /// *enclosing* function, so the guard is hoisted as statements into the
    /// current buffer — `__try_N = e; if __try_N.tag in ("Err", "None"): return
    /// __try_N` — and the value of `e?` is the unwrapped payload `__try_N._0`.
    fn emit_try(&mut self, inner: &ast::Expr) -> String {
        let value = self.emit_expr(inner);
        let tmp = format!("__try_{}", self.try_seq);
        self.try_seq += 1;
        self.line(format!("{tmp} = {value}"));
        self.line(format!("if {tmp}.tag == \"Err\" or {tmp}.tag == \"None\":"));
        self.indented(|em| em.line(format!("return {tmp}")));
        format!("{tmp}._0")
    }

    /// Emit an `if` whose branches `return` their value (for helper-def bodies).
    fn emit_if_return(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if {c}:"));
        self.indented(|em| em.emit_block_as_body(then));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                self.line("else:");
                self.indented(|em| em.emit_if_return(inner));
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                self.line("else:");
                self.indented(|em| em.emit_block_as_body(b));
            }
            Some(other) => {
                let v = self.emit_expr(other);
                self.line("else:");
                self.indented(|em| em.line(format!("return {v}")));
            }
            None => {}
        }
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
        self.line(format!("def {}({}):", f.sig.name, params.join(", ")));
        self.indented(|em| em.emit_block_as_body(&f.body));
    }

    /// A fresh temporary name (`__t0`, `__t1`, …) for scrutinees and
    /// destructuring binds.
    pub(crate) fn fresh_tmp(&mut self) -> String {
        let n = self.tmp_seq;
        self.tmp_seq += 1;
        format!("__t{n}")
    }

    /// Run `f`, capturing the lines it emits into a separate buffer. Indentation
    /// resets to zero inside.
    pub(crate) fn render<F: FnOnce(&mut Self)>(&mut self, f: F) -> Vec<String> {
        let saved_lines = std::mem::take(&mut self.lines);
        let saved_indent = std::mem::replace(&mut self.indent, 0);
        f(self);
        let captured = std::mem::replace(&mut self.lines, saved_lines);
        self.indent = saved_indent;
        captured
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
        // `Add` is numeric addition; `Concat` (the checker's rewrite of a
        // string-typed `+`) is string concatenation — Python `+` does both.
        ast::BinOp::Add | ast::BinOp::Concat => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        // neoc `number` is f64, so `/` is true division — Python `/` already is.
        ast::BinOp::Div => "/",
        ast::BinOp::Rem => "%",
    }
}

/// Normalise a neoc numeric literal: strip digit separators. An integer literal
/// stays a Python `int` and a fractional literal a `float`; neoc `number` is f64
/// but Python `/` is already true division (`10 / 4` is `2.5`), and `range`
/// needs an `int`, so no `.0` is forced. The value formatter renders both forms
/// to match the goldens.
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
