//! Elaboration: rewrite string-typed `+` to `BinOp::Concat`.
//!
//! The parser emits [`ast::BinOp::Add`] for every `+`. neoc overloads `+` —
//! arithmetic on `number`, concatenation on `string` (`DYNAMIC.md` §4.2) — so
//! the emitter needs to know which `+` is a string join to pick Lua `..`. This
//! pass walks every body the checker walks, infers each `Add`'s operand types
//! through the real [`Checker`], and rewrites the node to [`ast::BinOp::Concat`]
//! when both operands are `string`. Everything else is copied unchanged.
//!
//! The walk mirrors the checker's scope handling (parameters, `let` bindings,
//! `match` arms, closures, `for` patterns) so that when it asks the checker for
//! an operand's type, the binding environment matches what [`Checker::infer`]
//! would see at that point. Type decisions reuse `infer`; this module never
//! re-implements inference.

use neoc_ast as ast;

use crate::collect::collect;
use crate::env::SymbolTable;
use crate::infer::Checker;
use crate::infer::marshal::{ExternSig, is_option};
use crate::ty::Ty;

/// The walking elaborator. Owns a [`Checker`] purely for its inference and
/// scope machinery; any diagnostics it would raise are discarded, as
/// elaboration runs after [`crate::check`] has already reported them.
pub struct Elaborator<'a> {
    checker: Checker<'a>,
}

impl<'a> Elaborator<'a> {
    fn new(checker: Checker<'a>) -> Self {
        Self { checker }
    }

    // ---- items -------------------------------------------------------------

    fn module(&mut self, module: &ast::Module) -> ast::Module {
        let mut items = Vec::with_capacity(module.items.len());
        let mut expansions = Vec::new();
        for item in &module.items {
            // A blanket impl has no runtime home (its target is a parameter), so
            // it is replaced by a concrete `impl Trait for X` for every type X
            // that satisfies its conditions — the methods dispatch identically.
            // Expansions are appended after all items, so each target type's
            // table is already declared when its blanket methods attach to it.
            if let ast::ItemKind::Impl(b) = &item.kind {
                if let Some(param) = blanket_param(b) {
                    expansions.extend(self.expand_blanket(item, b, &param));
                    continue;
                }
            }
            items.push(self.item(item));
        }
        items.extend(expansions);
        ast::Module { items }
    }

    /// Expands a blanket impl into one concrete impl per satisfying type. The
    /// satisfying types are sorted for deterministic emission.
    fn expand_blanket(
        &mut self,
        item: &ast::Item,
        b: &ast::ImplBlock,
        param: &str,
    ) -> Vec<ast::Item> {
        let conditions: Vec<String> = crate::collect::generic_param_bounds(&b.generics)
            .remove(param)
            .unwrap_or_default();
        self.checker
            .blanket_targets(&conditions)
            .into_iter()
            .map(|target| {
                let concrete = ast::ImplBlock {
                    generics: Vec::new(),
                    trait_: b.trait_.clone(),
                    ty: ast::Type::Path(ast::TypePath {
                        segments: vec![ast::TypeSegment {
                            name: target,
                            args: Vec::new(),
                        }],
                    }),
                    items: b.items.clone(),
                    assoc_bindings: Vec::new(),
                };
                ast::Item {
                    attrs: item.attrs.clone(),
                    vis: item.vis,
                    kind: ast::ItemKind::Impl(self.impl_block(&concrete)),
                    span: item.span,
                }
            })
            .collect()
    }

    fn item(&mut self, item: &ast::Item) -> ast::Item {
        let kind = match &item.kind {
            ast::ItemKind::Fn(f) => ast::ItemKind::Fn(self.fn_decl(f, None)),
            ast::ItemKind::Const(c) => ast::ItemKind::Const(ast::ConstDecl {
                name: c.name.clone(),
                ty: c.ty.clone(),
                value: self.expr(&c.value),
            }),
            ast::ItemKind::Impl(b) => ast::ItemKind::Impl(self.impl_block(b)),
            ast::ItemKind::Trait(t) => ast::ItemKind::Trait(self.trait_decl(t)),
            other => other.clone(),
        };
        ast::Item {
            attrs: item.attrs.clone(),
            vis: item.vis,
            kind,
            span: item.span,
        }
    }

    fn fn_decl(&mut self, f: &ast::FnDecl, self_ty: Option<&str>) -> ast::FnDecl {
        ast::FnDecl {
            attrs: f.attrs.clone(),
            sig: f.sig.clone(),
            body: self.fn_body(&f.sig, &f.body, self_ty, None),
        }
    }

