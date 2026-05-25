//! Built-in derive macros lowered to Python: `Clone`, `Eq`, `Debug`,
//! `Display`.
//!
//! Each derive is structural — it reads a `struct`'s fields or an `enum`'s
//! variants from the AST and attaches a method, with no type information. The
//! runtime conventions match `lang/DYNAMIC.md` §6–§7 and the Lua backend:
//!
//! * a struct value is an instance carrying its fields as attributes (a newtype
//!   payload at `_0`);
//! * an enum value is a runtime `_Enum` instance with a `tag` attribute, a tuple
//!   payload at `_0`, `_1`, … and a named payload by field name;
//! * methods are plain functions taking the receiver first.
//!
//! `Debug`/`Display` route each value through the runtime `_fmt`, so a whole
//! number prints `1` (not `1.0`) and a boolean prints `true`/`false` — matching
//! the Lua/Rust goldens. `Eq` compares each field with Python `==`.

use crate::Emitter;
use neoc_ast as ast;

/// A single data slot in a value: a named field, or a positional index (newtype
/// / tuple payload, stored under attribute `_<i>`).
#[derive(Clone, Copy)]
enum DataKey<'a> {
    Named(&'a str),
    Index(usize),
}

impl DataKey<'_> {
    /// Attribute access for the key on receiver `recv`, e.g. `recv.x` or
    /// `recv._0`.
    fn access(self, recv: &str) -> String {
        match self {
            DataKey::Named(name) => format!("{recv}.{name}"),
            DataKey::Index(i) => format!("{recv}._{i}"),
        }
    }

    /// Human-readable label for Debug output: the field name, or `_<i>` for a
    /// positional slot.
    fn label(self) -> String {
        match self {
            DataKey::Named(name) => name.to_string(),
            DataKey::Index(i) => format!("_{i}"),
        }
    }
}

