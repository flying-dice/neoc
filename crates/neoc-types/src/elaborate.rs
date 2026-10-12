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
        let mut targets: Vec<String> = self
            .checker
            .table()
            .types
            .iter()
            .filter(|(_, def)| {
                matches!(
                    def,
                    crate::env::TypeDef::Struct(_) | crate::env::TypeDef::Enum(_)
                )
            })
            .map(|(name, _)| name.clone())
            .filter(|name| {
                let ty = Ty::named(name);
                conditions
                    .iter()
                    .all(|c| self.checker.type_satisfies(&ty, c))
            })
            .collect();
        targets.sort();
        targets
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
            body: self.fn_body(&f.sig, &f.body, self_ty),
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
                    .map(|body| self.fn_body(&ti.sig, body, Some("Self"))),
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

    /// Mirrors [`Checker::check_fn_body`]: enters the signature's type params,
    /// binds each parameter, then elaborates the body in that scope.
    fn fn_body(
        &mut self,
        sig: &ast::FnSig,
        body: &ast::Block,
        self_ty: Option<&str>,
    ) -> ast::Block {
        let prev_params = std::mem::take(&mut self.checker.type_params);
        let prev_bounds = std::mem::take(&mut self.checker.type_bounds);
        self.checker.type_params = sig.generics.iter().map(|g| g.name.clone()).collect();
        self.checker.type_bounds = crate::collect::param_bounds(sig);

        self.checker.scopes.push();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => {
                    if let Some(t) = self_ty {
                        self.checker.scopes.bind("self", Ty::named(t));
                    }
                }
                ast::Param::Named { name, ty, .. } => {
                    let pty = Checker::subst_self(self.checker.lower(ty), self_ty);
                    self.checker.scopes.bind(name.clone(), pty);
                }
            }
        }
        let ret = sig.ret.as_ref().map_or(Ty::Unit, |r| self.checker.lower(r));
        let ret = Checker::subst_self(ret, self_ty);
        let prev_ret = std::mem::replace(&mut self.checker.ret_ty, ret);
        let prev_target = std::mem::replace(
            &mut self.checker.impl_target,
            Checker::impl_target_of(self_ty),
        );

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
        self.checker.scopes.push();
        let stmts = block.stmts.iter().map(|s| self.stmt(s)).collect();
        let value = block.value.as_ref().map(|e| Box::new(self.expr(e)));
        self.checker.scopes.pop();
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
            ast::ExprKind::Call { callee, args } => ast::ExprKind::Call {
                callee: self.boxed(callee),
                args: self.exprs(args),
            },
            ast::ExprKind::MethodCall {
                receiver,
                method,
                method_span,
                args,
            } => ast::ExprKind::MethodCall {
                receiver: self.boxed(receiver),
                method: method.clone(),
                method_span: *method_span,
                args: self.exprs(args),
            },
            ast::ExprKind::Field { base, field } => ast::ExprKind::Field {
                base: self.boxed(base),
                field: field.clone(),
            },
            ast::ExprKind::Index { base, index } => ast::ExprKind::Index {
                base: self.boxed(base),
                index: self.boxed(index),
            },
            ast::ExprKind::StructLit { path, fields, base } => ast::ExprKind::StructLit {
                path: self.resolve_self_head(path),
                fields: fields
                    .iter()
                    .map(|f| ast::FieldInit {
                        name: f.name.clone(),
                        value: f.value.as_ref().map(|v| self.expr(v)),
                    })
                    .collect(),
                base: base.as_ref().map(|b| self.boxed(b)),
            },
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
            ast::ExprKind::Closure { params, body } => self.closure(params, body),
            ast::ExprKind::Block(b) => ast::ExprKind::Block(self.block(b)),
            ast::ExprKind::Return(value) => {
                ast::ExprKind::Return(value.as_ref().map(|e| self.boxed(e)))
            }
            // Leaves: literals, paths, unit, break, continue.
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
        let new_target = self.boxed(target);
        let new_value = self.boxed(value);
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

    fn closure(&mut self, params: &[ast::ClosureParam], body: &ast::Expr) -> ast::ExprKind {
        self.checker.scopes.push();
        for p in params {
            let ty = p.ty.as_ref().map_or(Ty::Unknown, |t| self.checker.lower(t));
            self.checker.scopes.bind(p.name.clone(), ty);
        }
        let new_body = self.expr(body);
        self.checker.scopes.pop();
        ast::ExprKind::Closure {
            params: params.to_vec(),
            body: Box::new(new_body),
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
    let checker = Checker::new(table);
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