    fn impl_block(&mut self, b: &ast::ImplBlock) -> ast::ImplBlock {
        let target = match &b.ty {
            ast::Type::Path(p) => p.segments.last().map(|s| s.name.clone()),
            _ => None,
        };
        let items = b
            .items
            .iter()
            .map(|f| self.fn_decl(f, target.as_deref()))
            .collect();
        ast::ImplBlock {
            generics: b.generics.clone(),
            trait_: b.trait_.clone(),
            ty: b.ty.clone(),
            items,
            assoc_bindings: b.assoc_bindings.clone(),
        }
    }

    fn trait_decl(&mut self, t: &ast::TraitDecl) -> ast::TraitDecl {
        let items = t
            .items
            .iter()
            .map(|ti| ast::TraitItem {
                sig: ti.sig.clone(),
                default: ti
                    .default
                    .as_ref()
                    .map(|body| self.fn_body(&ti.sig, body, Some("Self"), Some(&t.name))),
            })
            .collect();
        ast::TraitDecl {
            name: t.name.clone(),
            generics: t.generics.clone(),
            supertraits: t.supertraits.clone(),
            assoc_types: t.assoc_types.clone(),
            items,
        }
    }

    /// `ty` with `Self` read as `target`, when there is one.
    fn subst_target(ty: Ty, target: Option<&Ty>) -> Ty {
        match target {
            Some(t) => ty.subst_self(t),
            None => ty,
        }
    }

