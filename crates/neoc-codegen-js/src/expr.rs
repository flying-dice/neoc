//! Expression, statement, and block lowering.
//!
//! `emit_expr` returns a JavaScript expression string (block-like forms become
//! IIFEs so they carry a value). Statement-context lowering pushes lines via the
//! emitter's line buffer. Enum construction, `match`, and `?` consult the
//! [`crate::EnumIndex`] to resolve variant paths; a method call lowers to the
//! native `recv.m(args)`, dispatching up the receiver's prototype chain
//! (`lang/DYNAMIC.md` §4.7). The one form still needing the type checker is `+`
//! on `string` (it lowers to `+` either way, but the checker rewrites it to
//! [`ast::BinOp::Concat`] so addition and concatenation stay distinct upstream).

use std::fmt::Write as _;

use crate::Emitter;
use neoc_ast as ast;

/// What a `::`-joined path resolves to at a use site.
pub(crate) enum PathKind {
    /// The path is a value name (local, function, const, prelude ctor).
    Value(String),
    /// The path names an enum variant: `(enum name, variant name, shape)`.
    Variant(String, String, VariantShape),
    /// The path is an associated function on a method-bearing type —
    /// `Type::fn`, where `Type` carries a prototype. Emits `Type$.fn`, keeping
    /// the qualifier so the call reads the type's prototype instead of dropping
    /// to a bare (unbound) name. Mirrors `Assoc` in the Lua backend.
    Assoc(String, String),
    /// The path reads a member of an extern namespace — an imported `mod` or an
    /// extern struct's static surface. Emits `ns.member` directly (the extern
    /// value *is* the target value, `lang/EXTERN.md`).
    Extern(String, String),
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
        match &l.pattern {
            // A wildcard binds no name: run the initialiser for its effect and
            // discard the value. A literal `let _ = …` is invalid JS the moment
            // a scope holds two of them — the second `_` is a redeclaration — so
            // lower to a bare expression statement, which discards without a name.
            ast::Pattern::Wildcard => {
                if let Some(v) = &l.value {
                    self.emit_expr_stmt(v);
                }
            }
            // A simple binding maps straight to a `let`.
            ast::Pattern::Binding { name, .. } => match &l.value {
                Some(v) => {
                    let value = self.emit_expr(v);
                    self.line(format!("let {name} = {value};"));
                }
                None => self.line(format!("let {name};")),
            },
            // Everything else is an irrefutable destructure (or a diagnostic).
            _ => self.emit_let_destructure(&l.pattern, l.value.as_ref()),
        }
    }

    /// Lower an irrefutable destructuring `let`: a struct (`let Point { x, y } =
    /// p`) or newtype/tuple-struct (`let Wrapper(inner) = w`) pattern. The
    /// initialiser is bound to a temp once, then each variable is bound from a
    /// projection of it. A refutable pattern (an enum variant with siblings)
    /// stays a diagnostic.
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
                // A value that lowers to a `{`-prefixed object literal — a
                // struct or enum-variant construction — parses as a block, not
                // an expression, at statement start, so `{ … };` is a
                // `SyntaxError`. Paren-wrap it to keep it an expression run for
                // effect. This is the only statement-vs-expression ambiguity
                // `emit_expr` can produce; every other value form is `(`-,
                // identifier-, or literal-prefixed.
                if s.starts_with('{') {
                    self.line(format!("({s});"));
                } else {
                    self.line(format!("{s};"));
                }
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
            ast::ExprKind::Call { callee, args } => self.emit_call(callee, args),
            ast::ExprKind::Field { base, field } => {
                let b = self.emit_expr(base);
                match field {
                    ast::FieldAccess::Named(n, _) => format!("{b}.{n}"),
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
                // A `{`-prefixed body — a struct or enum-variant object literal —
                // parses as a function block, not a returned object, after `=>`.
                // Paren-wrap it so it stays an expression. A block-bodied closure
                // lowers through `emit_block_like_value` to a `(`-prefixed IIFE,
                // so this targets only the object-literal case.
                if b.starts_with('{') {
                    format!("({}) => ({b})", ps.join(", "))
                } else {
                    format!("({}) => {b}", ps.join(", "))
                }
            }
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span: _,
                args,
            } => {
                let recv = self.emit_expr(receiver);
                // `Eq` is the one derive whose method takes both operands
                // explicitly — `S$.eq = function(a, b)`, no `this`
                // (`lang/DYNAMIC.md` §6.2). The `==` rewrite (`crate::elaborate`)
                // produces a `MethodCall { method: "eq", args: [rhs] }`; lower it
                // so the receiver is passed as the first operand and resolves
                // through the prototype chain: `recv.eq(recv, rhs)`. `this` binds
                // to `recv` and is ignored by the comparison.
                if method == "eq" && args.len() == 1 {
                    let rhs = self.emit_expr(&args[0]);
                    return format!("{recv}.eq({recv}, {rhs})");
                }
                let a = self.emit_args(args);
                // A method call lowers to the native property call: `this` binds
                // to the receiver and dispatch resolves up its prototype chain
                // (`lang/DYNAMIC.md` §4.7). A foreign (`extern`) receiver
                // dispatches by the same call, so neoc and host values are
                // uniform — no registry, no dispatch helper.
                format!("{recv}.{method}({a})")
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

    /// Lower a call. A path callee may be an enum-variant construction, an
    /// `#[extern(constructor)]` (→ `new T(a)`), or a tuple/newtype struct
    /// constructor; otherwise it is an ordinary call.
    fn emit_call(&mut self, callee: &ast::Expr, args: &[ast::Expr]) -> String {
        if let ast::ExprKind::Path(segments) = &callee.kind {
            if let Some(PathKind::Variant(en, var, _)) = self.resolve_path(segments) {
                return self.emit_tuple_variant(&en, &var, args);
            }
            // An `#[extern(constructor)]` call `T::new(a)` lowers to the
            // target's constructor form, `new T(a)` (`lang/EXTERN.md`).
            if let [ty, method] = segments.as_slice() {
                if self.is_extern_constructor(ty, method) {
                    let a = self.emit_args(args);
                    let qualified = self.qualify_extern_value(ty);
                    return format!("new {qualified}({a})");
                }
            }
            // A call to a single-segment tuple/newtype struct name constructs a
            // tagged object.
            if let [name] = segments.as_slice() {
                if self.is_tuple_struct(name) {
                    return self.emit_tuple_struct_lit(name, args);
                }
                // A bare call to a free extern `fn` declared inside an extern
                // `mod` qualifies through the `mod`'s import binding
                // (`serve(opts)` → `Bun.serve(opts)`), so it resolves against the
                // imported namespace rather than an unbound name (`lang/EXTERN.md`
                // §Lowering). Covers both locally-declared and `use`-imported
                // ambient/namespaced mod fns.
                if let Some(qualified) = self.qualify_extern_fn(name) {
                    let a = self.emit_args(args);
                    return format!("{qualified}({a})");
                }
            }
        }
        let f = self.emit_expr(callee);
        let a = self.emit_args(args);
        format!("{f}({a})")
    }

    fn emit_path(&mut self, segments: &[ast::Ident]) -> String {
        match self.resolve_path(segments) {
            // A lone `self` reads as `this`: a method body's receiver is the
            // implicit `this`, not a parameter (`lang/DYNAMIC.md` §3.3).
            Some(PathKind::Value(name)) if name == "self" => "this".to_string(),
            Some(PathKind::Value(name)) => {
                self.note_prelude(&name);
                name
            }
            // `Type::fn` reads the type's prototype — keep the qualifier so the
            // associated function resolves against `Type$`, not a bare name. The
            // call site `Type::fn(args)` then composes through this.
            Some(PathKind::Assoc(ty, name)) => {
                format!("{ty}$.{name}")
            }
            // `ns::member` on an extern namespace reads the target value
            // directly (`lang/EXTERN.md` §Lowering). When `ns` is an extern
            // struct inside a `mod`, qualify it through the `mod`'s import so the
            // static surface resolves (`sqlite.Database.f`); a `mod` head or a
            // top-level struct passes through unchanged.
            Some(PathKind::Extern(ns, name)) => {
                format!("{}.{name}", self.qualify_extern_value(&ns))
            }
            Some(PathKind::Variant(en, var, VariantShape::Unit)) => {
                self.note_prelude(&en);
                let proto = self.proto_link(&en);
                format!("{{ __enum: \"{en}\", __tag: \"{var}\"{proto} }}")
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
                .or_else(|| {
                    // An extern namespace head — an imported `mod` or extern
                    // struct — keeps its qualifier: `ns.member` reads the target
                    // value directly (`lang/EXTERN.md`).
                    self.is_extern_namespace(en)
                        .then(|| PathKind::Extern(en.clone(), var.clone()))
                })
                .or_else(|| {
                    // `Type::fn` on a method-bearing type is an associated
                    // function living on the prototype `Type$`; keep the
                    // qualifier. A head that is not a known method-bearing type (a
                    // module prefix) still drops to its last segment, as before.
                    self.type_has_methods(en)
                        .then(|| PathKind::Assoc(en.clone(), var.clone()))
                })
                .or_else(|| Some(PathKind::Value(var.clone()))),
        }
    }

    /// Whether `name` heads an extern namespace: an imported `mod` or an extern
    /// struct (whose static surface is read directly as `name.member`).
    pub(crate) fn is_extern_namespace(&self, name: &str) -> bool {
        self.extern_structs.contains(name)
            || self.extern_imports.iter().any(|i| i.local_name == name)
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
            let proto = self.proto_link(&en);
            return format!("{{ __enum: \"{en}\", __tag: \"{var}\"{body}{proto} }}");
        }
        let Some(name) = path.last() else {
            return "undefined".to_string();
        };
        // An extern struct literal *is* the target object: a bare object
        // literal, no `__struct` tag (`lang/EXTERN.md` §Lowering). So an option
        // bag builds naturally and a function-typed field is a JS closure.
        if self.is_extern_struct(name) {
            let body = self.emit_extern_field_inits(fields);
            return format!("{{ {body} }}");
        }
        if let Some(base) = base {
            return self.emit_struct_update(name, fields, base);
        }
        let body = self.emit_field_inits(fields);
        let proto = self.proto_link(name);
        format!("{{ __struct: \"{name}\"{body}{proto} }}")
    }

    /// The `, __proto__: Type$` suffix linking a constructed value to its
    /// prototype, or `""` for a data-only type with no prototype
    /// (`lang/DYNAMIC.md` §3.1, §7.1).
    fn proto_link(&self, name: &str) -> String {
        if self.type_has_methods(name) {
            format!(", __proto__: {name}$")
        } else {
            String::new()
        }
    }

    /// Whether `name` is a struct declared in an `extern` block.
    pub(crate) fn is_extern_struct(&self, name: &str) -> bool {
        self.extern_structs.contains(name)
    }

    /// Whether `(ty, method)` is an `#[extern(constructor)]` — `ty::method(a)`
    /// lowers to `new ty(a)`.
    pub(crate) fn is_extern_constructor(&self, ty: &str, method: &str) -> bool {
        self.extern_constructors
            .contains(&(ty.to_string(), method.to_string()))
    }

    /// Qualify an extern struct's value reference through its enclosing `mod`'s
    /// import binding (`Database` in `mod sqlite` → `sqlite.Database`), so the
    /// reference resolves against the imported namespace rather than a bare
    /// unbound name (`lang/EXTERN.md` §Lowering). A struct with no enclosing
    /// `mod` (top-level / ambient) keeps its bare name.
    pub(crate) fn qualify_extern_value(&self, ty: &str) -> String {
        match self.extern_struct_ns.get(ty) {
            Some(ns) => format!("{ns}.{ty}"),
            None => ty.to_string(),
        }
    }

    /// Qualify a free extern `fn`'s call through its enclosing `mod`'s import
    /// binding (`serve` in `mod Bun` → `Bun.serve`), so the bare call resolves
    /// against the imported namespace rather than an unbound name (`lang/EXTERN.md`
    /// §Lowering). The `mod` binding is the JS global root for an ambient mod and
    /// the `import * as` namespace for a non-ambient one, so the same `ns.fn`
    /// qualification works for both. Returns `None` for a fn with no enclosing
    /// `mod` (top-level), which keeps its bare name.
    pub(crate) fn qualify_extern_fn(&self, name: &str) -> Option<String> {
        self.extern_fn_ns.get(name).map(|ns| format!("{ns}.{name}"))
    }

    /// Lower extern struct-literal fields to `name: value` entries with no
    /// leading separator (the object literal has no tag to follow), expanding
    /// field shorthand (`{ x }` → `x: x`).
    fn emit_extern_field_inits(&mut self, fields: &[ast::FieldInit]) -> String {
        fields
            .iter()
            .map(|f| {
                let value = match &f.value {
                    Some(v) => self.emit_expr(v),
                    None => f.name.clone(),
                };
                format!("{}: {value}", f.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
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
        let proto = self.proto_link(name);
        format!("{{ ...{base_js}, __struct: \"{name}\"{body}{proto} }}")
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

    /// Emit the exported constructor namespace for a `pub enum E` — an object
    /// `export const E = { … }` whose keys are the variants, each producing the
    /// exact runtime shape `E::Variant` already lowers to (`lang/DYNAMIC.md` §3.2,
    /// §7.1), plus a `match(value, arms)` reader that dispatches on `__tag`. This
    /// is the only place host code touches `__tag`/`_0`: it builds and reads
    /// enums through the namespace, never by hand. Purely additive — internal
    /// `E::Variant`/`match` lowering is unchanged. Emitted only for `pub` enums.
    pub(crate) fn emit_pub_enum_namespace(&mut self, e: &ast::EnumDecl) {
        let proto = self.proto_link(&e.name);
        self.line(format!("export const {} = {{", e.name));
        self.indented(|em| {
            for v in &e.variants {
                em.line(enum_namespace_entry(&e.name, v, &proto));
            }
            // The `match` reader: dispatch on the tag to the matching arm,
            // passing the payload. Tuple → positional `_0`[,`_1`…]; struct → the
            // field object; unit → no args. The `__tag`/`_0` access lives here so
            // host code never writes it.
            em.line(format!(
                "match: (value, arms) => {},",
                enum_namespace_match(e)
            ));
        });
        self.line("};");
    }

    /// Lower a tuple-variant construction `E::V(a, …)` to its tagged object,
    /// storing the payload under integer-keyed properties `_0`, `_1`, ….
    fn emit_tuple_variant(&mut self, en: &str, var: &str, args: &[ast::Expr]) -> String {
        self.note_prelude(en);
        let payload = self.emit_positional(args);
        let proto = self.proto_link(en);
        format!("{{ __enum: \"{en}\", __tag: \"{var}\"{payload}{proto} }}")
    }

    /// Lower a tuple/newtype struct construction `N(a, …)` to its tagged object.
    fn emit_tuple_struct_lit(&mut self, name: &str, args: &[ast::Expr]) -> String {
        let payload = self.emit_positional(args);
        let proto = self.proto_link(name);
        format!("{{ __struct: \"{name}\"{payload}{proto} }}")
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
    /// (__try_N.__tag === "Err" || __try_N.__tag === "None") return __try_N;` — and
    /// the expression value of `e?` is the unwrapped payload `__try_N._0`.
    fn emit_try(&mut self, inner: &ast::Expr) -> String {
        let value = self.emit_expr(inner);
        let tmp = format!("__try_{}", self.try_seq);
        self.try_seq += 1;
        self.line(format!("const {tmp} = {value};"));
        self.line(format!(
            "if ({tmp}.__tag === \"Err\" || {tmp}.__tag === \"None\") return {tmp};"
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

/// One entry of a `pub enum`'s constructor namespace, building the exact
/// runtime shape `E::Variant` lowers to (`proto` is the `, __proto__: E$` suffix
/// for a method-bearing enum, or empty). A unit variant is a value (object); a
/// tuple variant is an arrow `(_0, _1) => ({ … })`; a struct variant is an arrow
/// `({ x, y }) => ({ … })` spreading the named fields.
fn enum_namespace_entry(enum_name: &str, v: &ast::Variant, proto: &str) -> String {
    let head = format!("__enum: \"{enum_name}\", __tag: \"{}\"", v.name);
    match &v.body {
        ast::VariantBody::Unit => format!("{}: {{ {head}{proto} }},", v.name),
        ast::VariantBody::Tuple(tys) => {
            let params = (0..tys.len())
                .map(|i| format!("_{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let payload = (0..tys.len()).fold(String::new(), |mut acc, i| {
                let _ = write!(acc, ", _{i}");
                acc
            });
            format!("{}: ({params}) => ({{ {head}{payload}{proto} }}),", v.name)
        }
        ast::VariantBody::Named(fields) => {
            let names = fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let payload = fields.iter().fold(String::new(), |mut acc, f| {
                let _ = write!(acc, ", {}", f.name);
                acc
            });
            format!(
                "{}: ({{ {names} }}) => ({{ {head}{payload}{proto} }}),",
                v.name
            )
        }
    }
}

/// The body of a `pub enum`'s `match(value, arms)` reader: dispatch on
/// `value.__tag` to `arms[tag]`, passing the variant's payload — tuple →
/// positional `_0`[,`_1`…], struct → the field object, unit → no args. Built as
/// a ternary chain over the variants so each variant passes exactly its payload.
fn enum_namespace_match(e: &ast::EnumDecl) -> String {
    let mut chain = String::new();
    for v in &e.variants {
        let call = match &v.body {
            ast::VariantBody::Unit => format!("arms.{}()", v.name),
            ast::VariantBody::Tuple(tys) => {
                let args = (0..tys.len())
                    .map(|i| format!("value._{i}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("arms.{}({args})", v.name)
            }
            ast::VariantBody::Named(fields) => {
                let obj = fields
                    .iter()
                    .map(|f| format!("{}: value.{}", f.name, f.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("arms.{}({{ {obj} }})", v.name)
            }
        };
        let _ = write!(chain, "value.__tag === \"{}\" ? {call} : ", v.name);
    }
    // Exhaustiveness is a static guarantee; an unmatched tag cannot occur, so the
    // final fallthrough is `undefined`.
    chain.push_str("undefined");
    chain
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
