//! Attribute well-formedness (`STATIC.md` §9).
//!
//! Every built-in attribute constrains what it may attach to and the shape of
//! its arguments. The parser accepts any `#[...]`; this pass turns a misapplied
//! or malformed attribute into a diagnostic so mistakes surface at check time
//! rather than being silently dropped by the backends.
//!
//! The recognised attributes are:
//!
//! - `#[derive(...)]` — only on a `struct` or `enum`; each argument names a
//!   built-in derivable trait (`Clone`, `Eq`, `Debug`, `Display`). Deriving
//!   `Eq` requires every field's type to support equality; deriving `Display`
//!   requires every field's type to itself satisfy `Display`.
//! - `#[extern(...)]` — only on an item inside an `extern` block; the closed
//!   param set is `ambient` / `path = "..."` / `name = "..."` / `constructor`
//!   (`lang/EXTERN.md` §Attributes). Validated by [`Checker::check_extern_block`],
//!   not the top-level item pass — extern items are nested, not module items.
//! - `#[test]` — only on a `fn` taking no parameters and returning `()` or
//!   `Result<(), E>`.
//!
//! Any other attribute name is unknown and reported as such: the compiler has
//! no tolerate-unknown-attributes policy, so an unrecognised `#[foo]` is a
//! mistake, not an extension point.

use neoc_ast as ast;

use crate::env::TypeDef;
use crate::infer::Checker;
use crate::ty::Ty;

/// The built-in derivable traits (`DYNAMIC.md` §6); mirrors `neoc_macros`.
const DERIVABLE: [&str; 4] = ["Clone", "Eq", "Debug", "Display"];

/// The recognised attribute names at module level. Anything else is unknown.
/// `#[extern(...)]` is valid only inside an `extern` block, validated
/// separately, so it is not listed here.
const KNOWN_ATTRS: [&str; 2] = ["derive", "test"];

