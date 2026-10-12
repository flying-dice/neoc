//! `Option<T>` at the `extern` boundary (`lang/EXTERN.md` §Option marshalling).
//!
//! Two halves share this file. The well-formedness rule rejects an extern
//! signature in which `Option` cannot marshal unambiguously. The lookups tell
//! elaboration which call, field read, or field write crosses the boundary and
//! what type the host sees there.

use neoc_ast as ast;

use super::Checker;
use crate::collect::lower_type;
use crate::env::{StructFields, TypeDef};
use crate::ty::Ty;

/// Whether `ty` is `Option<_>`.
pub(crate) fn is_option(ty: &Ty) -> bool {
    matches!(ty, Ty::Named { name, .. } if name == "Option")
}

/// Whether `Option` occurs anywhere inside `ty`.
fn contains_option(ty: &Ty) -> bool {
    match ty {
        Ty::Named { name, args } => name == "Option" || args.iter().any(contains_option),
        Ty::List(inner) => contains_option(inner),
        Ty::Fn(params, ret) => params.iter().any(contains_option) || contains_option(ret),
        _ => false,
    }
}

/// The signature a call crosses the boundary with: parameter types (receiver
/// excluded) and the return type.
pub(crate) type ExternSig = (Vec<Ty>, Ty);

impl Checker<'_> {
    /// Reports every `Option` an extern signature or field type cannot marshal:
    /// one nested inside another `Option`, and one that is not the whole
    /// parameter, return, or field type. `span` anchors the diagnostic.
    pub(crate) fn check_extern_option_types(
        &mut self,
        kind: &ast::ExternItemKind,
        span: ast::Span,
    ) {
        match kind {
            ast::ExternItemKind::Fn(sig) => self.check_extern_sig_options(sig, span),
            ast::ExternItemKind::Impl(b) => {
                for m in &b.items {
                    self.check_extern_sig_options(&m.sig, span);
                }
            }
            ast::ExternItemKind::Struct(s) => {
                let generics: Vec<String> = s.generics.iter().map(|g| g.name.clone()).collect();
                match &s.body {
                    ast::StructBody::Named(fields) => {
                        for f in fields {
                            self.check_extern_option_ty(&f.ty, &generics, span);
                        }
                    }
                    ast::StructBody::Tuple(ty) => {
                        let ty = self.table().resolve_alias_ty(lower_type(ty, &generics));
                        if contains_option(&ty) {
                            self.error_at(
                                span,
                                format!(
                                    "`{ty}` cannot be the field of an `extern` tuple struct: an `Option` field \
                                     needs a name to marshal; declare a named field"
                                ),
                            );
                        }
                    }
                    ast::StructBody::Unit => {}
                }
            }
            ast::ExternItemKind::Mod(_) => {}
        }
    }

    fn check_extern_sig_options(&mut self, sig: &ast::FnSig, span: ast::Span) {
        let generics: Vec<String> = sig.generics.iter().map(|g| g.name.clone()).collect();
        for p in &sig.params {
            if let ast::Param::Named { ty, .. } = p {
                self.check_extern_option_ty(ty, &generics, span);
            }
        }
        if let Some(ret) = &sig.ret {
            self.check_extern_option_ty(ret, &generics, span);
        }
    }

    fn check_extern_option_ty(&mut self, ty: &ast::Type, generics: &[String], span: ast::Span) {
        let ty = self.table().resolve_alias_ty(lower_type(ty, generics));
        if let Ty::Named { name, args } = &ty {
            if name == "Option" {
                if args.iter().any(contains_option) {
                    self.error_at(
                        span,
                        format!(
                            "`{ty}` is ambiguous across an `extern` boundary: `Some(None)` and `None` both reach the host as `nil`"
                        ),
                    );
                }
                return;
            }
        }
        if contains_option(&ty) {
            self.error_at(
                span,
                format!(
                    "`{ty}` puts an `Option` inside another type at an `extern` boundary; \
                     `Option<T>` marshals only as a whole parameter, return, or field type"
                ),
            );
        }
    }

    /// The extern signature a call to `callee` crosses, if it names a
    /// host-implemented function, associated function, or constructor.
    pub(crate) fn extern_callee_sig(&self, callee: &ast::Expr) -> Option<ExternSig> {
        let ast::ExprKind::Path(segs) = &callee.kind else {
            return None;
        };
        let table = self.table();
        match segs.as_slice() {
            [] => None,
            [name] => {
                if self.scopes.lookup(name).is_some() || !table.extern_values.contains(name) {
                    return None;
                }
                fn_sig(table.values.get(name)?)
            }
            [.., owner, member] => {
                if table.types.contains_key(owner) {
                    return self.host_method_sig(owner, member);
                }
                if !table.extern_values.contains(member) {
                    return None;
                }
                fn_sig(table.values.get(member)?)
            }
        }
    }

    /// Marks `expr` as a place where the host receives a named extern `fn`
    /// raw, when `expr` is just that name.
    pub(crate) fn allow_host_fn(&mut self, expr: &ast::Expr) {
        if matches!(expr.kind, ast::ExprKind::Path(_)) {
            self.host_fn_spans.insert((expr.span.start, expr.span.end));
        }
    }

    /// Rejects a host-implemented `fn` whose signature carries an `Option`
    /// when it is named as a value anywhere the host does not receive it raw.
    pub(crate) fn check_extern_fn_value(&mut self, expr: &ast::Expr) {
        let Some((params, ret)) = self.extern_callee_sig(expr) else {
            return;
        };
        if !params.iter().any(is_option) && !is_option(&ret) {
            return;
        }
        if self
            .host_fn_spans
            .contains(&(expr.span.start, expr.span.end))
        {
            return;
        }
        self.error_at(
            expr.span,
            "a host function that takes or returns an `Option` can be called, or handed \
             directly to another host function; it cannot be stored or passed on as a value"
                .to_string(),
        );
    }

    /// Whether `owner.member` is a host-implemented method.
    pub(crate) fn is_host_method(&self, owner: &str, member: &str) -> bool {
        self.host_method_sig(owner, member).is_some()
    }

    /// Whether `name` is a host-implemented type.
    pub(crate) fn is_extern_type(&self, name: &str) -> bool {
        self.table().extern_types.contains(name)
    }

    /// Whether `ty` names a host-implemented type.
    pub(crate) fn is_extern_ty(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Named { name, .. } if self.is_extern_type(name))
    }

    /// Infers `expr` without keeping the diagnostics it produces.
    pub(crate) fn quiet_infer(&mut self, expr: &ast::Expr) -> Ty {
        let kept = self.diagnostics.len();
        let ty = self.infer(expr);
        self.diagnostics.truncate(kept);
        ty
    }

    /// The extern signature of `receiver.method(..)`, if the host implements
    /// that method. A neoc `impl` method on an extern type does not cross.
    pub(crate) fn extern_method_sig(
        &mut self,
        receiver: &ast::Expr,
        method: &str,
    ) -> Option<ExternSig> {
        let Ty::Named { name, .. } = self.infer(receiver) else {
            return None;
        };
        self.host_method_sig(&name, method)
    }

    /// The signature of `owner::member`, if the host implements it.
    fn host_method_sig(&self, owner: &str, member: &str) -> Option<ExternSig> {
        let m = self
            .table()
            .methods
            .get(&(owner.to_string(), member.to_string()))
            .filter(|m| m.host)?;
        Some((m.params.clone(), m.ret.clone()))
    }

    /// Whether a struct pattern against a value of type `scrutinee` reads
    /// `Option` field `ty` raw from the host — `scrutinee` is host-implemented
    /// and `ty` is `Option<_>`.
    pub(crate) fn is_host_option_field(&self, scrutinee: &Ty, ty: &Ty) -> bool {
        self.is_extern_ty(scrutinee) && is_option(ty)
    }

    /// Whether a struct literal headed `path` builds a host-implemented
    /// struct. `Self` reads as the enclosing impl's target; a qualified path
    /// names an enum variant, never a host struct.
    pub(crate) fn is_host_struct_head(&self, path: &[ast::Ident]) -> bool {
        match path {
            [head] if head == "Self" => self
                .impl_target
                .as_deref()
                .is_some_and(|t| self.is_extern_type(t)),
            [head] => self.is_extern_type(head),
            _ => false,
        }
    }

    /// The declared type of `field` on host-implemented struct `owner`.
    pub(crate) fn extern_field_ty(&self, owner: &str, field: &ast::FieldAccess) -> Option<Ty> {
        let table = self.table();
        if !table.extern_types.contains(owner) {
            return None;
        }
        let (Some(TypeDef::Struct(s)), ast::FieldAccess::Named(name, _)) =
            (table.types.get(owner), field)
        else {
            return None;
        };
        match &s.fields {
            StructFields::Named(fields) => fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, ty)| ty.clone()),
            _ => None,
        }
    }

    /// The declared type of field `name` on the struct literal `path`, when
    /// that struct is host-implemented.
    pub(crate) fn extern_lit_field_ty(&self, path: &[ast::Ident], name: &str) -> Option<Ty> {
        let [owner] = path else { return None };
        self.extern_field_ty(
            owner,
            &ast::FieldAccess::Named(name.to_string(), ast::Span::default()),
        )
    }

    /// The type of the host-implemented value `expr` reads, when it is a
    /// field access on a host-implemented struct.
    pub(crate) fn extern_field_read_ty(
        &mut self,
        base: &ast::Expr,
        field: &ast::FieldAccess,
    ) -> Option<Ty> {
        let Ty::Named { name, .. } = self.infer(base) else {
            return None;
        };
        self.extern_field_ty(&name, field)
    }
}

fn fn_sig(ty: &Ty) -> Option<ExternSig> {
    match ty {
        Ty::Fn(params, ret) => Some((params.clone(), (**ret).clone())),
        _ => None,
    }
}
