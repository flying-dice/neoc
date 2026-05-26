//! Expression and pattern inference plus the well-formedness checks.
//!
//! The [`Checker`] owns the symbol table and a value-scope stack, walks every
//! function body, and pushes a [`Diagnostic`](crate::Diagnostic) for each
//! violation of `STATIC.md` §4–§8. Inference is the rank-1, no-subtyping
//! fragment described in §3.5: types flow from annotations and arguments, and
//! `Ty::Unknown` absorbs errors so one mistake produces one diagnostic.

use neoc_ast as ast;

use crate::Diagnostic;
use crate::collect::lower_type;
use crate::env::{Scopes, StructFields, SymbolTable, TypeDef, VariantFields};
use crate::ty::Ty;

/// The walking type-checker. Holds the collected declarations, the lexical
/// scope stack, the enclosing function's return type (for `?` and bare
/// `return`), and the accumulated diagnostics.
pub struct Checker<'a> {
    table: &'a SymbolTable,
    pub(crate) scopes: Scopes,
    /// Return type of the function whose body is currently being checked.
    pub(crate) ret_ty: Ty,
    /// Type parameters in scope for the current item.
    pub(crate) type_params: Vec<String>,
    /// Bounds for the in-scope type parameters: `T` → the trait names it is
    /// bound by (from `<T: Trait>` and `where T: Trait`). Lets a method call on
    /// a bounded parameter resolve through the bound trait's methods
    /// (`STATIC.md` §method-resolution).
    pub(crate) type_bounds: std::collections::HashMap<String, Vec<String>>,
    diagnostics: Vec<Diagnostic>,
    /// When set, every inferred expression's type is recorded in `expr_types`.
    /// Off by default so the compile path pays nothing; tooling turns it on for
    /// position→type queries.
    record_types: bool,
    expr_types: Vec<ExprType>,
    /// When set, every binding and single-segment identifier use is recorded in
    /// `resolution`, for scope-aware references/rename/goto. Off by default.
    record_resolution: bool,
    resolution: crate::resolve::Resolution,
}

/// The inferred type of one expression, with the byte range it occupies. The
/// basis for tooling that maps a cursor position to a type (hover, type inlay
/// hints).
#[derive(Debug, Clone, PartialEq)]
pub struct ExprType {
    pub start: usize,
    pub end: usize,
    pub ty: Ty,
}

impl<'a> Checker<'a> {
    #[must_use]
    pub fn new(table: &'a SymbolTable) -> Self {
        Self {
            table,
            scopes: Scopes::new(),
            ret_ty: Ty::Unit,
            type_params: Vec::new(),
            type_bounds: std::collections::HashMap::new(),
            diagnostics: Vec::new(),
            record_types: false,
            expr_types: Vec::new(),
            record_resolution: false,
            resolution: crate::resolve::Resolution::default(),
        }
    }

    /// Records every inferred expression's type and span from here on, for
    /// tooling position→type queries. The compile path leaves this off.
    pub fn record_types(&mut self) {
        self.record_types = true;
    }

    /// The expression types recorded since [`Self::record_types`] was enabled.
    #[must_use]
    pub fn into_expr_types(self) -> Vec<ExprType> {
        self.expr_types
    }

    /// Records every binding and single-segment identifier use from here on, for
    /// scope-aware tooling (references, rename, goto). The compile path leaves
    /// this off.
    pub fn record_resolution(&mut self) {
        self.record_resolution = true;
    }

    /// The scope resolution recorded since [`Self::record_resolution`] was
    /// enabled.
    #[must_use]
    pub fn into_resolution(self) -> crate::resolve::Resolution {
        self.resolution
    }

    /// Binds `name` of type `ty` in the current scope and, when recording,
    /// notes the binding's def-span and kind. The single funnel every binding
    /// site goes through, so the resolution map and the type scope never drift.
    fn bind_local(
        &mut self,
        name: impl Into<String>,
        ty: Ty,
        def: ast::Span,
        kind: crate::resolve::BindingKind,
    ) {
        let name = name.into();
        let id = self.scopes.bind(name.clone(), ty);
        if self.record_resolution {
            self.resolution.note_binding(crate::resolve::Binding {
                id,
                name,
                def: (def.start, def.end),
                kind,
            });
        }
    }

    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Whether `ty` is a nominal type carrying an `eq` method — i.e. a
    /// `#[derive(Eq)]` `struct`/`enum`. Used by elaboration to rewrite `==`/`!=`
    /// on such a type to a structural `.eq` call instead of Lua reference
    /// equality (`DYNAMIC.md` §4.2 / §6.2). The `Result`/`Option` prelude is not
    /// considered `Eq`; only user types that derive it are.
    #[must_use]
    pub fn type_derives_eq(&self, ty: &Ty) -> bool {
        let Ty::Named { name, .. } = ty else {
            return false;
        };
        if name == "Result" || name == "Option" {
            return false;
        }
        self.table
            .methods
            .contains_key(&(name.clone(), "eq".to_string()))
    }

    /// The collected declarations the checker resolves names against.
    pub(crate) fn table(&self) -> &SymbolTable {
        self.table
    }