    /// Mirrors [`Checker::check_fn_body`]: enters the signature's type params,
    /// binds each parameter, then elaborates the body in that scope. For a
    /// default body of trait `trait_self`, `Self` is a parameter bounded by
    /// that trait (mirrors `Checker::check_trait_defaults`).
    fn fn_body(
        &mut self,
        sig: &ast::FnSig,
        body: &ast::Block,
        self_ty: Option<&str>,
        trait_self: Option<&str>,
    ) -> ast::Block {
        let prev_params = std::mem::take(&mut self.checker.type_params);
        let prev_bounds = std::mem::take(&mut self.checker.type_bounds);
        self.checker.type_params = sig.generics.iter().map(|g| g.name.clone()).collect();
        self.checker.type_bounds = crate::collect::param_bounds(sig);
        if let Some(trait_name) = trait_self {
            self.checker.enter_trait_self(trait_name);
        }
        let self_target = self_ty.map(|t| self.checker.self_target(t));

        self.checker.scopes.push_fn();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => {
                    if let Some(st) = &self_target {
                        self.checker.scopes.bind("self", st.clone());
                    }
                }
                ast::Param::Named { name, ty, .. } => {
                    let pty = Self::subst_target(self.checker.lower(ty), self_target.as_ref());
                    self.checker.scopes.bind(name.clone(), pty);
                }
            }
        }
        let ret = sig.ret.as_ref().map_or(Ty::Unit, |r| self.checker.lower(r));
        let ret = Self::subst_target(ret, self_target.as_ref());
        let prev_ret = std::mem::replace(&mut self.checker.ret_ty, ret);
        let prev_target =
            std::mem::replace(&mut self.checker.impl_target, self_ty.map(str::to_string));

        let block = self.block(body);

        self.checker.impl_target = prev_target;
        self.checker.ret_ty = prev_ret;
        self.checker.scopes.pop();
        self.checker.type_params = prev_params;
        self.checker.type_bounds = prev_bounds;
        block
    }

    /// `path` with a bare `Self` head read as the enclosing impl's target, so the
    /// lowering sees the struct's own name.
    fn resolve_self_head(&self, path: &[ast::Ident]) -> Vec<ast::Ident> {
        match (path, &self.checker.impl_target) {
            ([head], Some(target)) if head == "Self" => vec![target.clone()],
            _ => path.to_vec(),
        }
    }

    // ---- statements & blocks ----------------------------------------------

    fn block(&mut self, block: &ast::Block) -> ast::Block {
        self.checker.enter_block(&block.stmts);
        let stmts = block.stmts.iter().map(|s| self.stmt(s)).collect();
        let value = block.value.as_ref().map(|e| Box::new(self.expr(e)));
        self.checker.exit_block();
        ast::Block { stmts, value }
    }

    fn stmt(&mut self, stmt: &ast::Stmt) -> ast::Stmt {
        let kind = match &stmt.kind {
            ast::StmtKind::Let(l) => ast::StmtKind::Let(self.let_stmt(l)),
            ast::StmtKind::Expr(e) => ast::StmtKind::Expr(self.expr(e)),
            ast::StmtKind::Item(item) => ast::StmtKind::Item(self.item(item)),
        };
        ast::Stmt::new(kind, stmt.span)
    }

    /// Mirrors [`Checker::check_let`]: elaborate the initialiser, then bind the
    /// pattern at the same type the checker would, so later operands see it.
    fn let_stmt(&mut self, l: &ast::LetStmt) -> ast::LetStmt {
        let value = l.value.as_ref().map(|e| self.expr(e));
        let declared = l.ty.as_ref().map(|t| self.checker.lower(t));
        let value_ty = l.value.as_ref().map(|e| self.checker.infer(e));
        let ty = match (&declared, &value_ty) {
            (Some(d), _) => d.clone(),
            (None, Some(v)) => v.clone(),
            (None, None) => Ty::Unknown,
        };
        // Elaboration does not record resolution; the kind is immaterial here.
        self.checker
            .bind_pattern(&l.pattern, &ty, crate::resolve::BindingKind::Let);
        ast::LetStmt {
            pattern: l.pattern.clone(),
            ty: l.ty.clone(),
            value,
        }
    }

    // ---- expressions -------------------------------------------------------

    /// Elaborates an expression, preserving its byte [`ast::Span`]. Rewritten
    /// nodes (a string-typed `+` becoming `Concat`, an `==` on an `Eq` type
    /// becoming an `.eq` call) inherit the original expression's span so a
    /// later diagnostic still points at the source the user wrote.
    fn expr(&mut self, expr: &ast::Expr) -> ast::Expr {
        let span = expr.span;
        let kind = match &expr.kind {
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, span),
            ast::ExprKind::Unary { op, operand } => ast::ExprKind::Unary {
                op: *op,
                operand: self.boxed(operand),
            },
            ast::ExprKind::Range {
                start,
                end,
                inclusive,
            } => ast::ExprKind::Range {
                start: self.boxed(start),
                end: self.boxed(end),
                inclusive: *inclusive,
            },
            ast::ExprKind::Try(inner) => ast::ExprKind::Try(self.boxed(inner)),
            ast::ExprKind::Call { callee, args } => self.call(callee, args, span),
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span,
                args,
            } => self.method_call(receiver, method, *method_span, args, span),
            ast::ExprKind::Field { base, field } => self.field(base, field, span),
            ast::ExprKind::Index { base, index } => ast::ExprKind::Index {
                base: self.boxed(base),
                index: self.boxed(index),
            },
            ast::ExprKind::StructLit { path, fields, base } => {
                let head = self.resolve_self_head(path);
                ast::ExprKind::StructLit {
                    path: head.clone(),
                    fields: fields.iter().map(|f| self.field_init(&head, f)).collect(),
                    base: base.as_ref().map(|b| self.boxed(b)),
                }
            }
            ast::ExprKind::List(items) => ast::ExprKind::List(self.exprs(items)),
            ast::ExprKind::Assign { target, op, value } => self.assign(target, *op, value),
            ast::ExprKind::If { cond, then, else_ } => ast::ExprKind::If {
                cond: self.boxed(cond),
                then: self.block(then),
                else_: else_.as_ref().map(|e| self.boxed(e)),
            },
            ast::ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms),
            ast::ExprKind::While { cond, body } => ast::ExprKind::While {
                cond: self.boxed(cond),
                body: self.block(body),
            },
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => self.for_expr(pattern, iter, body),
            ast::ExprKind::Loop { body } => ast::ExprKind::Loop {
                body: self.block(body),
            },
            ast::ExprKind::Closure { params, body } => self.closure(params, body, span),
            ast::ExprKind::Block(b) => ast::ExprKind::Block(self.block(b)),
            ast::ExprKind::Return(value) => {
                ast::ExprKind::Return(value.as_ref().map(|e| self.boxed(e)))
            }
            // Leaves: literals, unit, break, continue.
            other => other.clone(),
        };
        ast::Expr::new(kind, span)
    }

    /// The binary nodes that can change. A `+` whose operands are both `string`
    /// becomes [`ast::BinOp::Concat`] so the emitter picks Lua `..`. An `==`/`!=`
    /// whose operands are a `#[derive(Eq)]` type becomes a call to that type's
    /// structural `eq` method (`DYNAMIC.md` §4.2 / §6.2) — `a == b` to
    /// `a.eq(b)`, `a != b` to `!a.eq(b)` — because Lua `==` on tables compares
    /// identity, not structure. Everything else is copied unchanged.
    fn binary(
        &mut self,
        op: ast::BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: ast::Span,
    ) -> ast::ExprKind {
        let new_lhs = self.expr(lhs);
        let new_rhs = self.expr(rhs);

        if matches!(op, ast::BinOp::Eq | ast::BinOp::Ne) {
            let lt = self.checker.infer(lhs);
            if self.checker.type_derives_eq(&lt) {
                // The rewritten `.eq` call (and the `!` wrapper for `!=`) keeps
                // the original comparison's span so a diagnostic still points at
                // the source the user wrote.
                let call = ast::Expr::new(
                    ast::ExprKind::MethodCall {
                        receiver: Box::new(new_lhs),
                        method: "eq".to_string(),
                        method_span: span,
                        args: vec![new_rhs],
                    },
                    span,
                );
                return match op {
                    ast::BinOp::Eq => call.kind,
                    _ => ast::ExprKind::Unary {
                        op: ast::UnOp::Not,
                        operand: Box::new(call),
                    },
                };
            }
        }

        let op = if matches!(op, ast::BinOp::Add) {
            let lt = self.checker.infer(lhs);
            let rt = self.checker.infer(rhs);
            if lt == Ty::Str && rt == Ty::Str {
                ast::BinOp::Concat
            } else {
                op
            }
        } else {
            op
        };
        ast::ExprKind::Binary {
            op,
            lhs: Box::new(new_lhs),
            rhs: Box::new(new_rhs),
        }
    }

    /// An assignment. A string-typed `p += v` elaborates to the compound op
    /// [`ast::AssignOp::Concat`] (`STATIC.md` §4.1, compound assignment), so the
    /// emitter lowers it with Lua `..` and still evaluates the place `p` once.
    /// Every other assignment is copied with its operands elaborated.
    fn assign(
        &mut self,
        target: &ast::Expr,
        op: ast::AssignOp,
        value: &ast::Expr,
    ) -> ast::ExprKind {
        let new_target = Box::new(self.place(target));
        let new_value = Box::new(match self.extern_place_ty(target) {
            Some(ty) => marshal_to_host(&ty, self.expr(value)),
            None => self.expr(value),
        });
        let op = if op == ast::AssignOp::Add
            && self.checker.infer(target) == Ty::Str
            && self.checker.infer(value) == Ty::Str
        {
            ast::AssignOp::Concat
        } else {
            op
        };
        ast::ExprKind::Assign {
            target: new_target,
            op,
            value: new_value,
        }
    }

    fn match_expr(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) -> ast::ExprKind {
        let new_scrutinee = self.expr(scrutinee);
        let scrut = self.checker.infer(scrutinee);
        let new_arms = arms
            .iter()
            .map(|arm| {
                self.checker.scopes.push();
                self.checker.bind_pattern(
                    &arm.pattern,
                    &scrut,
                    crate::resolve::BindingKind::MatchArm,
                );
                let guard = arm.guard.as_ref().map(|g| self.expr(g));
                let body = self.expr(&arm.body);
                self.checker.scopes.pop();
                ast::MatchArm {
                    pattern: arm.pattern.clone(),
                    guard,
                    body,
                }
            })
            .collect();
        ast::ExprKind::Match {
            scrutinee: Box::new(new_scrutinee),
            arms: new_arms,
        }
    }

    fn for_expr(
        &mut self,
        pattern: &ast::Pattern,
        iter: &ast::Expr,
        body: &ast::Block,
    ) -> ast::ExprKind {
        let new_iter = self.expr(iter);
        let elem = match self.checker.infer(iter) {
            Ty::List(inner) => *inner,
            _ => Ty::Unknown,
        };
        self.checker.scopes.push();
        self.checker
            .bind_pattern(pattern, &elem, crate::resolve::BindingKind::ForPattern);
        let new_body = self.block(body);
        self.checker.scopes.pop();
        ast::ExprKind::For {
            pattern: pattern.clone(),
            iter: Box::new(new_iter),
            body: new_body,
        }
    }

    /// A closure. Parameters bind at the types the checker recorded for it.
    fn closure(
        &mut self,
        params: &[ast::ClosureParam],
        body: &ast::Expr,
        span: ast::Span,
    ) -> ast::ExprKind {
        self.checker.scopes.push();
        let recorded = self.checker.closure_param_tys(span).cloned();
        for (i, p) in params.iter().enumerate() {
            let ty = recorded
                .as_ref()
                .and_then(|r| r.get(i).cloned())
                .or_else(|| p.ty.as_ref().map(|t| self.checker.lower(t)))
                .unwrap_or(Ty::Unknown);
            self.checker.scopes.bind(p.name.clone(), ty);
        }
        let new_body = self.expr(body);
        self.checker.scopes.pop();
        ast::ExprKind::Closure {
            params: params.to_vec(),
            body: Box::new(new_body),
        }
    }

    // ---- extern boundary ---------------------------------------------------
    //
    // `Option<T>` crosses the boundary as `nil` / the bare value (`lang/EXTERN.md`
    // §Option marshalling). Every call, field read, field write, and struct
    // literal that touches a host-implemented signature is wrapped in a
    // `Marshal` node here, where the types are known.

    /// A call. Arguments bound for an `Option` parameter of a host-implemented
    /// callee unwrap; an `Option` result wraps.
    fn call(&mut self, callee: &ast::Expr, args: &[ast::Expr], span: ast::Span) -> ast::ExprKind {
        let sig = self.checker.extern_callee_sig(callee);
        let new_callee = match &callee.kind {
            ast::ExprKind::Path(_) => callee.clone(),
            _ => self.expr(callee),
        };
        let args = self.exprs(args);
        let call = ast::ExprKind::Call {
            callee: Box::new(new_callee),
            args: marshal_args(sig.as_ref(), args),
        };
        marshal_result(sig.as_ref(), call, span)
    }

    fn method_call(
        &mut self,
        receiver: &ast::Expr,
        method: &str,
        method_span: ast::Span,
        args: &[ast::Expr],
        span: ast::Span,
    ) -> ast::ExprKind {
        let sig = self.checker.extern_method_sig(receiver, method);
        let args = self.exprs(args);
        let call = ast::ExprKind::MethodCall {
            receiver: self.boxed(receiver),
            method: method.to_string(),
            method_span,
            args: marshal_args(sig.as_ref(), args),
        };
        marshal_result(sig.as_ref(), call, span)
    }

    /// A field read. An `Option` field of a host-implemented struct wraps.
    fn field(
        &mut self,
        base: &ast::Expr,
        field: &ast::FieldAccess,
        span: ast::Span,
    ) -> ast::ExprKind {
        let ty = self.checker.extern_field_read_ty(base, field);
        let read = ast::ExprKind::Field {
            base: self.boxed(base),
            field: field.clone(),
        };
        marshal_result(ty.map(|t| (Vec::new(), t)).as_ref(), read, span)
    }

    /// An assignment target: a field place keeps its own read un-wrapped (the
    /// write marshals the value instead); any other target elaborates as usual.
    fn place(&mut self, target: &ast::Expr) -> ast::Expr {
        match &target.kind {
            ast::ExprKind::Field { base, field } => ast::Expr::new(
                ast::ExprKind::Field {
                    base: self.boxed(base),
                    field: field.clone(),
                },
                target.span,
            ),
            _ => self.expr(target),
        }
    }

    /// The declared type of an assignment target that is a field of a
    /// host-implemented struct.
    fn extern_place_ty(&mut self, target: &ast::Expr) -> Option<Ty> {
        let ast::ExprKind::Field { base, field } = &target.kind else {
            return None;
        };
        self.checker.extern_field_read_ty(base, field)
    }

    /// One `name: value` of a struct literal. A field of a host-implemented
    /// struct typed `Option<T>` unwraps; shorthand expands to the named value.
    fn field_init(&mut self, path: &[ast::Ident], init: &ast::FieldInit) -> ast::FieldInit {
        let ty = self.checker.extern_lit_field_ty(path, &init.name);
        let value = match (&init.value, ty) {
            (Some(v), Some(ty)) => Some(marshal_to_host(&ty, self.expr(v))),
            (Some(v), None) => Some(self.expr(v)),
            (None, Some(ty)) if is_option(&ty) => {
                let named = ast::Expr::new(
                    ast::ExprKind::Path(vec![init.name.clone()]),
                    ast::Span::default(),
                );
                Some(marshal_to_host(&ty, named))
            }
            (None, _) => None,
        };
        ast::FieldInit {
            name: init.name.clone(),
            value,
        }
    }

    // ---- helpers -----------------------------------------------------------

    fn boxed(&mut self, expr: &ast::Expr) -> Box<ast::Expr> {
        Box::new(self.expr(expr))
    }

    fn exprs(&mut self, exprs: &[ast::Expr]) -> Vec<ast::Expr> {
        exprs.iter().map(|e| self.expr(e)).collect()
    }
}