/// The data keys carrying a struct's fields: every named field, or `_0` for a
/// newtype. A unit struct has none.
fn struct_keys(body: &ast::StructBody) -> Vec<DataKey<'_>> {
    match body {
        ast::StructBody::Unit => Vec::new(),
        ast::StructBody::Tuple(_) => vec![DataKey::Index(0)],
        ast::StructBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

/// The data keys carrying a variant's payload.
fn variant_keys(body: &ast::VariantBody) -> Vec<DataKey<'_>> {
    match body {
        ast::VariantBody::Unit => Vec::new(),
        ast::VariantBody::Tuple(tys) => (0..tys.len()).map(DataKey::Index).collect(),
        ast::VariantBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

impl Emitter {
    /// Emit the methods for each `#[derive(...)]` on a struct, inside its class.
    pub(crate) fn emit_struct_derives(&mut self, s: &ast::StructDecl, attrs: &[ast::Attribute]) {
        for name in crate::derive_names(attrs) {
            match name.as_str() {
                "Clone" => self.derive_clone_struct(s),
                "Eq" => self.derive_eq_struct(s),
                "Debug" => self.derive_render_struct(s, "debug", true),
                "Display" => self.derive_render_struct(s, "display", false),
                other => self.error(unknown_derive(other)),
            }
        }
    }

    /// Emit the methods for each `#[derive(...)]` on an enum, inside its class.
    pub(crate) fn emit_enum_derives(&mut self, e: &ast::EnumDecl, attrs: &[ast::Attribute]) {
        for name in crate::derive_names(attrs) {
            match name.as_str() {
                "Clone" => self.derive_clone_enum(e),
                "Eq" => self.derive_eq_enum(e),
                "Debug" => self.derive_render_enum(e, "debug", true),
                "Display" => self.derive_render_enum(e, "display", false),
                other => self.error(unknown_derive(other)),
            }
        }
    }

    fn derive_clone_struct(&mut self, s: &ast::StructDecl) {
        self.line("def clone(self):");
        let args = struct_keys(&s.body)
            .into_iter()
            .map(|k| k.access("self"))
            .collect::<Vec<_>>()
            .join(", ");
        self.indented(|e| e.line(format!("return {}({args})", s.name)));
    }

    fn derive_clone_enum(&mut self, e: &ast::EnumDecl) {
        self.line("def clone(self):");
        self.indented(|em| {
            for v in &e.variants {
                em.line(format!("if self.tag == \"{}\":", v.name));
                let ctor = clone_ctor(&e.name, v);
                em.indented(|em| em.line(format!("return {ctor}")));
            }
        });
    }

    fn derive_eq_struct(&mut self, s: &ast::StructDecl) {
        self.line("def eq(self, other):");
        let mut conds = vec!["self._struct == other._struct".to_string()];
        for k in struct_keys(&s.body) {
            conds.push(format!("{} == {}", k.access("self"), k.access("other")));
        }
        self.indented(|e| e.line(format!("return {}", conds.join(" and "))));
    }

    fn derive_eq_enum(&mut self, e: &ast::EnumDecl) {
        self.line("def eq(self, other):");
        self.indented(|em| {
            em.line("if self.tag != other.tag:");
            em.indented(|em| em.line("return False"));
            for v in &e.variants {
                let keys = variant_keys(&v.body);
                if keys.is_empty() {
                    continue;
                }
                let conds: Vec<String> = keys
                    .iter()
                    .map(|k| format!("{} == {}", k.access("self"), k.access("other")))
                    .collect();
                em.line(format!("if self.tag == \"{}\":", v.name));
                em.indented(|em| em.line(format!("return {}", conds.join(" and "))));
            }
            em.line("return True");
        });
    }

    /// Emit a `debug`/`display` method on a struct, rendering through `_fmt`.
    fn derive_render_struct(&mut self, s: &ast::StructDecl, method: &str, debug: bool) {
        self.line(format!("def {method}(self):"));
        let body = render_expr(&s.name, &struct_keys(&s.body), "self", debug);
        self.indented(|e| e.line(format!("return {body}")));
    }

    /// Emit a `debug`/`display` method on an enum, one `if` per variant.
    fn derive_render_enum(&mut self, e: &ast::EnumDecl, method: &str, debug: bool) {
        self.line(format!("def {method}(self):"));
        self.indented(|em| {
            for v in &e.variants {
                em.line(format!("if self.tag == \"{}\":", v.name));
                let body = render_expr(&v.name, &variant_keys(&v.body), "self", debug);
                em.indented(|em| em.line(format!("return {body}")));
            }
        });
    }
}

/// The constructor expression cloning variant `v` of enum `en` from `self`. A
/// derived `Clone` makes `en` a method-carrying enum, so the clone instantiates
/// the enum's subclass to keep `value.method()` dispatch.
fn clone_ctor(en: &str, v: &ast::Variant) -> String {
    let keys = variant_keys(&v.body);
    let mut parts = vec![py_str(&v.name)];
    match &v.body {
        ast::VariantBody::Named(_) => {
            for k in &keys {
                if let DataKey::Named(name) = k {
                    parts.push(format!("{name}={}", k.access("self")));
                }
            }
        }
        _ => {
            for k in &keys {
                parts.push(k.access("self"));
            }
        }
    }
    format!("{en}({})", parts.join(", "))
}

/// Build the Python string expression for a `Debug`/`Display` render of `name`
/// with the given data keys read from `recv`.
///
/// `Display` (when `debug` is false) joins the values with `", "` and adds no
/// type-name decoration. `Debug` decorates: a value with no data is the bare
/// label; named keys yield `Name { f = <f>, … }`; positional keys yield
/// `Name(<0>, <1>)`. Each value is rendered with `_fmt`.
fn render_expr(name: &str, keys: &[DataKey], recv: &str, debug: bool) -> String {
    if keys.is_empty() {
        return py_str(name);
    }
    if !debug {
        let mut parts: Vec<String> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(py_str(", "));
            }
            parts.push(fmt_call(*k, recv));
        }
        return join_concat(&parts);
    }
    let positional = matches!(keys.first(), Some(DataKey::Index(_)));
    let mut parts: Vec<String> = Vec::new();
    if positional {
        parts.push(py_str(&format!("{name}(")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(py_str(", "));
            }
            parts.push(fmt_call(*k, recv));
        }
        parts.push(py_str(")"));
    } else {
        parts.push(py_str(&format!("{name} {{ ")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(py_str(", "));
            }
            parts.push(py_str(&format!("{} = ", k.label())));
            parts.push(fmt_call(*k, recv));
        }
        parts.push(py_str(" }"));
    }
    join_concat(&parts)
}

/// A `_fmt(<access>)` call for the key on `recv`.
fn fmt_call(k: DataKey, recv: &str) -> String {
    format!("_fmt({})", k.access(recv))
}

/// Join string-valued parts with Python `+`.
fn join_concat(parts: &[String]) -> String {
    parts.join(" + ")
}

/// A Python double-quoted string literal of `s`. Backslashes and quotes are
/// escaped; inputs here are identifiers and fixed punctuation.
fn py_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The diagnostic for an unknown derive name.
fn unknown_derive(name: &str) -> String {
    format!("unknown derive `{name}`; the derivable traits are Clone, Eq, Debug, Display")
}