    /// Records a diagnostic with no attached source range. The compiler maps a
    /// `0..0` span to the start of the file. Prefer [`Self::error_at`] whenever
    /// a node span is in hand.
    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: 0,
            end: 0,
        });
    }

    /// Records a diagnostic anchored to a node's byte [`ast::Span`], so the LSP
    /// can underline the offending source rather than falling back to line 0.
    pub(crate) fn error_at(&mut self, span: ast::Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: span.start,
            end: span.end,
        });
    }

    /// Lowers an AST type and expands any transparent `type` aliases through
    /// the symbol table, so an annotation written with an alias compares equal
    /// to its underlying type (`STATIC.md` §3.3).
    pub(crate) fn lower(&self, ty: &ast::Type) -> Ty {
        self.table
            .resolve_alias_ty(lower_type(ty, &self.type_params))
    }

    // ---- item walking ------------------------------------------------------

    /// Checks every item in the module.
    pub fn check_module(&mut self, module: &ast::Module) {
        for item in &module.items {
            self.check_item(item);
        }
    }

    fn check_item(&mut self, item: &ast::Item) {
        self.check_item_attrs(item);
        match &item.kind {
            ast::ItemKind::Fn(f) => self.check_fn(f, None),
            ast::ItemKind::Const(c) => {
                let declared = self.lower(&c.ty);
                let actual = self.infer(&c.value);
                self.expect(&declared, &actual, "const initialiser");
            }
            ast::ItemKind::Impl(b) => self.check_impl(b),
            ast::ItemKind::Trait(t) => {
                for ti in &t.items {
                    if let Some(body) = &ti.default {
                        self.check_fn_body(&ti.sig, body, Some("Self"));
                    }
                }
            }
            _ => {}
        }
    }

    fn check_fn(&mut self, f: &ast::FnDecl, self_ty: Option<&str>) {
        self.check_fn_body(&f.sig, &f.body, self_ty);
    }

    fn check_fn_body(&mut self, sig: &ast::FnSig, body: &ast::Block, self_ty: Option<&str>) {
        let prev_params = std::mem::take(&mut self.type_params);
        let prev_bounds = std::mem::take(&mut self.type_bounds);
        self.type_params = sig.generics.iter().map(|g| g.name.clone()).collect();
        self.type_bounds = crate::collect::param_bounds(sig);

        self.scopes.push();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => {
                    if let Some(t) = self_ty {
                        self.scopes.bind("self", Ty::named(t));
                    }
                }
                ast::Param::Named { name, ty, span } => {
                    let pty = Self::subst_self(self.lower(ty), self_ty);
                    self.bind_local(name.clone(), pty, *span, crate::resolve::BindingKind::Param);
                }
            }
        }
        let ret = sig.ret.as_ref().map_or(Ty::Unit, |r| self.lower(r));
        let ret = Self::subst_self(ret, self_ty);
        let prev_ret = std::mem::replace(&mut self.ret_ty, ret.clone());

        let actual = self.infer_block(body);
        self.expect(&ret, &actual, "function body");

        self.ret_ty = prev_ret;
        self.scopes.pop();
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
    }

    /// A receiver method `method` declared on the named trait, with the
    /// trait's `Self` projected to `self_ty`. `None` when the trait has no such
    /// receiver method — only receiver methods resolve through method syntax,
    /// an associated fn (no `self`) is not callable as `x.m()`.
    fn resolve_trait_method(
        &self,
        trait_name: &str,
        method: &str,
        self_ty: &Ty,
    ) -> Option<(Vec<Ty>, Ty)> {
        let Some(TypeDef::Trait(t)) = self.table.types.get(trait_name) else {
            return None;
        };
        let m = t
            .methods
            .iter()
            .find(|m| m.name == method && m.has_receiver)?;
        let proj = |ty: &Ty| subst_self_param(ty, self_ty);
        Some((m.params.iter().map(proj).collect(), proj(&m.ret)))
    }

    /// A method `method` provided by one of the trait bounds on type parameter
    /// `param`, with `Self` projected to the parameter itself. `None` when no
    /// bound supplies it. The cross-bound search is first-match in declaration
    /// order.
    fn bound_method(&self, param: &str, method: &str) -> Option<(Vec<Ty>, Ty)> {
        let bounds = self.type_bounds.get(param)?;
        let self_param = Ty::Param(param.to_string());
        bounds
            .iter()
            .find_map(|trait_name| self.resolve_trait_method(trait_name, method, &self_param))
    }

    /// A method `method` on a trait used directly as a type (the existential),
    /// with the trait's `Self` projected to the trait type itself, so
    /// `x.clone()` on `x: Drawable` yields `Drawable` (`STATIC.md` §3.4).
    fn trait_method(&self, trait_name: &str, method: &str) -> Option<(Vec<Ty>, Ty)> {
        self.resolve_trait_method(trait_name, method, &Ty::named(trait_name))
    }

    /// Checks a call's argument count and types against an already-projected
    /// parameter list, pushing a diagnostic per mismatch.
    fn check_call_args(
        &mut self,
        method: &str,
        params: &[Ty],
        arg_tys: &[Ty],
        args: &[ast::Expr],
        span: ast::Span,
    ) {
        if params.len() != arg_tys.len() {
            self.error_at(
                span,
                format!(
                    "method `{method}` takes {} argument(s) but {} were supplied",
                    params.len(),
                    arg_tys.len()
                ),
            );
            return;
        }
        for (i, (p, a)) in params.iter().zip(arg_tys).enumerate() {
            if !self.assignable(p, a) {
                self.error_at(
                    args[i].span,
                    format!(
                        "argument {} of `{method}` expects `{p}`, found `{a}`",
                        i + 1
                    ),
                );
            }
        }
    }

    pub(crate) fn subst_self(ty: Ty, self_ty: Option<&str>) -> Ty {
        let Some(target) = self_ty else { return ty };
        match ty {
            Ty::Named { name, args } if name == "Self" && args.is_empty() => Ty::named(target),
            Ty::Named { name, args } => Ty::Named {
                name,
                args: args
                    .into_iter()
                    .map(|a| Self::subst_self(a, self_ty))
                    .collect(),
            },
            Ty::List(inner) => Ty::List(Box::new(Self::subst_self(*inner, self_ty))),
            Ty::Fn(params, r) => Ty::Fn(
                params
                    .into_iter()
                    .map(|p| Self::subst_self(p, self_ty))
                    .collect(),
                Box::new(Self::subst_self(*r, self_ty)),
            ),
            other => other,
        }
    }

    fn check_impl(&mut self, b: &ast::ImplBlock) {
        let Some(target) = (match &b.ty {
            ast::Type::Path(p) => p.segments.last().map(|s| s.name.clone()),
            _ => None,
        }) else {
            return;
        };
        // Trait-impl completeness (`STATIC.md` §8).
        if let Some(trait_path) = &b.trait_ {
            self.check_trait_impl(b, &target, trait_path);
        }
        for f in &b.items {
            self.check_fn(f, Some(&target));
        }
    }

    fn check_trait_impl(&mut self, b: &ast::ImplBlock, target: &str, trait_path: &ast::TypePath) {
        let Some(trait_name) = trait_path.segments.last().map(|s| &s.name) else {
            return;
        };
        let Some(TypeDef::Trait(t)) = self.table.types.get(trait_name) else {
            self.error(format!(
                "unknown trait `{trait_name}` in impl for `{target}`"
            ));
            return;
        };
        let provided: Vec<&String> = b.items.iter().map(|f| &f.sig.name).collect();
        // Missing required methods.
        for m in &t.methods {
            if !m.has_default && !provided.iter().any(|p| **p == m.name) {
                self.error(format!(
                    "`impl {trait_name} for {target}` is missing method `{}`",
                    m.name
                ));
            }
        }
        // Extraneous methods not declared by the trait.
        for f in &b.items {
            if !t.methods.iter().any(|m| m.name == f.sig.name) {
                self.error(format!(
                    "method `{}` is not a member of trait `{trait_name}`",
                    f.sig.name
                ));
            }
        }
        // Signature conformance: a provided method must match the trait's
        // declared signature — parameter types, return type, and receiver —
        // with `Self` read as the target type (`STATIC.md` §3.4). The impl's
        // collected signature already substitutes `Self` to the target; the
        // trait's is projected here. An `Unknown` on either side is left to the
        // error that produced it, so resolution failures do not cascade.
        let self_ty = Ty::named(target);
        for m in &t.methods {
            let Some(info) = self
                .table
                .methods
                .get(&(target.to_string(), m.name.clone()))
            else {
                continue;
            };
            let want_params: Vec<Ty> = m
                .params
                .iter()
                .map(|p| subst_self_param(p, &self_ty))
                .collect();
            let want_ret = subst_self_param(&m.ret, &self_ty);
            let exact = |a: &Ty, b: &Ty| a == b || a.is_unknown() || b.is_unknown();
            let matches = info.has_receiver == m.has_receiver
                && info.params.len() == want_params.len()
                && info
                    .params
                    .iter()
                    .zip(&want_params)
                    .all(|(a, b)| exact(a, b))
                && exact(&info.ret, &want_ret);
            if !matches {
                let want = Ty::Fn(want_params, Box::new(want_ret));
                let got = Ty::Fn(info.params.clone(), Box::new(info.ret.clone()));
                self.error(format!(
                    "method `{}` does not match trait `{trait_name}`: expected `{want}`, found `{got}`",
                    m.name
                ));
            }
        }
    }

    // ---- statements & blocks ----------------------------------------------

    fn infer_block(&mut self, block: &ast::Block) -> Ty {
        self.scopes.push();
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        let ty = block.value.as_ref().map_or(Ty::Unit, |e| self.infer(e));
        self.scopes.pop();
        ty
    }

    fn check_stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Let(l) => self.check_let(l),
            ast::StmtKind::Expr(e) => {
                let _ = self.infer(e);
            }
            ast::StmtKind::Item(item) => self.check_item(item),
        }
    }

    fn check_let(&mut self, l: &ast::LetStmt) {
        let declared = l.ty.as_ref().map(|t| self.lower(t));
        let value_ty = l.value.as_ref().map(|e| self.infer(e));
        let ty = match (&declared, &value_ty) {
            (Some(d), Some(v)) => {
                self.expect(d, v, "let binding");
                d.clone()
            }
            (Some(d), None) => d.clone(),
            (None, Some(v)) => v.clone(),
            (None, None) => Ty::Unknown,
        };
        self.bind_pattern(&l.pattern, &ty, crate::resolve::BindingKind::Let);
    }

    // ---- expression inference ---------------------------------------------

    /// Infers the type of an expression, emitting diagnostics on the way.
    pub fn infer(&mut self, expr: &ast::Expr) -> Ty {
        let span = expr.span;
        let ty = match &expr.kind {
            ast::ExprKind::Int(_) | ast::ExprKind::Float(_) => Ty::Number,
            ast::ExprKind::Str(_) => Ty::Str,
            ast::ExprKind::Bool(_) => Ty::Bool,
            ast::ExprKind::Unit => Ty::Unit,
            ast::ExprKind::Path(segs) => self.infer_path(segs, span),
            ast::ExprKind::Binary { op, lhs, rhs } => self.infer_binary(*op, lhs, rhs, span),
            ast::ExprKind::Unary { op, operand } => self.infer_unary(*op, operand),
            ast::ExprKind::Call { callee, args } => self.infer_call(callee, args, span),
            ast::ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.infer_method_call(receiver, method, args, span),
            ast::ExprKind::Field { base, field } => self.infer_field(base, field, span),
            ast::ExprKind::StructLit { path, fields, base } => {
                self.infer_struct_lit(path, fields, base.as_deref(), span)
            }
            ast::ExprKind::If { cond, then, else_ } => self.infer_if(cond, then, else_.as_deref()),
            ast::ExprKind::Match { scrutinee, arms } => self.infer_match(scrutinee, arms, span),
            ast::ExprKind::Block(b) => self.infer_block(b),
            ast::ExprKind::Try(inner) => self.infer_try(inner),
            ast::ExprKind::List(items) => self.infer_list(items),
            ast::ExprKind::Index { base, index } => self.infer_index(base, index, span),
            ast::ExprKind::Range { start, end, .. } => {
                let s = self.infer(start);
                self.expect(&Ty::Number, &s, "range start");
                let e = self.infer(end);
                self.expect(&Ty::Number, &e, "range end");
                // Ranges drive `for`; treat their value as a list of numbers.
                Ty::List(Box::new(Ty::Number))
            }
            ast::ExprKind::Assign { target, value, .. } => {
                let t = self.infer(target);
                let v = self.infer(value);
                self.expect(&t, &v, "assignment");
                Ty::Unit
            }
            ast::ExprKind::While { cond, body } => {
                let c = self.infer(cond);
                self.expect(&Ty::Bool, &c, "while condition");
                let _ = self.infer_block(body);
                Ty::Unit
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                let it = self.infer(iter);
                let elem = match it {
                    Ty::List(inner) => *inner,
                    Ty::Unknown => Ty::Unknown,
                    other => {
                        self.error_at(
                            iter.span,
                            format!("`for` expects a list to iterate, found `{other}`"),
                        );
                        Ty::Unknown
                    }
                };
                self.scopes.push();
                self.bind_pattern(pattern, &elem, crate::resolve::BindingKind::ForPattern);
                let _ = self.infer_block(body);
                self.scopes.pop();
                Ty::Unit
            }
            ast::ExprKind::Loop { body } => {
                let _ = self.infer_block(body);
                Ty::Unit
            }
            ast::ExprKind::Closure { params, body } => self.infer_closure(params, body),
            ast::ExprKind::Return(value) => {
                let actual = value.as_ref().map_or(Ty::Unit, |e| self.infer(e));
                let expected = self.ret_ty.clone();
                self.expect(&expected, &actual, "return value");
                Ty::Unknown
            }
            ast::ExprKind::Break | ast::ExprKind::Continue => Ty::Unknown,
        };
        if self.record_types {
            self.expr_types.push(ExprType {
                start: span.start,
                end: span.end,
                ty: ty.clone(),
            });
        }
        ty
    }

    fn infer_path(&mut self, segs: &[ast::Ident], span: ast::Span) -> Ty {
        if segs.is_empty() {
            return Ty::Unknown;
        }
        if segs.len() == 1 {
            let name = &segs[0];
            if let Some(t) = self.scopes.lookup(name) {
                let t = t.clone();
                // Record the use → the local binding it resolves to. A shadow
                // wins because `binding_of` reads the innermost frame first.
                if self.record_resolution {
                    if let Some(id) = self.scopes.binding_of(name) {
                        self.resolution.note_use(
                            span.start,
                            span.end,
                            crate::resolve::Resolved::Local(id),
                        );
                    }
                }
                return t;
            }
            if let Some(t) = self.table.values.get(name) {
                // Resolves to a top-level item, not a local — record it as such
                // so a rename of the item skips a shadowing local elsewhere.
                if self.record_resolution {
                    self.resolution.note_use(
                        span.start,
                        span.end,
                        crate::resolve::Resolved::Item(name.clone()),
                    );
                }
                return t.clone();
            }
            // A bare uppercase name may be a unit struct or unit variant.
            if let Some(ty) = self.resolve_unit_value(name) {
                return ty;
            }
            self.error_at(span, format!("unknown name `{name}`"));
            return Ty::Unknown;
        }
        // Qualified path: `Type::assoc` / `Enum::Variant`.
        self.resolve_qualified(segs, span)
    }

    /// Resolves a bare name to a unit struct or unit enum variant, if one
    /// exists. Returns the constructed type.
    fn resolve_unit_value(&self, name: &str) -> Option<Ty> {
        if let Some(TypeDef::Struct(s)) = self.table.types.get(name) {
            if matches!(s.fields, StructFields::Unit) {
                return Some(Ty::named(name));
            }
        }
        // A unit variant referenced unqualified (e.g. `None`).
        for def in self.table.types.values() {
            if let TypeDef::Enum(e) = def {
                if let Some(v) = e.variants.iter().find(|v| v.name == name) {
                    if matches!(v.fields, VariantFields::Unit) {
                        return Some(self.enum_instance(name, e));
                    }
                }
            }
        }
        None
    }

    fn enum_instance(&self, variant: &str, e: &crate::env::EnumInfo) -> Ty {
        // Build the enum type. Find which enum owns this variant by scanning.
        for (name, def) in &self.table.types {
            if let TypeDef::Enum(other) = def {
                if std::ptr::eq(other, e) || other.variants.iter().any(|v| v.name == variant) {
                    let args = other
                        .generics
                        .iter()
                        .map(|g| Ty::Param(g.clone()))
                        .collect();
                    return Ty::Named {
                        name: name.clone(),
                        args,
                    };
                }
            }
        }
        Ty::Unknown
    }

    fn resolve_qualified(&mut self, segs: &[ast::Ident], span: ast::Span) -> Ty {
        let type_name = &segs[segs.len() - 2];
        let member = &segs[segs.len() - 1];
        match self.table.types.get(type_name) {
            Some(TypeDef::Enum(e)) => {
                if let Some(v) = e.variants.iter().find(|v| v.name == *member) {
                    let enum_ty = Ty::Named {
                        name: type_name.clone(),
                        args: e.generics.iter().map(|g| Ty::Param(g.clone())).collect(),
                    };
                    return match &v.fields {
                        VariantFields::Tuple(tys) => Ty::Fn(tys.clone(), Box::new(enum_ty)),
                        VariantFields::Unit | VariantFields::Named(_) => enum_ty,
                    };
                }
                self.error_at(
                    span,
                    format!("enum `{type_name}` has no variant `{member}`"),
                );
                Ty::Unknown
            }
            Some(TypeDef::Struct(_) | TypeDef::Opaque) => {
                if let Some(m) = self.table.methods.get(&(type_name.clone(), member.clone())) {
                    return Ty::Fn(m.params.clone(), Box::new(m.ret.clone()));
                }
                self.error_at(
                    span,
                    format!("`{type_name}` has no associated function `{member}`"),
                );
                Ty::Unknown
            }
            _ => {
                self.error_at(span, format!("unresolved path `{}`", segs.join("::")));
                Ty::Unknown
            }
        }
    }

    fn infer_binary(
        &mut self,
        op: ast::BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: ast::Span,
    ) -> Ty {
        use ast::BinOp;
        let l = self.infer(lhs);
        let r = self.infer(rhs);
        match op {
            BinOp::Add => {
                // `+` is arithmetic on number, concatenation on string.
                if l.compatible(&Ty::Str) && r.compatible(&Ty::Str) && !l.is_unknown() {
                    Ty::Str
                } else if l.compatible(&Ty::Number) && r.compatible(&Ty::Number) {
                    if l.is_unknown() && r.is_unknown() {
                        Ty::Unknown
                    } else {
                        Ty::Number
                    }
                } else {
                    self.error_at(
                        span,
                        format!(
                            "`+` expects two `number`s or two `string`s, found `{l}` and `{r}`"
                        ),
                    );
                    Ty::Unknown
                }
            }
            BinOp::Concat => {
                // Only produced by elaboration (the checker rewrites a
                // string-typed `Add` to `Concat`), so it won't appear during
                // checking — but keep the arm honest: both operands are string.
                self.expect(&Ty::Str, &l, "concatenation operand");
                self.expect(&Ty::Str, &r, "concatenation operand");
                Ty::Str
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                self.expect(&Ty::Number, &l, "arithmetic operand");
                self.expect(&Ty::Number, &r, "arithmetic operand");
                Ty::Number
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                self.expect(&Ty::Number, &l, "comparison operand");
                self.expect(&Ty::Number, &r, "comparison operand");
                Ty::Bool
            }
            BinOp::Eq | BinOp::Ne => {
                if !l.compatible(&r) {
                    self.error_at(span, format!("cannot compare `{l}` with `{r}`"));
                }
                Ty::Bool
            }
            BinOp::And | BinOp::Or => {
                self.expect(&Ty::Bool, &l, "logical operand");
                self.expect(&Ty::Bool, &r, "logical operand");
                Ty::Bool
            }
        }
    }

    fn infer_unary(&mut self, op: ast::UnOp, operand: &ast::Expr) -> Ty {
        let t = self.infer(operand);
        match op {
            ast::UnOp::Not => {
                self.expect(&Ty::Bool, &t, "`!` operand");
                Ty::Bool
            }
            ast::UnOp::Neg => {
                self.expect(&Ty::Number, &t, "`-` operand");
                Ty::Number
            }
        }
    }

    fn infer_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], span: ast::Span) -> Ty {
        let callee_ty = self.infer(callee);
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
        match callee_ty {
            Ty::Fn(params, ret) => {
                if params.len() != arg_tys.len() {
                    self.error_at(
                        span,
                        format!(
                            "this call passes {} argument(s) but the function takes {}",
                            arg_tys.len(),
                            params.len()
                        ),
                    );
                    return Self::instantiate_return(&params, &arg_tys, &ret);
                }
                for (i, (p, a)) in params.iter().zip(&arg_tys).enumerate() {
                    if !self.assignable(p, a) {
                        self.error_at(
                            args[i].span,
                            format!("argument {} expects `{p}`, found `{a}`", i + 1),
                        );
                    }
                }
                let subst = Self::param_subst(&params, &arg_tys);
                self.check_call_bounds(callee, &subst, span);
                Self::apply_subst(&ret, &subst)
            }
            Ty::Unknown => Ty::Unknown,
            other => {
                self.error_at(callee.span, format!("`{other}` is not callable"));
                Ty::Unknown
            }
        }
    }

    /// Substitutes type parameters in a function's return type from the
    /// argument types (rank-1 inference for generic constructors like `Ok`).
    fn instantiate_return(params: &[Ty], args: &[Ty], ret: &Ty) -> Ty {
        Self::apply_subst(ret, &Self::param_subst(params, args))
    }

    /// The substitution binding each of a function's type parameters to the
    /// concrete type the matching argument supplied, inferred by unifying every
    /// parameter type against its argument.
    fn param_subst(params: &[Ty], args: &[Ty]) -> std::collections::HashMap<String, Ty> {
        let mut subst = std::collections::HashMap::new();
        for (p, a) in params.iter().zip(args) {
            Self::unify_param(p, a, &mut subst);
        }
        subst
    }

    /// Enforces the callee's declared trait bounds against the concrete types
    /// its type parameters were instantiated to (`STATIC.md` §3.5). Applies only
    /// to a direct call of a named function or constructor whose bounds were
    /// collected; an indirect call through a value carries no bounds to check.
    fn check_call_bounds(
        &mut self,
        callee: &ast::Expr,
        subst: &std::collections::HashMap<String, Ty>,
        span: ast::Span,
    ) {
        let ast::ExprKind::Path(segs) = &callee.kind else {
            return;
        };
        let Some(name) = segs.last() else { return };
        self.check_collected_bounds(name, subst, span);
    }

    /// Enforces the bounds collected under `name` (a function, constructor, or
    /// generic type) against the substitution that instantiated its parameters,
    /// pushing one diagnostic per parameter whose concrete type fails a bound.
    fn check_collected_bounds(
        &mut self,
        name: &str,
        subst: &std::collections::HashMap<String, Ty>,
        span: ast::Span,
    ) {
        let Some(bounds) = self.table.fn_bounds.get(name).cloned() else {
            return;
        };
        for (param, traits) in &bounds {
            let Some(concrete) = subst.get(param) else {
                continue;
            };
            for tr in traits {
                if !self.type_satisfies(concrete, tr) {
                    self.error_at(
                        span,
                        format!("the type `{concrete}` does not satisfy the bound `{tr}`"),
                    );
                }
            }
        }
    }

    /// Whether `ty` satisfies the bound `trait_name`. A concrete type satisfies
    /// it when an `impl` or derive records the pair; a type parameter satisfies
    /// it when its own in-scope bounds include the trait (bound propagation); an
    /// unknown type satisfies it vacuously, so an earlier error does not cascade.
    fn type_satisfies(&self, ty: &Ty, trait_name: &str) -> bool {
        match ty {
            Ty::Unknown => true,
            Ty::Param(p) => self
                .type_bounds
                .get(p)
                .is_some_and(|bs| bs.iter().any(|b| b == trait_name)),
            _ => ty_impl_name(ty).is_some_and(|n| self.table.implements(&n, trait_name)),
        }
    }

    fn unify_param(param: &Ty, arg: &Ty, subst: &mut std::collections::HashMap<String, Ty>) {
        match (param, arg) {
            (Ty::Param(name), a) if !a.is_unknown() => {
                subst.entry(name.clone()).or_insert_with(|| a.clone());
            }
            (Ty::List(p), Ty::List(a)) => Self::unify_param(p, a, subst),
            (Ty::Named { args: pa, .. }, Ty::Named { args: aa, .. }) if pa.len() == aa.len() => {
                for (p, a) in pa.iter().zip(aa) {
                    Self::unify_param(p, a, subst);
                }
            }
            _ => {}
        }
    }

    fn apply_subst(ty: &Ty, subst: &std::collections::HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::Param(name) => subst.get(name).cloned().unwrap_or(Ty::Unknown),
            Ty::List(inner) => Ty::List(Box::new(Self::apply_subst(inner, subst))),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|p| Self::apply_subst(p, subst)).collect(),
                Box::new(Self::apply_subst(ret, subst)),
            ),
            Ty::Named { name, args } => Ty::Named {
                name: name.clone(),
                args: args.iter().map(|a| Self::apply_subst(a, subst)).collect(),
            },
            other => other.clone(),
        }
    }

    /// Substitute the type parameters in `ty` from `subst`, leaving any
    /// parameter not in the map unchanged. Unlike [`Self::apply_subst`] (which
    /// erases unmapped parameters to `Unknown` for call-return instantiation),
    /// this is for projecting a field or method-return type of a nominal type
    /// through its concrete type arguments, where a parameter outside the
    /// nominal type's own generics must survive.
    fn subst_named(ty: &Ty, subst: &std::collections::HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::Param(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
            Ty::List(inner) => Ty::List(Box::new(Self::subst_named(inner, subst))),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|p| Self::subst_named(p, subst)).collect(),
                Box::new(Self::subst_named(ret, subst)),
            ),
            Ty::Named { name, args } => Ty::Named {
                name: name.clone(),
                args: args.iter().map(|a| Self::subst_named(a, subst)).collect(),
            },
            other => other.clone(),
        }
    }

    /// The substitution mapping a nominal type's generic parameter names to the
    /// concrete type arguments carried by `ty`. Returns an empty map when `ty`
    /// is not a generic nominal type or carries no arguments.
    fn nominal_subst(&self, ty: &Ty) -> std::collections::HashMap<String, Ty> {
        let mut subst = std::collections::HashMap::new();
        let Ty::Named { name, args } = ty else {
            return subst;
        };
        let generics = match self.table.types.get(name) {
            Some(TypeDef::Struct(s)) => &s.generics,
            Some(TypeDef::Enum(e)) => &e.generics,
            _ => return subst,
        };
        for (param, arg) in generics.iter().zip(args) {
            subst.insert(param.clone(), arg.clone());
        }
        subst
    }

    fn infer_method_call(
        &mut self,
        receiver: &ast::Expr,
        method: &str,
        args: &[ast::Expr],
        span: ast::Span,
    ) -> Ty {
        let recv = self.infer(receiver);
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
        // A method on a bounded type parameter resolves through its bounds:
        // `fn f<T: Display>(x: T) { x.show() }` finds `show` on `Display`.
        if let Ty::Param(p) = &recv {
            if let Some((params, ret)) = self.bound_method(p, method) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(
                span,
                format!("type parameter `{p}` has no method `{method}` from its bounds"),
            );
            return Ty::Unknown;
        }
        let Ty::Named { name, .. } = &recv else {
            if recv.is_unknown() {
                return Ty::Unknown;
            }
            self.error_at(
                receiver.span,
                format!("type `{recv}` has no method `{method}`"),
            );
            return Ty::Unknown;
        };
        // A method on a trait used as a type (the existential) resolves through
        // the trait's own methods, with `Self` read as the trait type.
        if self.is_trait(name) {
            if let Some((params, ret)) = self.trait_method(name, method) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        }
        let Some(m) = self.table.methods.get(&(name.clone(), method.to_string())) else {
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        };
        if m.params.len() != arg_tys.len() {
            self.error_at(
                span,
                format!(
                    "method `{method}` takes {} argument(s) but {} were supplied",
                    m.params.len(),
                    arg_tys.len()
                ),
            );
            return m.ret.clone();
        }
        // Project the method signature through the receiver's concrete type
        // arguments, so `Box<number>::get(self) -> T` yields `number`.
        let subst = self.nominal_subst(&recv);
        for (i, (p, a)) in m.params.iter().zip(&arg_tys).enumerate() {
            let p = Self::subst_named(p, &subst);
            if !self.assignable(&p, a) {
                self.error_at(
                    args[i].span,
                    format!(
                        "argument {} of `{method}` expects `{p}`, found `{a}`",
                        i + 1
                    ),
                );
            }
        }
        Self::subst_named(&m.ret, &subst)
    }

    fn infer_field(&mut self, base: &ast::Expr, field: &ast::FieldAccess, span: ast::Span) -> Ty {
        let base_ty = self.infer(base);
        let Ty::Named { name, .. } = &base_ty else {
            if base_ty.is_unknown() {
                return Ty::Unknown;
            }
            self.error_at(base.span, format!("type `{base_ty}` has no fields"));
            return Ty::Unknown;
        };
        let Some(TypeDef::Struct(s)) = self.table.types.get(name) else {
            self.error_at(
                base.span,
                format!("type `{name}` is not a struct with fields"),
            );
            return Ty::Unknown;
        };
        // Project the field type through the base's concrete type arguments, so
        // `Box<number>.value` yields `number` rather than the parameter `T`.
        let subst = self.nominal_subst(&base_ty);
        match (field, &s.fields) {
            (ast::FieldAccess::Named(f), StructFields::Named(fields)) => {
                if let Some((_, ty)) = fields.iter().find(|(n, _)| n == f) {
                    Self::subst_named(ty, &subst)
                } else {
                    self.error_at(span, format!("struct `{name}` has no field `{f}`"));
                    Ty::Unknown
                }
            }
            (ast::FieldAccess::Tuple(0), StructFields::Tuple(ty)) => Self::subst_named(ty, &subst),
            _ => {
                self.error_at(span, format!("invalid field access on `{name}`"));
                Ty::Unknown
            }
        }
    }

    fn infer_struct_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        base: Option<&ast::Expr>,
        span: ast::Span,
    ) -> Ty {
        let Some(name) = path.last() else {
            return Ty::Unknown;
        };
        // Struct-variant literal `Enum::Variant { .. }`.
        if path.len() >= 2 {
            return self.infer_variant_lit(path, fields, span);
        }
        let Some(TypeDef::Struct(s)) = self.table.types.get(name) else {
            self.error_at(span, format!("unknown struct `{name}`"));
            for f in fields {
                if let Some(v) = &f.value {
                    let _ = self.infer(v);
                }
            }
            return Ty::Unknown;
        };
        let StructFields::Named(decl) = &s.fields else {
            self.error_at(span, format!("`{name}` is not a struct with named fields"));
            return Ty::named(name);
        };
        let generics = s.generics.clone();
        let decl = decl.clone();
        self.check_named_fields(name, &decl, fields, base.is_some());
        // Infer the struct's type arguments by unifying each declared field
        // type against its initialiser's type, so `Box { value: 99 }` yields
        // `Box<number>` and downstream field/method projection (§generics) sees
        // the concrete argument.
        let result = self.infer_nominal_args(name, &generics, &decl, fields, base, span);
        if let Some(b) = base {
            let bt = self.infer(b);
            self.expect(&Ty::named(name), &bt, "struct update base");
        }
        result
    }

    /// Build the `Ty::Named` for a struct literal, inferring its type arguments
    /// (when the struct is generic) by unifying each declared field type against
    /// the initialiser's inferred type. A non-generic struct returns
    /// `Ty::named(name)`. A parameter with no initialiser to pin it stays
    /// `Unknown`.
    fn infer_nominal_args(
        &mut self,
        name: &str,
        generics: &[String],
        decl: &[(String, Ty)],
        inits: &[ast::FieldInit],
        base: Option<&ast::Expr>,
        span: ast::Span,
    ) -> Ty {
        if generics.is_empty() {
            return Ty::named(name);
        }
        let mut subst = std::collections::HashMap::new();
        for (fname, declared) in decl {
            let actual = inits.iter().find(|i| i.name == *fname).and_then(|init| {
                init.value
                    .as_ref()
                    .map(|e| self.infer(e))
                    .or_else(|| self.scopes.lookup(fname).cloned())
            });
            if let Some(actual) = actual {
                Self::unify_param(declared, &actual, &mut subst);
            }
        }
        // A `..base` update copies the remaining fields from a value of this
        // same type, so its arguments fill any parameter the inits did not pin.
        if let Some(b) = base {
            if let Ty::Named { name: bn, args } = self.infer(b) {
                if bn == name {
                    for (param, arg) in generics.iter().zip(args) {
                        subst.entry(param.clone()).or_insert(arg);
                    }
                }
            }
        }
        // The named-literal construction site: a parameter's bound must be
        // satisfied by the type its field initialisers pinned it to (§3.5).
        self.check_collected_bounds(name, &subst, span);
        let args = generics
            .iter()
            .map(|g| subst.get(g).cloned().unwrap_or(Ty::Unknown))
            .collect();
        Ty::Named {
            name: name.to_string(),
            args,
        }
    }

    fn infer_variant_lit(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldInit],
        span: ast::Span,
    ) -> Ty {
        let enum_name = &path[path.len() - 2];
        let variant = &path[path.len() - 1];
        let Some(TypeDef::Enum(e)) = self.table.types.get(enum_name) else {
            self.error_at(span, format!("unknown enum `{enum_name}`"));
            return Ty::Unknown;
        };
        let Some(v) = e.variants.iter().find(|v| v.name == *variant) else {
            self.error_at(
                span,
                format!("enum `{enum_name}` has no variant `{variant}`"),
            );
            return Ty::Unknown;
        };
        let VariantFields::Named(decl) = &v.fields else {
            self.error_at(
                span,
                format!("variant `{enum_name}::{variant}` does not have named fields"),
            );
            return Ty::named(enum_name);
        };
        let decl = decl.clone();
        self.check_named_fields(&format!("{enum_name}::{variant}"), &decl, fields, false);
        Ty::named(enum_name)
    }

    fn check_named_fields(
        &mut self,
        owner: &str,
        decl: &[(String, Ty)],
        inits: &[ast::FieldInit],
        has_base: bool,
    ) {
        for init in inits {
            match decl.iter().find(|(n, _)| *n == init.name) {
                Some((_, expected)) => {
                    let actual = match &init.value {
                        Some(e) => self.infer(e),
                        None => self
                            .scopes
                            .lookup(&init.name)
                            .cloned()
                            .unwrap_or(Ty::Unknown),
                    };
                    if !self.assignable(expected, &actual) {
                        self.error(format!(
                            "field `{}` of `{owner}` expects `{expected}`, found `{actual}`",
                            init.name
                        ));
                    }
                }
                None => self.error(format!("`{owner}` has no field `{}`", init.name)),
            }
        }
        if !has_base {
            for (fname, _) in decl {
                if !inits.iter().any(|i| i.name == *fname) {
                    self.error(format!("missing field `{fname}` in `{owner}`"));
                }
            }
        }
    }

    fn infer_if(&mut self, cond: &ast::Expr, then: &ast::Block, else_: Option<&ast::Expr>) -> Ty {
        let c = self.infer(cond);
        self.expect(&Ty::Bool, &c, "`if` condition");
        let then_ty = self.infer_block(then);
        match else_ {
            Some(e) => {
                let else_ty = self.infer(e);
                if !then_ty.compatible(&else_ty) {
                    self.error(format!(
                        "`if` branches disagree: `{then_ty}` vs `{else_ty}`"
                    ));
                    Ty::Unknown
                } else if then_ty.is_unknown() {
                    else_ty
                } else {
                    then_ty
                }
            }
            // An `if` with no `else` is a statement of type `()`.
            None => Ty::Unit,
        }
    }

    fn infer_list(&mut self, items: &[ast::Expr]) -> Ty {
        let tys: Vec<Ty> = items.iter().map(|i| self.infer(i)).collect();
        // An empty list needs an annotation; with none, its element is unknown.
        let Some(first) = tys.first() else {
            return Ty::List(Box::new(Ty::Unknown));
        };
        // Homogeneous (modulo `Unknown`/`Param`): the first type is the element.
        if tys.iter().all(|t| first.compatible(t)) {
            return Ty::List(Box::new(first.clone()));
        }
        // Heterogeneous: if every element's concrete type implements exactly one
        // common trait, the list is of that trait — the existential element type
        // (`STATIC.md` §3.4). A shared set of zero or several traits is ambiguous
        // and reported as a disagreement (the author annotates to choose).
        if let Some(trait_ty) = self.common_trait(&tys) {
            return Ty::List(Box::new(trait_ty));
        }
        let clash = tys.iter().find(|t| !first.compatible(t)).unwrap_or(first);
        self.error(format!("list elements disagree: `{first}` vs `{clash}`"));
        Ty::List(Box::new(first.clone()))
    }

    /// The single trait every element type implements, when exactly one exists
    /// — the element type of a heterogeneous list of implementors. `None` when
    /// the element types share no trait, share more than one (ambiguous), or
    /// any element is not a concrete nominal type.
    fn common_trait(&self, tys: &[Ty]) -> Option<Ty> {
        let mut common: Option<std::collections::BTreeSet<String>> = None;
        for t in tys {
            let name = ty_impl_name(t)?;
            let traits: std::collections::BTreeSet<String> = self
                .table
                .trait_impls
                .iter()
                .filter(|(ty, _)| *ty == name)
                .map(|(_, tr)| tr.clone())
                .collect();
            common = Some(match common {
                None => traits,
                Some(c) => &c & &traits,
            });
        }
        let common = common?;
        if common.len() == 1 {
            common.into_iter().next().map(Ty::named)
        } else {
            None
        }
    }

    fn infer_index(&mut self, base: &ast::Expr, index: &ast::Expr, span: ast::Span) -> Ty {
        let b = self.infer(base);
        let i = self.infer(index);
        self.expect(&Ty::Number, &i, "list index");
        match b {
            Ty::List(inner) => *inner,
            Ty::Unknown => Ty::Unknown,
            other => {
                self.error_at(span, format!("cannot index into `{other}`"));
                Ty::Unknown
            }
        }
    }

    fn infer_closure(&mut self, params: &[ast::ClosureParam], body: &ast::Expr) -> Ty {
        self.scopes.push();
        let mut ptys = Vec::new();
        for p in params {
            let ty = p.ty.as_ref().map_or(Ty::Unknown, |t| self.lower(t));
            self.bind_local(
                p.name.clone(),
                ty.clone(),
                p.span,
                crate::resolve::BindingKind::ClosureParam,
            );
            ptys.push(ty);
        }
        let ret = self.infer(body);
        self.scopes.pop();
        Ty::Fn(ptys, Box::new(ret))
    }

    // ---- `?` (STATIC.md §6) -----------------------------------------------

    fn infer_try(&mut self, inner: &ast::Expr) -> Ty {
        let t = self.infer(inner);
        match &t {
            Ty::Named { name, args } if name == "Result" && args.len() == 2 => match &self.ret_ty {
                Ty::Named { name: rn, args: ra } if rn == "Result" && ra.len() == 2 => {
                    if !args[1].compatible(&ra[1]) {
                        self.error(format!(
                            "`?` error type `{}` does not match the function's error type `{}`",
                            args[1], ra[1]
                        ));
                    }
                    args[0].clone()
                }
                _ => {
                    self.error(
                        "`?` on a `Result` is only valid in a function returning `Result`"
                            .to_string(),
                    );
                    args[0].clone()
                }
            },
            Ty::Named { name, args } if name == "Option" && args.len() == 1 => match &self.ret_ty {
                Ty::Named { name: rn, .. } if rn == "Option" => args[0].clone(),
                _ => {
                    self.error(
                        "`?` on an `Option` is only valid in a function returning `Option`"
                            .to_string(),
                    );
                    args[0].clone()
                }
            },
            Ty::Unknown => Ty::Unknown,
            other => {
                self.error(format!(
                    "`?` expects a `Result` or `Option`, found `{other}`"
                ));
                Ty::Unknown
            }
        }
    }

    // ---- match (STATIC.md §7) ---------------------------------------------

    fn infer_match(
        &mut self,
        scrutinee: &ast::Expr,
        arms: &[ast::MatchArm],
        span: ast::Span,
    ) -> Ty {
        let scrut = self.infer(scrutinee);
        let mut result: Option<Ty> = None;
        for arm in arms {
            self.scopes.push();
            self.bind_pattern(&arm.pattern, &scrut, crate::resolve::BindingKind::MatchArm);
            if let Some(g) = &arm.guard {
                let gt = self.infer(g);
                self.expect(&Ty::Bool, &gt, "match guard");
            }
            let body_ty = self.infer(&arm.body);
            self.scopes.pop();
            match &result {
                None => result = Some(body_ty),
                Some(r) => {
                    if !r.compatible(&body_ty) {
                        self.error_at(
                            arm.body.span,
                            format!("match arms disagree: `{r}` vs `{body_ty}`"),
                        );
                    }
                }
            }
        }
        self.check_exhaustive(&scrut, arms, span);
        result.unwrap_or(Ty::Unit)
    }

    /// Checks `match` exhaustiveness and redundant arms (`STATIC.md` §7). The
    /// `span` is the whole `match` expression, used to anchor exhaustiveness
    /// diagnostics.
    fn check_exhaustive(&mut self, scrut: &Ty, arms: &[ast::MatchArm], span: ast::Span) {
        // Redundant-arm check: anything after an unguarded catch-all is dead.
        let catch_all = arms
            .iter()
            .position(|a| a.guard.is_none() && self.is_catch_all(&a.pattern));
        if let Some(idx) = catch_all {
            if idx + 1 < arms.len() {
                self.error_at(
                    span,
                    "this match arm is unreachable; an earlier arm already matches every value"
                        .to_string(),
                );
            }
            return;
        }

        match scrut {
            Ty::Bool => {
                let covers_true = arms.iter().any(|a| pattern_matches_bool(&a.pattern, true));
                let covers_false = arms.iter().any(|a| pattern_matches_bool(&a.pattern, false));
                if !(covers_true && covers_false) {
                    self.error_at(
                        span,
                        "this match on `bool` is not exhaustive; cover both `true` and `false` or add a `_` arm"
                            .to_string(),
                    );
                }
            }
            Ty::Named { name, .. } => match self.table.types.get(name) {
                Some(TypeDef::Enum(e)) => {
                    let covered = Self::covered_variants(arms);
                    let missing: Vec<&str> = e
                        .variants
                        .iter()
                        .filter(|v| !covered.iter().any(|c| c == &v.name))
                        .map(|v| v.name.as_str())
                        .collect();
                    if !missing.is_empty() {
                        self.error_at(
                            span,
                            format!(
                                "this match on `{name}` is not exhaustive; missing variant(s): {}",
                                missing.join(", ")
                            ),
                        );
                    }
                }
                Some(TypeDef::Struct(_)) => {
                    // A struct has a single shape, so one unguarded irrefutable
                    // struct/tuple pattern (every sub-pattern irrefutable) covers
                    // every value — like an irrefutable `let` destructure.
                    let covered = arms
                        .iter()
                        .any(|a| a.guard.is_none() && is_irrefutable(&a.pattern));
                    if !covered {
                        self.error_at(
                            span,
                            format!(
                                "this match on `{name}` is not exhaustive; bind its fields with a struct pattern or add a `_` arm"
                            ),
                        );
                    }
                }
                _ => self.error_at(
                    span,
                    format!("this match on `{name}` is not exhaustive; add a `_` arm"),
                ),
            },
            Ty::Number | Ty::Str => {
                self.error_at(
                    span,
                    format!(
                        "this match on `{scrut}` needs a binding or `_` arm; literal arms can never be exhaustive"
                    ),
                );
            }
            _ => {}
        }
    }

    /// `true` for a pattern that matches every value of the scrutinee: a `_`
    /// wildcard or a bare binding that does *not* resolve to a unit variant or
    /// unit struct (those are coverage of one case, not a catch-all).
    fn is_catch_all(&self, pattern: &ast::Pattern) -> bool {
        match pattern {
            ast::Pattern::Wildcard => true,
            ast::Pattern::Binding { name, .. } => self.resolve_unit_value(name).is_none(),
            ast::Pattern::Or(alts) => alts.iter().any(|a| self.is_catch_all(a)),
            _ => false,
        }
    }

    /// The set of enum-variant names an arm list covers via variant patterns.
    /// Unqualified bare-ident patterns that resolve to a unit variant count too.
    fn covered_variants(arms: &[ast::MatchArm]) -> Vec<String> {
        let mut covered = Vec::new();
        for arm in arms {
            collect_covered(&arm.pattern, &mut covered);
        }
        covered
    }

    // ---- patterns (STATIC.md §4.2) ----------------------------------------

    /// Binds the names a pattern introduces against the scrutinee type. Also
    /// validates structural patterns against the scrutinee's declaration.
    pub(crate) fn bind_pattern(
        &mut self,
        pattern: &ast::Pattern,
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::Path(_) => {}
            ast::Pattern::Binding { name, span } => {
                // An uppercase bare ident may be a unit variant/struct, not a
                // binding. Treat as a binding unless it resolves to one.
                if self.resolve_unit_value(name).is_some() {
                    return;
                }
                self.bind_local(name.clone(), ty.clone(), *span, kind);
            }
            ast::Pattern::Or(alts) => {
                for alt in alts {
                    self.bind_pattern(alt, ty, kind);
                }
            }
            ast::Pattern::Tuple { path, elems } => self.bind_tuple_pattern(path, elems, ty, kind),
            ast::Pattern::Struct { path, fields, .. } => {
                self.bind_struct_pattern(path, fields, ty, kind);
            }
        }
    }

    fn bind_tuple_pattern(
        &mut self,
        path: &[ast::Ident],
        elems: &[ast::Pattern],
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        // Element types come from the variant or newtype the path names.
        let elem_tys = self.tuple_pattern_field_tys(path, ty, elems.len());
        for (pat, ety) in elems.iter().zip(elem_tys) {
            self.bind_pattern(pat, &ety, kind);
        }
    }

    fn tuple_pattern_field_tys(&self, path: &[ast::Ident], ty: &Ty, arity: usize) -> Vec<Ty> {
        let unknowns = vec![Ty::Unknown; arity];
        let Some(ctor) = path.last() else {
            return unknowns;
        };
        // Newtype struct pattern `UserId(x)`.
        if let Some(TypeDef::Struct(s)) = self.table.types.get(ctor) {
            if let StructFields::Tuple(inner) = &s.fields {
                return vec![inner.clone()];
            }
        }
        // Enum tuple variant `Ok(v)`, `Shape::Move(a, b)`.
        let enum_name = if path.len() >= 2 {
            Some(path[path.len() - 2].clone())
        } else {
            self.owning_enum(ctor)
        };
        if let Some(en) = enum_name {
            if let Some(TypeDef::Enum(e)) = self.table.types.get(&en) {
                if let Some(v) = e.variants.iter().find(|v| v.name == *ctor) {
                    if let VariantFields::Tuple(tys) = &v.fields {
                        return Self::instantiate_variant_fields(ty, e, tys);
                    }
                }
            }
        }
        unknowns
    }

    /// Substitutes the enum's type arguments (from the scrutinee `ty`) into a
    /// variant's declared field types, so `Ok(v)` binds `v` at the scrutinee's
    /// `T` rather than the bare parameter.
    fn instantiate_variant_fields(ty: &Ty, e: &crate::env::EnumInfo, fields: &[Ty]) -> Vec<Ty> {
        let mut subst = std::collections::HashMap::new();
        if let Ty::Named { args, .. } = ty {
            for (g, a) in e.generics.iter().zip(args) {
                subst.insert(g.clone(), a.clone());
            }
        }
        fields
            .iter()
            .map(|f| Self::apply_subst(f, &subst))
            .collect()
    }

    fn bind_struct_pattern(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldPat],
        ty: &Ty,
        kind: crate::resolve::BindingKind,
    ) {
        let decl = self.struct_pattern_field_tys(path, ty);
        for fp in fields {
            let fty = decl
                .iter()
                .find(|(n, _)| *n == fp.name)
                .map_or(Ty::Unknown, |(_, t)| t.clone());
            match &fp.pattern {
                Some(p) => self.bind_pattern(p, &fty, kind),
                // Shorthand `Point { x }` binds the field name itself.
                None => self.bind_local(fp.name.clone(), fty, fp.span, kind),
            }
        }
    }

    fn struct_pattern_field_tys(&self, path: &[ast::Ident], ty: &Ty) -> Vec<(String, Ty)> {
        let Some(ctor) = path.last() else {
            return Vec::new();
        };
        if let Some(TypeDef::Struct(s)) = self.table.types.get(ctor) {
            if let StructFields::Named(fields) = &s.fields {
                return fields.clone();
            }
        }
        let enum_name = if path.len() >= 2 {
            Some(path[path.len() - 2].clone())
        } else {
            self.owning_enum(ctor)
        };
        if let Some(en) = enum_name {
            if let Some(TypeDef::Enum(e)) = self.table.types.get(&en) {
                if let Some(v) = e.variants.iter().find(|v| v.name == *ctor) {
                    if let VariantFields::Named(fields) = &v.fields {
                        let subst_fields: Vec<(String, Ty)> = fields
                            .iter()
                            .map(|(n, t)| {
                                let mut subst = std::collections::HashMap::new();
                                if let Ty::Named { args, .. } = ty {
                                    for (g, a) in e.generics.iter().zip(args) {
                                        subst.insert(g.clone(), a.clone());
                                    }
                                }
                                (n.clone(), Self::apply_subst(t, &subst))
                            })
                            .collect();
                        return subst_fields;
                    }
                }
            }
        }
        Vec::new()
    }

    /// Finds the enum that declares a variant of the given name.
    fn owning_enum(&self, variant: &str) -> Option<String> {
        self.table.types.iter().find_map(|(name, def)| match def {
            TypeDef::Enum(e) if e.variants.iter().any(|v| v.name == variant) => Some(name.clone()),
            _ => None,
        })
    }

    // ---- shared --------------------------------------------------------------

    /// Records a mismatch diagnostic when `actual` may not be used where
    /// `expected` is required. `Unknown` on either side suppresses the report.
    fn expect(&mut self, expected: &Ty, actual: &Ty, context: &str) {
        if !self.assignable(expected, actual) {
            self.error(format!("{context} expects `{expected}`, found `{actual}`"));
        }
    }

    /// Whether a value of type `actual` may be used where `expected` is
    /// required. This is [`Ty::compatible`] plus the existential rule: when
    /// `expected` is a trait name used as a type, any concrete type that
    /// implements the trait — or the same trait itself — is admissible (the
    /// trait-as-type existential, `STATIC.md` §3.4). The relation is
    /// directional: an implementor flows into the trait, never the reverse
    /// (neoc has no implicit downcast). It recurses through lists, function
    /// types, and nominal type arguments, so `[Drawable]` accepts `[Circle]`.
    fn assignable(&self, expected: &Ty, actual: &Ty) -> bool {
        match (expected, actual) {
            (Ty::Unknown | Ty::Param(_), _) | (_, Ty::Unknown | Ty::Param(_)) => true,
            (Ty::Named { name, args }, actual) if args.is_empty() && self.is_trait(name) => {
                matches!(actual, Ty::Named { name: a, .. } if a == name)
                    || self.type_satisfies(actual, name)
            }
            (Ty::List(a), Ty::List(b)) => self.assignable(a, b),
            (Ty::Fn(pa, ra), Ty::Fn(pb, rb)) => {
                pa.len() == pb.len()
                    && pa.iter().zip(pb).all(|(x, y)| self.assignable(x, y))
                    && self.assignable(ra, rb)
            }
            (Ty::Named { name: na, args: aa }, Ty::Named { name: nb, args: ab }) => {
                na == nb
                    && aa.len() == ab.len()
                    && aa.iter().zip(ab).all(|(x, y)| self.assignable(x, y))
            }
            _ => expected.compatible(actual),
        }
    }

    /// Whether `name` names a trait in the type namespace.
    fn is_trait(&self, name: &str) -> bool {
        matches!(self.table.types.get(name), Some(TypeDef::Trait(_)))
    }
}