/// Unwraps each argument bound for an `Option` parameter of an extern `sig`.
fn marshal_args(sig: Option<&ExternSig>, args: Vec<ast::Expr>) -> Vec<ast::Expr> {
    let Some((params, _)) = sig else { return args };
    args.into_iter()
        .enumerate()
        .map(|(i, arg)| match params.get(i) {
            Some(ty) => marshal_to_host(ty, arg),
            None => arg,
        })
        .collect()
}

/// `arg` as the host receives a value of declared type `ty`: an `Option`
/// unwraps to `nil` / the bare value, anything else passes through.
fn marshal_to_host(ty: &Ty, arg: ast::Expr) -> ast::Expr {
    if !is_option(ty) {
        return arg;
    }
    let span = arg.span;
    ast::Expr::new(
        ast::ExprKind::Marshal {
            direction: ast::MarshalDirection::ToHost,
            operand: Box::new(arg),
        },
        span,
    )
}

/// Wraps the extern result `kind` when the host declares it `Option<T>`.
fn marshal_result(sig: Option<&ExternSig>, kind: ast::ExprKind, span: ast::Span) -> ast::ExprKind {
    match sig {
        Some((_, ret)) if is_option(ret) => ast::ExprKind::Marshal {
            direction: ast::MarshalDirection::FromHost,
            operand: Box::new(ast::Expr::new(kind, span)),
        },
        _ => kind,
    }
}