impl Checker<'_> {
    /// Validates every attribute attached to an item against `STATIC.md` §9.
    ///
    /// Reports unknown attribute names, attributes applied to the wrong kind of
    /// item, unknown or malformed derive arguments, and (for `derive(Eq)` /
    /// `derive(Display)`) fields whose type does not support the derived trait.
    /// Diagnostics anchor to the item's span so the LSP can underline it.
    pub(crate) fn check_item_attrs(&mut self, item: &ast::Item) {
        for attr in &item.attrs {
            for entry in &attr.items {
                self.check_attr_item(entry, item);
            }
        }
        self.check_field_attrs(item);
    }

    /// Validates the attributes on a struct's or enum's fields: field-validation
    /// attributes (`#[min]` / `#[max]` / `#[max_len]`) must match the field's
    /// type, and no other attribute is permitted on a field. Diagnostics anchor
    /// to the item's span — `Field` nodes carry no span of their own.
    fn check_field_attrs(&mut self, item: &ast::Item) {
        let fields: Vec<(&str, &ast::Type, &[ast::Attribute])> = match &item.kind {
            ast::ItemKind::Struct(s) => match &s.body {
                ast::StructBody::Named(fields) => fields
                    .iter()
                    .map(|f| (f.name.as_str(), &f.ty, f.attrs.as_slice()))
                    .collect(),
                _ => Vec::new(),
            },
            ast::ItemKind::Enum(e) => e
                .variants
                .iter()
                .flat_map(|v| match &v.body {
                    ast::VariantBody::Named(fields) => fields
                        .iter()
                        .map(|f| (f.name.as_str(), &f.ty, f.attrs.as_slice()))
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        for (name, ast_ty, attrs) in fields {
            let ty = self.lower(ast_ty);
            for entry in attrs.iter().flat_map(|a| &a.items) {
                self.check_field_attr(entry, name, &ty, item.span);
            }
        }
    }

    fn check_field_attr(&mut self, entry: &ast::AttrItem, field: &str, ty: &Ty, span: ast::Span) {
        let ok = match entry.name.as_str() {
            "min" | "max" => matches!(ty, Ty::Number),
            "max_len" => matches!(ty, Ty::Str | Ty::List(_)),
            other => {
                self.error_at(
                    span,
                    format!("unknown attribute `{other}` on field `{field}`"),
                );
                return;
            }
        };
        if !ok {
            let wants = if entry.name == "max_len" {
                "a string or list"
            } else {
                "a number"
            };
            self.error_at(
                span,
                format!(
                    "`{}` on field `{field}` needs {wants}, not `{ty}`",
                    entry.name
                ),
            );
        }
    }

    fn check_attr_item(&mut self, entry: &ast::AttrItem, item: &ast::Item) {
        match entry.name.as_str() {
            "derive" => self.check_derive(entry, item),
            "test" => self.check_test(entry, item),
            // `#[extern(...)]` outside an `extern` block is misplaced; inside
            // one it is validated by `check_extern_block`.
            "extern" => self.error_at(
                item.span,
                "`extern` attributes apply only to items inside an `extern` block".to_string(),
            ),
            other => {
                debug_assert!(!KNOWN_ATTRS.contains(&other));
                self.error_at(item.span, format!("unknown attribute `{other}`"));
            }
        }
    }

    /// `#[derive(...)]`: struct/enum only, known derive names, non-empty,
    /// field-level `Eq`/`Display` obligations.
    fn check_derive(&mut self, entry: &ast::AttrItem, item: &ast::Item) {
        let on_type = matches!(item.kind, ast::ItemKind::Struct(_) | ast::ItemKind::Enum(_));
        if !on_type {
            self.error_at(
                item.span,
                "`derive` applies only to a struct or enum".to_string(),
            );
            return;
        }
        if entry.args.is_empty() {
            self.error_at(
                item.span,
                "`derive` needs at least one trait, e.g. `#[derive(Clone)]`".to_string(),
            );
            return;
        }
        for arg in &entry.args {
            let ast::AttrArg::Item(inner) = arg else {
                self.error_at(
                    item.span,
                    "`derive` arguments are trait names, e.g. `#[derive(Clone, Eq)]`".to_string(),
                );
                continue;
            };
            if !DERIVABLE.contains(&inner.name.as_str()) {
                self.error_at(
                    item.span,
                    format!(
                        "unknown derive `{}`; the derivable traits are Clone, Eq, Debug, Display",
                        inner.name
                    ),
                );
                continue;
            }
            match inner.name.as_str() {
                "Eq" => self.check_derive_fields(item, "eq", "Eq", "support equality"),
                "Display" => {
                    self.check_derive_fields(item, "display", "Display", "satisfy Display");
                }
                _ => {}
            }
        }
    }

    /// For `derive(Eq)` / `derive(Display)`: every field whose type is a
    /// user-defined `struct`/`enum` must itself carry `method` (i.e. derive or
    /// implement the trait). Primitives, lists, function types, type
    /// parameters, prelude types, and `ext` types are taken to satisfy it.
    fn check_derive_fields(&mut self, item: &ast::Item, method: &str, trait_: &str, needs: &str) {
        for (field, ty) in self.item_field_types(item) {
            if !self.ty_has_method(&ty, method) {
                self.error_at(
                    item.span,
                    format!("deriving {trait_} needs field `{field}` of type `{ty}` to {needs}"),
                );
            }
        }
    }

    /// The `(field name, field type)` pairs a struct or enum carries, lowered
    /// through the symbol table. A unit struct or an enum's unit variants
    /// contribute nothing.
    fn item_field_types(&self, item: &ast::Item) -> Vec<(String, Ty)> {
        let mut out = Vec::new();
        match &item.kind {
            ast::ItemKind::Struct(s) => self.collect_struct_fields(&s.body, &mut out),
            ast::ItemKind::Enum(e) => {
                for v in &e.variants {
                    match &v.body {
                        ast::VariantBody::Unit => {}
                        ast::VariantBody::Tuple(tys) => {
                            for (i, ty) in tys.iter().enumerate() {
                                out.push((format!("{}.{i}", v.name), self.lower(ty)));
                            }
                        }
                        ast::VariantBody::Named(fields) => {
                            for f in fields {
                                out.push((format!("{}.{}", v.name, f.name), self.lower(&f.ty)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    fn collect_struct_fields(&self, body: &ast::StructBody, out: &mut Vec<(String, Ty)>) {
        match body {
            ast::StructBody::Unit => {}
            ast::StructBody::Tuple(ty) => out.push(("0".to_string(), self.lower(ty))),
            ast::StructBody::Named(fields) => {
                for f in fields {
                    out.push((f.name.clone(), self.lower(&f.ty)));
                }
            }
        }
    }

    /// Whether `ty` carries `method` for the purpose of a derive obligation.
    ///
    /// Only a user-defined `struct`/`enum` can fail: it must have the method
    /// recorded (via `#[derive]` or an explicit `impl`). Every other type —
    /// primitive, list, function, type parameter, prelude `Result`/`Option`,
    /// `ext` type, or an unknown name — is treated as satisfying the trait, so
    /// the check never produces a false positive on a type the checker cannot
    /// reason about structurally.
    fn ty_has_method(&self, ty: &Ty, method: &str) -> bool {
        let Ty::Named { name, .. } = ty else {
            return true;
        };
        // The prelude `Option`/`Result` are stored as enums but carry no derived
        // methods; they are taken to satisfy the trait (as documented above).
        if matches!(name.as_str(), "Option" | "Result") {
            return true;
        }
        match self.table().types.get(name) {
            Some(TypeDef::Struct(_) | TypeDef::Enum(_)) => self
                .table()
                .methods
                .contains_key(&(name.clone(), method.to_string())),
            _ => true,
        }
    }

    /// Validates the items of an `extern` block (`lang/EXTERN.md` §Errors).
    ///
    /// Enforces body-elision, the closed `#[extern(...)]` param set, where each
    /// param may appear, and constructor/ambient rules. `ambient` is whether an
    /// enclosing `mod` is already ambient (it propagates to descendants, so
    /// re-applying it deeper is an error). `span` anchors block-level
    /// diagnostics. Recurses through nested `mod`s. Every message speaks neoc.
    pub(crate) fn check_extern_block(
        &mut self,
        items: &[ast::ExternItem],
        ambient: bool,
        span: ast::Span,
    ) {
        for ext in items {
            let attr = self.check_extern_attr(&ext.attrs, &ext.kind, ambient, span);
            let child_ambient = ambient || attr.ambient;
            match &ext.kind {
                ast::ExternItemKind::Fn(_) | ast::ExternItemKind::Struct(_) => {}
                ast::ExternItemKind::Impl(b) => {
                    for m in &b.items {
                        self.check_extern_method_attr(m, span);
                    }
                }
                ast::ExternItemKind::Mod(m) => {
                    self.check_extern_block(&m.items, child_ambient, span);
                }
            }
        }
    }

    /// Validates the `#[extern(...)]` attribute(s) on a non-method extern item
    /// (`mod`/`struct`/`fn`), returning the parsed attribute. Reports unknown
    /// bare attributes, path-based attributes, duplicate `#[extern]`, unknown
    /// params, params on the wrong item kind, and ambient re-application.
    fn check_extern_attr(
        &mut self,
        attrs: &[ast::Attribute],
        kind: &ast::ExternItemKind,
        ambient: bool,
        span: ast::Span,
    ) -> ast::ExternAttr {
        self.guard_extern_attr_shape(attrs, span);
        let parsed = ast::ExternAttr::parse(attrs).unwrap_or_default();
        let is_mod = matches!(kind, ast::ExternItemKind::Mod(_));
        if parsed.path.is_some() && !is_mod {
            self.error_at(
                span,
                "`#[extern(path = \"…\")]` applies only to a `mod`".to_string(),
            );
        }
        if parsed.name.is_some() && is_mod {
            self.error_at(
                span,
                "`#[extern(name = \"…\")]` applies to an `fn` or `struct`, not a `mod`".to_string(),
            );
        }
        if parsed.constructor {
            self.error_at(
                span,
                "`#[extern(constructor)]` applies only to an `fn` inside an `impl`".to_string(),
            );
        }
        if parsed.ambient && ambient {
            self.error_at(
                span,
                "`#[extern(ambient)]` is already in effect from an enclosing ambient `mod`"
                    .to_string(),
            );
        }
        parsed
    }

    /// Validates the `#[extern(...)]` attribute on an extern `impl` method.
    /// `constructor` is permitted here; `name` is incompatible with it, and a
    /// constructor takes no explicit return type.
    fn check_extern_method_attr(&mut self, m: &ast::FnDecl, span: ast::Span) {
        self.guard_extern_attr_shape(&m.attrs, span);
        let parsed = ast::ExternAttr::parse(&m.attrs).unwrap_or_default();
        if parsed.path.is_some() {
            self.error_at(
                span,
                "`#[extern(path = \"…\")]` applies only to a `mod`".to_string(),
            );
        }
        if parsed.constructor {
            if parsed.name.is_some() {
                self.error_at(
                    span,
                    "a constructor cannot also set `#[extern(name = \"…\")]`".to_string(),
                );
            }
            if m.sig.ret.is_some() {
                self.error_at(
                    span,
                    "a `#[extern(constructor)]` takes no return type — it constructs `Self`"
                        .to_string(),
                );
            }
        }
    }

    /// Reports the shape errors common to every `#[extern(...)]` site: an
    /// unknown bare attribute, a path-based attribute (reserved for future
    /// macros), a duplicate `#[extern(...)]`, or an unknown `#[extern(...)]`
    /// param.
    fn guard_extern_attr_shape(&mut self, attrs: &[ast::Attribute], span: ast::Span) {
        let mut extern_count = 0;
        for entry in attrs.iter().flat_map(|a| &a.items) {
            match entry.name.as_str() {
                "extern" => {
                    extern_count += 1;
                    self.check_extern_params(entry, span);
                }
                other => self.error_at(
                    span,
                    format!("unknown attribute `{other}` inside an `extern` block"),
                ),
            }
        }
        if extern_count > 1 {
            self.error_at(
                span,
                "only one `#[extern(…)]` attribute is allowed per item".to_string(),
            );
        }
    }

    /// Reports any unrecognised parameter of a `#[extern(...)]` attribute. The
    /// closed set is `ambient`, `constructor`, `path = "…"`, `name = "…"`.
    fn check_extern_params(&mut self, entry: &ast::AttrItem, span: ast::Span) {
        for arg in &entry.args {
            match arg {
                ast::AttrArg::Item(inner)
                    if matches!(inner.name.as_str(), "ambient" | "constructor") => {}
                ast::AttrArg::Named { name, value } if matches!(name.as_str(), "path" | "name") => {
                    if !matches!(value.as_ref(), ast::AttrArg::Str(_)) {
                        self.error_at(
                            span,
                            format!("`#[extern({name} = \"…\")]` needs a string value"),
                        );
                    }
                }
                ast::AttrArg::Item(inner) => self.error_at(
                    span,
                    format!("unknown `#[extern]` parameter `{}`", inner.name),
                ),
                ast::AttrArg::Named { name, .. } => {
                    self.error_at(span, format!("unknown `#[extern]` parameter `{name}`"));
                }
                _ => self.error_at(
                    span,
                    "`#[extern]` parameters are bare names or `key = \"value\"`".to_string(),
                ),
            }
        }
    }

    /// `#[test]`: a `fn` taking no parameters and returning `()` or
    /// `Result<(), E>`.
    fn check_test(&mut self, entry: &ast::AttrItem, item: &ast::Item) {
        if !entry.args.is_empty() {
            self.error_at(item.span, "`test` takes no arguments".to_string());
        }
        let ast::ItemKind::Fn(f) = &item.kind else {
            self.error_at(item.span, "`test` applies only to a function".to_string());
            return;
        };
        if !f.sig.params.is_empty() {
            self.error_at(
                item.span,
                "a `#[test]` function takes no parameters".to_string(),
            );
        }
        if !Self::is_test_return(f.sig.ret.as_ref()) {
            self.error_at(
                item.span,
                "a `#[test]` function returns `()` or `Result<(), E>`".to_string(),
            );
        }
    }

    /// Whether a function's return annotation is a valid `#[test]` return:
    /// absent / `()` (unit), or `Result<(), E>` for any `E`.
    fn is_test_return(ret: Option<&ast::Type>) -> bool {
        match ret {
            None | Some(ast::Type::Unit) => true,
            Some(ast::Type::Path(p)) => {
                let Some(seg) = p.segments.last() else {
                    return false;
                };
                seg.name == "Result"
                    && seg.args.len() == 2
                    && matches!(seg.args.first(), Some(ast::Type::Unit))
            }
            _ => false,
        }
    }
}
