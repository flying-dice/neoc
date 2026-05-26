//! Expression, statement, and block lowering.
//!
//! `emit_expr` returns a JavaScript expression string (block-like forms become
//! IIFEs so they carry a value). Statement-context lowering pushes lines via the
//! emitter's line buffer. Enum construction, `match`, and `?` consult the
//! [`crate::EnumIndex`] to resolve variant paths; method calls dispatch through
//! the runtime `__call` helper. The one form still needing the type checker is
//! `+` on `string` (it lowers to `+` either way, but the checker rewrites it to
//! [`ast::BinOp::Concat`] so addition and concatenation stay distinct upstream).

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
    /// Emit a block as a function body: statements, then `return <value>` when
    /// the block has a trailing value.
    pub(crate) fn emit_block_as_body(&mut self, block: &ast::Block) {
        for stmt in &block.stmts {
            self.emit_stmt(stmt);
        }
        if let Some(value) = &block.value {
            let v = self.emit_expr(value);
            self.line(format!("return {v};"));
        }
    }

    fn emit_stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Let(l) => self.emit_let(l),
            ast::StmtKind::Item(item) => self.emit_item_stmt(item),
            ast::StmtKind::Expr(e) => self.emit_expr_stmt(e),
        }
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
        // A simple binding or wildcard maps straight to a `let`.
        let name = match &l.pattern {
            ast::Pattern::Binding { name, .. } => Some(name.clone()),
            ast::Pattern::Wildcard => Some("_".to_string()),
            _ => None,
        };
        if let Some(name) = name {
            match &l.value {
                Some(v) => {
                    let value = self.emit_expr(v);
                    self.line(format!("let {name} = {value};"));
                }
                None => self.line(format!("let {name};")),
            }
            return;
        }
        self.emit_let_destructure(&l.pattern, l.value.as_ref());
    }

    /// Lower an irrefutable destructuring `let`: a struct (`let Point { x, y } =
    /// p`) or newtype/tuple-struct (`let Wrapper(inner) = w`) pattern. The
    /// initialiser is bound to a temp once, then each variable is bound from a
    /// projection of it. A refutable pattern (an enum variant with siblings)
    /// stays a diagnostic.
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
        let tmp = self.fresh_tmp("__d");
        self.line(format!("const {tmp} = {init};"));
        let compiled = self.compile_pattern(pattern, &tmp);
        for bind in &compiled.binds {
            self.line(bind);
        }
    }

    /// Whether `pattern` is irrefutable in `let`: a wildcard, a binding, or a
    /// struct/newtype/tuple-struct destructure whose sub-patterns are all
    /// irrefutable. An enum-variant path is refutable and stays a diagnostic.
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
    /// IIFE needed); everything else is a bare expression statement.
    fn emit_expr_stmt(&mut self, e: &ast::Expr) {
        match &e.kind {
            ast::ExprKind::If { .. } => self.emit_if_stmt(e),
            ast::ExprKind::While { cond, body } => {
                let c = self.emit_expr(cond);
                self.line(format!("while ({c}) {{"));
                self.indented(|em| em.emit_block_stmts(body));
                self.line("}");
            }
            ast::ExprKind::Loop { body } => {
                self.line("while (true) {");
                self.indented(|em| em.emit_block_stmts(body));
                self.line("}");
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.emit_for(pattern, iter, body),
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_stmt(scrutinee, arms),
            ast::ExprKind::Block(b) => {
                self.line("{");
                self.indented(|em| em.emit_block_stmts(b));
                self.line("}");
            }
            ast::ExprKind::Return(value) => {
                let r = value
                    .as_ref()
                    .map_or_else(String::new, |v| format!(" {}", self.emit_expr(v)));
                self.line(format!("return{r};"));
            }
            ast::ExprKind::Break => self.line("break;"),
            ast::ExprKind::Continue => self.line("continue;"),
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
                self.line(format!("{t} {sym} {v};"));
            }
            _ => {
                let s = self.emit_expr(e);
                self.line(format!("{s};"));
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
            let cmp = if *inclusive { "<=" } else { "<" };
            self.line(format!(
                "for (let {var} = {lo}; {var} {cmp} {hi}; {var}++) {{"
            ));
        } else {
            let it = self.emit_expr(iter);
            self.line(format!("for (const {var} of {it}) {{"));
        }
        self.indented(|em| em.emit_block_stmts(body));
        self.line("}");
    }

    fn emit_if_stmt(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if ({c}) {{"));
        self.indented(|em| em.emit_block_stmts(then));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                // `else if` chain: capture the nested `if` and splice it onto
                // the closing brace so it reads `} else if (…) {`.
                let lines = self.render(|em| em.emit_if_stmt(inner));
                self.splice_else(&lines);
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                let lines = self.render(|em| em.emit_block_stmts(b));
                self.line("} else {");
                self.indented(|em| {
                    for l in &lines {
                        em.line(l);
                    }
                });
                self.line("}");
                return;
            }
            Some(other) => {
                let lines = self.render(|em| em.emit_expr_stmt(other));
                self.line("} else {");
                self.indented(|em| {
                    for l in &lines {
                        em.line(l);
                    }
                });
                self.line("}");
                return;
            }
            None => {}
        }
        self.line("}");
    }

    /// Splice a captured `if (…) { … }` block onto an open `} else `, producing
    /// `} else if (…) { … }`. The captured lines start with `if (…) {`.
    fn splice_else(&mut self, lines: &[String]) {
        if let Some((first, rest)) = lines.split_first() {
            self.line(format!("}} else {first}"));
            for l in rest {
                self.line(l);
            }
        } else {
            self.line("}");
        }
    }

    /// Emit a JavaScript expression for `e`. Block-like forms become IIFEs.
    pub(crate) fn emit_expr(&mut self, e: &ast::Expr) -> String {
        match &e.kind {
            ast::ExprKind::Int(s) | ast::ExprKind::Float(s) => normalize_number(s),
            ast::ExprKind::Str(s) => js_string(s),
            ast::ExprKind::Bool(b) => b.to_string(),
            ast::ExprKind::Unit => "undefined".to_string(),
            ast::ExprKind::Path(segments) => self.emit_path(segments),
            ast::ExprKind::Binary { op, lhs, rhs } => {
                let l = self.emit_expr(lhs);
                let r = self.emit_expr(rhs);
                format!("({l} {} {r})", bin_op(*op))
            }
            ast::ExprKind::Unary { op, operand } => {
                let o = self.emit_expr(operand);
                let sym = match op {
                    ast::UnOp::Not => "!",
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
                    // constructs a tagged object.
                    if let [name] = segments.as_slice() {
                        if self.is_tuple_struct(name) {
                            return self.emit_tuple_struct_lit(name, args);
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
                    // Tuple/newtype payload is stored under integer-keyed
                    // properties `_0`, `_1`, …; `.0` reads `._0`.
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
            ast::ExprKind::If { .. } | ast::ExprKind::Block(_) => self.emit_block_like_value(e),
            ast::ExprKind::Closure { params, body } => {
                let ps: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                let b = self.emit_expr(body);
                format!("({}) => {b}", ps.join(", "))
            }
            ast::ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.uses_methods = true;
                let recv = self.emit_expr(receiver);
                let a = self.emit_args(args);
                if a.is_empty() {
                    format!("__call({recv}, \"{method}\")")
                } else {
                    format!("__call({recv}, \"{method}\", {a})")
                }
            }
            ast::ExprKind::Match { scrutinee, arms } => self.emit_match_value(scrutinee, arms),
            ast::ExprKind::Try(inner) => self.emit_try(inner),
            ast::ExprKind::Range { .. } => {
                self.error("range expressions are only supported as a `for` iterable");
                "undefined".to_string()
            }
            ast::ExprKind::Assign { .. } => {
                self.error("assignment is a statement, not an expression value");
                "undefined".to_string()
            }
            ast::ExprKind::While { .. }
            | ast::ExprKind::For { .. }
            | ast::ExprKind::Loop { .. } => {
                self.error("loops do not produce a value");
                "undefined".to_string()
            }
            ast::ExprKind::Return(_) | ast::ExprKind::Break | ast::ExprKind::Continue => {
                self.error("control-flow expression used in value position");
                "undefined".to_string()
            }
        }
    }

    fn emit_path(&mut self, segments: &[ast::Ident]) -> String {
        match self.resolve_path(segments) {
            Some(PathKind::Value(name)) => {
                self.note_prelude(&name);
                name
            }
            Some(PathKind::Variant(en, var, VariantShape::Unit)) => {
                self.note_prelude(&en);
                format!("{{ _enum: \"{en}\", tag: \"{var}\" }}")
            }
            // A tuple/struct variant in path position (no payload applied) is a
            // constructor function; the type checker should have rewritten it.
            Some(PathKind::Variant(_, _, _)) => {
                self.error(format!(
                    "enum variant `{}` needs its payload to construct a value",
                    segments.join("::")
                ));
                "undefined".to_string()
            }
            None => {
                self.error(format!(
                    "unresolved path `{}`: not a value or enum variant",
                    segments.join("::")
                ));
                "undefined".to_string()
            }
        }
    }

    /// Resolve a `::`-joined path against the symbol table.
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

    /// The payload shape of `Enum::Variant`, or `None`.
    pub(crate) fn variant_shape(&self, enum_name: &str, variant: &str) -> Option<VariantShape> {
        self.enums.variant_shape(enum_name, variant)
    }

    /// Resolve a path *in pattern position* to the variant it matches.
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
            return format!("{{ _enum: \"{en}\", tag: \"{var}\"{body} }}");
        }
        let Some(name) = path.last() else {
            return "undefined".to_string();
        };
        if let Some(base) = base {
            return self.emit_struct_update(name, fields, base);
        }
        let body = self.emit_field_inits(fields);
        format!("{{ __struct: \"{name}\"{body} }}")
    }

    /// Lower `S { f: e, .. base }` (functional update): spread the base's
    /// properties into a fresh object, then override the listed fields and
    /// re-stamp the `__struct` tag.
    fn emit_struct_update(
        &mut self,
        name: &str,
        fields: &[ast::FieldInit],
        base: &ast::Expr,
    ) -> String {
        let base_js = self.emit_expr(base);
        let body = self.emit_field_inits(fields);
        format!("{{ ...{base_js}, __struct: \"{name}\"{body} }}")
    }

    /// Lower struct-literal / struct-variant fields to `name: value` entries,
    /// expanding field shorthand (`{ x }` → `x: x`). Each entry is prefixed with
    /// `, ` so it appends cleanly after a leading tag.
    fn emit_field_inits(&mut self, fields: &[ast::FieldInit]) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for f in fields {
            let value = match &f.value {
                Some(v) => self.emit_expr(v),
                None => f.name.clone(),
            };
            let _ = write!(out, ", {}: {value}", f.name);
        }
        out
    }

    /// Lower a tuple-variant construction `E::V(a, …)` to its tagged object,
    /// storing the payload under integer-keyed properties `_0`, `_1`, ….
    fn emit_tuple_variant(&mut self, en: &str, var: &str, args: &[ast::Expr]) -> String {
        self.note_prelude(en);
        let payload = self.emit_positional(args);
        format!("{{ _enum: \"{en}\", tag: \"{var}\"{payload} }}")
    }

    /// Lower a tuple/newtype struct construction `N(a, …)` to its tagged object.
    fn emit_tuple_struct_lit(&mut self, name: &str, args: &[ast::Expr]) -> String {
        let payload = self.emit_positional(args);
        format!("{{ __struct: \"{name}\"{payload} }}")
    }

    /// Render positional payload args as `, _0: a, _1: b, …`, ready to append
    /// after a leading tag.
    fn emit_positional(&mut self, args: &[ast::Expr]) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (i, a) in args.iter().enumerate() {
            let v = self.emit_expr(a);
            let _ = write!(out, ", _{i}: {v}");
        }
        out
    }

    /// Wrap captured body lines in an IIFE that yields a value.
    fn wrap_iife(body: Vec<String>) -> String {
        let mut out = String::from("(() => {\n");
        for l in body {
            if l.is_empty() {
                out.push('\n');
            } else {
                out.push_str("  ");
                out.push_str(&l);
                out.push('\n');
            }
        }
        out.push_str("})()");
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
        let tmp = self.fresh_tmp("__s");
        let body = self.render(|em| {
            em.line(format!("const {tmp} = {scrut};"));
            for arm in arms {
                em.emit_match_arm(arm, &tmp, true);
            }
        });
        Self::wrap_iife(body)
    }

    /// Lower a `match` in statement position: the scrutinee bound once, arm
    /// bodies lowered for effect, no wrapping IIFE.
    pub(crate) fn emit_match_stmt(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) {
        let scrut = self.emit_expr(scrutinee);
        let tmp = self.fresh_tmp("__s");
        self.line("{");
        self.indented(|em| {
            em.line(format!("const {tmp} = {scrut};"));
            for arm in arms {
                em.emit_match_arm(arm, &tmp, false);
            }
        });
        self.line("}");
    }

    /// Emit one `match` arm. The pattern test gates the arm; bindings are
    /// introduced before a guard is evaluated, so the guard sees them.
    fn emit_match_arm(&mut self, arm: &ast::MatchArm, subject: &str, value: bool) {
        let compiled = self.compile_pattern(&arm.pattern, subject);
        self.line(format!("if ({}) {{", compiled.test()));
        self.indented(|em| {
            for bind in &compiled.binds {
                em.line(bind);
            }
            match &arm.guard {
                Some(guard) => {
                    let g = em.emit_expr(guard);
                    em.line(format!("if ({g}) {{"));
                    em.indented(|em| em.emit_arm_body(&arm.body, value));
                    em.line("}");
                }
                None => em.emit_arm_body(&arm.body, value),
            }
        });
        self.line("}");
    }

    /// Emit a match-arm body: `return <v>` in value context, lowered for effect
    /// in statement context.
    fn emit_arm_body(&mut self, body: &ast::Expr, value: bool) {
        if value {
            let v = self.emit_expr(body);
            self.line(format!("return {v};"));
        } else {
            self.emit_expr_stmt(body);
        }
    }

    /// Lower `e?`. The short-circuit must return the carrier from the enclosing
    /// function, which a self-contained IIFE cannot do. So the guard is hoisted
    /// as statements into the current buffer — `const __try_N = e; if
    /// (__try_N.tag === "Err" || __try_N.tag === "None") return __try_N;` — and
    /// the expression value of `e?` is the unwrapped payload `__try_N._0`.
    fn emit_try(&mut self, inner: &ast::Expr) -> String {
        let value = self.emit_expr(inner);
        let tmp = format!("__try_{}", self.try_seq);
        self.try_seq += 1;
        self.line(format!("const {tmp} = {value};"));
        self.line(format!(
            "if ({tmp}.tag === \"Err\" || {tmp}.tag === \"None\") return {tmp};"
        ));
        format!("{tmp}._0")
    }

    /// Emit an `if` whose branches `return` their value (for IIFE bodies).
    fn emit_if_return(&mut self, e: &ast::Expr) {
        let ast::ExprKind::If { cond, then, else_ } = &e.kind else {
            return;
        };
        let c = self.emit_expr(cond);
        self.line(format!("if ({c}) {{"));
        self.indented(|em| em.emit_block_as_body(then));
        match else_.as_deref() {
            Some(inner) if matches!(inner.kind, ast::ExprKind::If { .. }) => {
                let lines = self.render(|em| em.emit_if_return(inner));
                self.splice_else(&lines);
            }
            Some(ast::Expr {
                kind: ast::ExprKind::Block(b),
                ..
            }) => {
                let lines = self.render(|em| em.emit_block_as_body(b));
                self.line("} else {");
                self.indented(|em| {
                    for l in &lines {
                        em.line(l);
                    }
                });
                self.line("}");
            }
            Some(other) => {
                let v = self.emit_expr(other);
                self.line("} else {");
                self.indented(|em| em.line(format!("return {v};")));
                self.line("}");
            }
            None => self.line("}"),
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
        self.line(format!("function {}({}) {{", f.sig.name, params.join(", ")));
        self.indented(|em| em.emit_block_as_body(&f.body));
        self.line("}");
    }

    /// Run `f`, capturing the lines it emits into a separate buffer (used to
    /// build IIFE bodies and nested blocks). Indentation resets to zero inside.
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
        ast::BinOp::Or => "||",
        ast::BinOp::And => "&&",
        // `==`/`!=` on primitives stay strict comparisons. `Eq`-derived types
        // are rewritten to `a.eq(b)` method calls by the type checker's
        // elaboration before this runs, so a `BinOp::Eq` reaching here is
        // always a primitive comparison.
        ast::BinOp::Eq => "===",
        ast::BinOp::Ne => "!==",
        ast::BinOp::Lt => "<",
        ast::BinOp::Gt => ">",
        ast::BinOp::Le => "<=",
        ast::BinOp::Ge => ">=",
        // Numeric addition and string concatenation both lower to JavaScript
        // `+`. The type checker produces `Concat` for string-typed `+`; here
        // they share a target operator.
        ast::BinOp::Add | ast::BinOp::Concat => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        ast::BinOp::Div => "/",
        ast::BinOp::Rem => "%",
    }
}

/// Normalise a neoc numeric literal to JavaScript: strip digit separators.
fn normalize_number(s: &str) -> String {
    s.replace('_', "")
}

/// Encode a string value as a JavaScript double-quoted literal.
pub(crate) fn js_string(s: &str) -> String {
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