/// Returns a copy of `module` with every string-typed `+` (a [`ast::BinOp::Add`]
/// whose operands both infer to `string`) rewritten to [`ast::BinOp::Concat`],
/// so the emitter can lower it to Lua `..` rather than `+`. Every other
/// expression is structurally unchanged.
///
/// Operand types are determined by the same inference [`crate::check`] uses: the
/// pass walks each function, method, `const`, and trait-default body in the
/// scope the checker would build (parameters, `let` bindings, `match` arms,
/// closures, `for` patterns) and consults [`Checker::infer`] at each `+`. When
/// either operand's type is unknown or ambiguous — a generic parameter, an
/// unresolved name — the node is left as [`ast::BinOp::Add`] rather than
/// guessing concatenation.
///
/// # Arguments
///
/// * `module` — the parsed source file to elaborate. The caller owns it; this
///   function borrows it and returns a fresh, owned [`ast::Module`].
#[must_use]
pub fn elaborate(module: &ast::Module) -> ast::Module {
    let table = collect(module);
    elaborate_with_table(module, &table)
}

/// Elaborates a module against a caller-supplied [`SymbolTable`].
///
/// Like [`elaborate`], but the table is provided rather than freshly collected
/// from the module. A multi-file driver uses this so a `+` whose operand type
/// resolves through an imported `pub` item is still inferred as `string` and
/// rewritten to [`ast::BinOp::Concat`]; with a local-only table the operand's
/// type would be unknown and the node would stay [`ast::BinOp::Add`], emitting
/// arithmetic `+` for what is a concatenation. Mirrors [`crate::check_with_table`]:
/// collect the module's own declarations first, then add the `use` imports.
///
/// # Arguments
///
/// * `module` — the parsed source file to elaborate. The caller owns it; this
///   function borrows it and returns a fresh, owned [`ast::Module`].
/// * `table` — the symbol table to resolve operand types against. Ownership
///   stays with the caller; the elaborator borrows it for the call.
#[must_use]
pub fn elaborate_with_table(module: &ast::Module, table: &SymbolTable) -> ast::Module {
    // The checker decides what each closure parameter's type is; the
    // elaborator reads that record rather than re-deriving it.
    let mut first_pass = Checker::new(table);
    first_pass.check_module(module);
    let mut checker = Checker::new(table);
    checker.adopt_closure_params(first_pass);
    Elaborator::new(checker).module(module)
}

/// The target parameter of a blanket impl `impl<T: …> Trait for T`, or `None`
/// for a concrete or inherent impl.
fn blanket_param(b: &ast::ImplBlock) -> Option<String> {
    b.trait_.as_ref()?;
    let target = match &b.ty {
        ast::Type::Path(p) => p.segments.last().map(|s| s.name.clone())?,
        _ => return None,
    };
    b.generics
        .iter()
        .any(|g| g.name == target)
        .then_some(target)
}
