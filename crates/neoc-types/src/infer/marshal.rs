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
                if let ast::StructBody::Named(fields) = &s.body {
                    let generics: Vec<String> = s.generics.iter().map(|g| g.name.clone()).collect();
                    for f in fields {
                        self.check_extern_option_ty(&f.ty, &generics, span);
                    }
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
                    if !table.extern_types.contains(owner) {
                        return None;
                    }
                    let m = table.methods.get(&(owner.clone(), member.clone()))?;
                    return Some((m.params.clone(), m.ret.clone()));
                }
                if !table.extern_values.contains(member) {
                    return None;
                }
                fn_sig(table.values.get(member)?)
            }
        }
    }

    /// The extern signature of `receiver.method(..)`, if the receiver is a
    /// host-implemented type.
    pub(crate) fn extern_method_sig(
        &mut self,
        receiver: &ast::Expr,
        method: &str,
    ) -> Option<ExternSig> {
        let Ty::Named { name, .. } = self.infer(receiver) else {
            return None;
        };
        if !self.table().extern_types.contains(&name) {
            return None;
        }
        let m = self.table().methods.get(&(name, method.to_string()))?;
        Some((m.params.clone(), m.ret.clone()))
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
        let owner = path.last()?;
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