fn pattern_matches_bool(pattern: &ast::Pattern, value: bool) -> bool {
    match pattern {
        ast::Pattern::Literal(ast::LiteralPat::Bool(b)) => *b == value,
        ast::Pattern::Or(alts) => alts.iter().any(|a| pattern_matches_bool(a, value)),
        _ => false,
    }
}

/// Whether a pattern matches every value of its type (binds, never rejects).
/// A struct/tuple pattern is irrefutable when all its sub-patterns are; a
/// literal or variant path is refutable.
fn is_irrefutable(p: &ast::Pattern) -> bool {
    match p {
        ast::Pattern::Wildcard | ast::Pattern::Binding { .. } => true,
        ast::Pattern::Struct { fields, .. } => fields
            .iter()
            .all(|f| f.pattern.as_ref().is_none_or(is_irrefutable)),
        ast::Pattern::Tuple { elems, .. } => elems.iter().all(is_irrefutable),
        ast::Pattern::Or(alts) => alts.iter().any(is_irrefutable),
        ast::Pattern::Literal(_) | ast::Pattern::Path(_) => false,
    }
}

/// Walks a pattern, pushing any unit/tuple/struct variant names it covers.
fn collect_covered(pattern: &ast::Pattern, out: &mut Vec<String>) {
    match pattern {
        // A qualified unit variant (`Color::Red`), a tuple variant
        // (`Shape::Move(..)`), or a struct variant (`Shape::Circle { .. }`):
        // the final path segment is the variant name.
        ast::Pattern::Path(path)
        | ast::Pattern::Tuple { path, .. }
        | ast::Pattern::Struct { path, .. } => {
            if let Some(name) = path.last() {
                out.push(name.clone());
            }
        }
        // A bare unit variant reaches here as a binding (e.g. `None`); the
        // catch-all case returns before this runs (see `is_catch_all`).
        ast::Pattern::Binding { name, .. } => out.push(name.clone()),
        ast::Pattern::Or(alts) => {
            for a in alts {
                collect_covered(a, out);
            }
        }
        _ => {}
    }
}

