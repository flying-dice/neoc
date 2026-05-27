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

mod impl_check;
mod pattern;
mod solve;
mod usefulness;

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
    /// Associated-type bindings in scope for the current `impl` body: `Item` →
    /// the type it is bound to. Lets `Self::Item` resolve to the concrete type
    /// the impl assigned it (`STATIC.md` §3.4). Empty outside a trait impl.
    pub(crate) assoc_bindings: std::collections::HashMap<String, Ty>,
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
            assoc_bindings: std::collections::HashMap::new(),
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

    /// Records a `recv.member` access as resolving to `member` on `owner` (a
    /// type or trait), when recording is on and the member's def-span is known.
    /// The single funnel for member resolution, called at each point method or
    /// field resolution settles on an owner.
    fn record_member(&mut self, use_span: ast::Span, owner: &str, member: &str) {
        if !self.record_resolution {
            return;
        }
        if let Some(&def) = self
            .table
            .member_defs
            .get(&(owner.to_string(), member.to_string()))
        {
            self.resolution.note_member(
                use_span.start,
                use_span.end,
                crate::resolve::MemberRef {
                    owner: owner.to_string(),
                    member: member.to_string(),
                    def,
                },
            );
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
            labels: Vec::new(),
        });
    }

    /// Records a diagnostic anchored to a node's byte [`ast::Span`], so the LSP
    /// can underline the offending source rather than falling back to line 0.
    pub(crate) fn error_at(&mut self, span: ast::Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: span.start,
            end: span.end,
            labels: Vec::new(),
        });
    }

    /// Records a diagnostic with one or more secondary labels — related source
    /// locations (each a byte range plus its own message) that explain the
    /// primary one, such as where a trait method was declared. Surfaced as LSP
    /// related-information.
    pub(crate) fn error_labeled(
        &mut self,
        span: (usize, usize),
        message: impl Into<String>,
        labels: Vec<crate::Label>,
    ) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
            start: span.0,
            end: span.1,
            labels,
        });
    }

    /// Lowers an AST type and expands any transparent `type` aliases through
    /// the symbol table, so an annotation written with an alias compares equal
    /// to its underlying type (`STATIC.md` §3.3). Inside a trait impl, a
    /// `Self::Item` projection resolves to the impl's binding for `Item`.
    pub(crate) fn lower(&self, ty: &ast::Type) -> Ty {
        let lowered = self
            .table
            .resolve_alias_ty(lower_type(ty, &self.type_params));
        self.project_assoc(lowered)
    }

    /// Resolves any `Self::Item` projection sentinel in `ty` to the impl's
    /// binding for that associated type, recursing through composites. A no-op
    /// outside an impl (no bindings) or for an unbound name (left as-is, where
    /// it surfaces as a mismatch).
    fn project_assoc(&self, ty: Ty) -> Ty {
        crate::collect::project_assoc(ty, &self.assoc_bindings)
    }

    // ---- item walking ------------------------------------------------------

    /// Checks every item in the module.
    pub fn check_module(&mut self, module: &ast::Module) {
        self.check_coherence();
        for item in &module.items {
            self.check_item(item);
        }
    }

    /// Rejects the two blanket-impl overlaps a simple checker can detect: two
    /// blanket impls of the same trait (unorderable), and a blanket impl of a
    /// trait that a concrete type also implements directly (`STATIC.md` §3.5).
    /// neoc has no general coherence solver beyond these.
    fn check_coherence(&mut self) {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for bl in &self.table.blanket_impls {
            if !seen.insert(bl.trait_name.as_str()) {
                self.error(format!("conflicting blanket impls of `{}`", bl.trait_name));
            }
        }
        for bl in &self.table.blanket_impls {
            for (ty, tr) in &self.table.trait_impls {
                if tr == &bl.trait_name {
                    self.error(format!(
                        "`impl {tr} for {ty}` overlaps the blanket impl of `{tr}`"
                    ));
                }
            }
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
                self.check_supertraits(t);
                for ti in &t.items {
                    if let Some(body) = &ti.default {
                        self.check_fn_body(&ti.sig, body, Some("Self"));
                    }
                }
            }
            _ => {}
        }
    }

    /// Validates a trait's supertrait list: each named supertrait must be a
    /// trait, and the supertrait relation must be acyclic (`STATIC.md` §3.4).
    fn check_supertraits(&mut self, t: &ast::TraitDecl) {
        for sup in &t.supertraits {
            let Some(name) = sup.segments.last().map(|s| &s.name) else {
                continue;
            };
            if !self.is_trait(name) {
                self.error(format!("unknown supertrait `{name}` on trait `{}`", t.name));
            } else if self.trait_chain_includes(name, &t.name) {
                self.error(format!(
                    "supertrait cycle: `{}` and `{name}` are mutually dependent",
                    t.name
                ));
            }
        }
    }

    fn check_fn(&mut self, f: &ast::FnDecl, self_ty: Option<&str>) {
        self.check_fn_body(&f.sig, &f.body, self_ty);
    }

    fn check_fn_body(&mut self, sig: &ast::FnSig, body: &ast::Block, self_ty: Option<&str>) {
        // Extend (not replace) the in-scope parameters/bounds with the
        // function's own, so an enclosing impl's generics stay visible.
        let prev_params = self.type_params.clone();
        let prev_bounds = self.type_bounds.clone();
        self.type_params
            .extend(sig.generics.iter().map(|g| g.name.clone()));
        for (k, v) in crate::collect::param_bounds(sig) {
            self.type_bounds.entry(k).or_default().extend(v);
        }
        // `self`'s type is the target — a type parameter (a blanket impl's
        // target) or a concrete type. `Self` in the signature is read the same.
        let self_target: Option<Ty> = self_ty.map(|t| {
            if self.type_params.iter().any(|p| p == t) {
                Ty::Param(t.to_string())
            } else {
                Ty::named(t)
            }
        });

        self.scopes.push();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => {
                    if let Some(st) = &self_target {
                        self.scopes.bind("self", st.clone());
                    }
                }
                ast::Param::Named { name, ty, span } => {
                    let lowered = self.lower(ty);
                    let pty = match &self_target {
                        Some(st) => lowered.subst_self(st),
                        None => lowered,
                    };
                    self.bind_local(name.clone(), pty, *span, crate::resolve::BindingKind::Param);
                }
            }
        }
        let lowered_ret = sig.ret.as_ref().map_or(Ty::Unit, |r| self.lower(r));
        let ret = match &self_target {
            Some(st) => lowered_ret.subst_self(st),
            None => lowered_ret,
        };
        let prev_ret = std::mem::replace(&mut self.ret_ty, ret.clone());

        let actual = self.infer_block(body);
        self.expect(&ret, &actual, "function body");

        self.ret_ty = prev_ret;
        self.scopes.pop();
        self.type_params = prev_params;
        self.type_bounds = prev_bounds;
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
        match self_ty {
            Some(target) => ty.subst_self(&Ty::named(target)),
            None => ty,
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
                method_span,
                args,
            } => self.infer_method_call(receiver, method, *method_span, args, span),
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
        ty.map(&|t| match t {
            Ty::Param(name) => Some(subst.get(name).cloned().unwrap_or(Ty::Unknown)),
            _ => None,
        })
    }

    /// Substitute the type parameters in `ty` from `subst`, leaving any
    /// parameter not in the map unchanged. Unlike [`Self::apply_subst`] (which
    /// erases unmapped parameters to `Unknown` for call-return instantiation),
    /// this is for projecting a field or method-return type of a nominal type
    /// through its concrete type arguments, where a parameter outside the
    /// nominal type's own generics must survive.
    fn subst_named(ty: &Ty, subst: &std::collections::HashMap<String, Ty>) -> Ty {
        ty.map(&|t| match t {
            Ty::Param(name) => Some(subst.get(name).cloned().unwrap_or_else(|| t.clone())),
            _ => None,
        })
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
        method_span: ast::Span,
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
                self.record_member(method_span, name, method);
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        }
        self.record_member(method_span, name, method);
        let Some(m) = self.table.methods.get(&(name.clone(), method.to_string())) else {
            // A blanket impl may grant the method: if the receiver satisfies a
            // blanket's conditions and its trait declares `method`, resolve
            // through that trait with `Self` read as the concrete type.
            if let Some((params, ret)) = self.blanket_method(name, &recv, method) {
                self.check_call_args(method, &params, &arg_tys, args, span);
                return ret;
            }
            self.error_at(span, format!("type `{name}` has no method `{method}`"));
            return Ty::Unknown;
        };
        // Project the method signature through the receiver's concrete type
        // arguments, so `Box<number>::get(self) -> T` yields `number`, then
        // check the call against the projected parameters.
        let subst = self.nominal_subst(&recv);
        let params: Vec<Ty> = m
            .params
            .iter()
            .map(|p| Self::subst_named(p, &subst))
            .collect();
        let ret = Self::subst_named(&m.ret, &subst);
        self.check_call_args(method, &params, &arg_tys, args, span);
        ret
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
            (ast::FieldAccess::Named(f, fspan), StructFields::Named(fields)) => {
                if let Some((_, ty)) = fields.iter().find(|(n, _)| n == f) {
                    let ty = Self::subst_named(ty, &subst);
                    self.record_member(*fspan, name, f);
                    ty
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
                match actual {
                    // A trait value flows into the same trait or a supertrait.
                    Ty::Named { name: a, .. } if self.is_trait(a) => {
                        self.trait_chain_includes(a, name)
                    }
                    _ => self.type_satisfies(actual, name),
                }
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