/// Replaces `Self` with `self_param` throughout a type. Used to project a trait
/// method's signature onto the bounded type parameter whose bound supplied it,
/// so `T: Clone`'s `clone(self) -> Self` reads as `-> T` at the call site.
fn subst_self_param(ty: &Ty, self_param: &Ty) -> Ty {
    match ty {
        Ty::Named { name, args } if name == "Self" && args.is_empty() => self_param.clone(),
        Ty::Named { name, args } => Ty::Named {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| subst_self_param(a, self_param))
                .collect(),
        },
        Ty::List(inner) => Ty::List(Box::new(subst_self_param(inner, self_param))),
        Ty::Fn(params, ret) => Ty::Fn(
            params
                .iter()
                .map(|p| subst_self_param(p, self_param))
                .collect(),
            Box::new(subst_self_param(ret, self_param)),
        ),
        other => other.clone(),
    }
}

/// The name a type registers under in the trait-impl table: a nominal type's
/// own name, or a primitive's spelling. List and function types carry no
/// nominal identity, so they cannot be the target of an `impl` — `None`.
fn ty_impl_name(ty: &Ty) -> Option<String> {
    match ty {
        Ty::Named { name, .. } => Some(name.clone()),
        Ty::Number => Some("number".to_string()),
        Ty::Str => Some("string".to_string()),
        Ty::Bool => Some("bool".to_string()),
        _ => None,
    }
}
