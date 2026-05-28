//! Built-in derive macros for the JavaScript backend: `Clone`, `Eq`, `Debug`,
//! `Display`.
//!
//! Each derive is **structural** — it reads a `struct` or `enum` declaration's
//! fields and variants straight from the AST and attaches a method to the
//! type's prototype `Name$`. No type information is required.
//!
//! The emitted JavaScript follows the runtime conventions in `lang/DYNAMIC.md`
//! §6–§7, adapted to JS objects: a struct value is `{ __struct: "Name",
//! <fields>, __proto__: Name$ }` (newtype payload under `_0`); an enum value is
//! `{ __enum: "Name", __tag: "Variant", <payload>, __proto__: Name$ }` (tuple
//! payload under `_0`, `_1`, …, struct payload by field name). A derived method
//! is a `function` over the implicit `this` — except `Eq`, a two-argument
//! comparison taking both operands explicitly (`Name$.eq = function(a, b)`).

use crate::{DeriveTarget, Emitter};
use neoc_ast as ast;

/// A single data slot in a value: either a named field or a positional index
/// (newtype / tuple payload, stored under `_<i>`).
#[derive(Clone, Copy)]
enum DataKey<'a> {
    Named(&'a str),
    Index(usize),
}

impl DataKey<'_> {
    /// Property-access suffix for the key on receiver `recv`, e.g. `recv.x` or
    /// `recv._0`.
    fn access(self, recv: &str) -> String {
        match self {
            DataKey::Named(name) => format!("{recv}.{name}"),
            DataKey::Index(i) => format!("{recv}._{i}"),
        }
    }

    /// Object-literal entry copying the key from receiver `recv`, e.g.
    /// `x: this.x` or `_0: this._0`.
    fn copy_entry(self, recv: &str) -> String {
        match self {
            DataKey::Named(name) => format!("{name}: {recv}.{name}"),
            DataKey::Index(i) => format!("_{i}: {recv}._{i}"),
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

/// The keys carrying a struct's data: every named field, or `_0` for a
/// newtype. A unit struct has none.
fn struct_keys(body: &ast::StructBody) -> Vec<DataKey<'_>> {
    match body {
        ast::StructBody::Unit => Vec::new(),
        ast::StructBody::Tuple(_) => vec![DataKey::Index(0)],
        ast::StructBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

/// The keys carrying a variant's payload.
fn variant_keys(body: &ast::VariantBody) -> Vec<DataKey<'_>> {
    match body {
        ast::VariantBody::Unit => Vec::new(),
        ast::VariantBody::Tuple(tys) => (0..tys.len()).map(DataKey::Index).collect(),
        ast::VariantBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

impl Emitter {
    /// Emit the JavaScript implementing `derive_name` for `target`, attaching it
    /// to the type's prototype `Name$`. An unknown derive is a diagnostic.
    pub(crate) fn emit_derive(&mut self, derive_name: &str, target: DeriveTarget<'_>) {
        match derive_name {
            "Clone" => self.derive_clone(target),
            "Eq" => self.derive_eq(target),
            "Debug" => self.derive_debug(target),
            "Display" => self.derive_display(target),
            other => self.error(format!(
                "unknown derive `{other}`; the derivable traits are Clone, Eq, Debug, Display"
            )),
        }
    }

    fn type_name(target: DeriveTarget<'_>) -> &str {
        match target {
            DeriveTarget::Struct(s) => &s.name,
            DeriveTarget::Enum(e) => &e.name,
        }
    }

    // ---- Clone -------------------------------------------------------------

    fn derive_clone(&mut self, target: DeriveTarget<'_>) {
        let ty = Self::type_name(target);
        self.line(format!("{ty}$.clone = function() {{"));
        self.indented(|e| match target {
            DeriveTarget::Struct(s) => e.clone_struct_body(&s.name, &s.body),
            DeriveTarget::Enum(en) => e.clone_enum_body(&en.name, &en.variants),
        });
        self.line("};");
    }

    fn clone_struct_body(&mut self, name: &str, body: &ast::StructBody) {
        let mut entries = vec![format!("__struct: \"{name}\"")];
        entries.extend(struct_keys(body).into_iter().map(|k| k.copy_entry("this")));
        // Re-link the prototype so the clone is a full value methods resolve on
        // (`lang/DYNAMIC.md` §6.1).
        entries.push(format!("__proto__: {name}$"));
        self.line(format!("return {{ {} }};", entries.join(", ")));
    }

    fn clone_enum_body(&mut self, name: &str, variants: &[ast::Variant]) {
        for v in variants {
            let mut entries = vec![
                format!("__enum: \"{name}\""),
                format!("__tag: \"{}\"", v.name),
            ];
            entries.extend(
                variant_keys(&v.body)
                    .into_iter()
                    .map(|k| k.copy_entry("this")),
            );
            entries.push(format!("__proto__: {name}$"));
            self.line(format!("if (this.__tag === \"{}\") {{", v.name));
            self.indented(|e| e.line(format!("return {{ {} }};", entries.join(", "))));
            self.line("}");
        }
    }

    // ---- Eq ----------------------------------------------------------------

    fn derive_eq(&mut self, target: DeriveTarget<'_>) {
        let ty = Self::type_name(target);
        self.line(format!("{ty}$.eq = function(a, b) {{"));
        self.indented(|e| match target {
            DeriveTarget::Struct(s) => e.eq_struct_body(&s.body),
            DeriveTarget::Enum(en) => e.eq_enum_body(&en.variants),
        });
        self.line("};");
    }

    fn eq_struct_body(&mut self, body: &ast::StructBody) {
        let mut conds = vec!["a.__struct === b.__struct".to_string()];
        for k in struct_keys(body) {
            conds.push(format!("{} === {}", k.access("a"), k.access("b")));
        }
        self.line(format!("return {};", conds.join(" && ")));
    }

    fn eq_enum_body(&mut self, variants: &[ast::Variant]) {
        self.line("if (a.__enum !== b.__enum || a.__tag !== b.__tag) {");
        self.indented(|e| e.line("return false;"));
        self.line("}");
        for v in variants {
            let keys = variant_keys(&v.body);
            if keys.is_empty() {
                continue;
            }
            let conds: Vec<String> = keys
                .iter()
                .map(|k| format!("{} === {}", k.access("a"), k.access("b")))
                .collect();
            self.line(format!("if (a.__tag === \"{}\") {{", v.name));
            self.indented(|e| e.line(format!("return {};", conds.join(" && "))));
            self.line("}");
        }
        self.line("return true;");
    }

    // ---- Debug -------------------------------------------------------------

    fn derive_debug(&mut self, target: DeriveTarget<'_>) {
        let ty = Self::type_name(target);
        self.line(format!("{ty}$.debug = function() {{"));
        self.indented(|e| match target {
            DeriveTarget::Struct(s) => {
                let r = debug_render(&s.name, &struct_keys(&s.body), "this");
                e.line(format!("return {r};"));
            }
            DeriveTarget::Enum(en) => e.debug_enum_body(&en.variants),
        });
        self.line("};");
    }

    fn debug_enum_body(&mut self, variants: &[ast::Variant]) {
        for v in variants {
            self.line(format!("if (this.__tag === \"{}\") {{", v.name));
            let r = debug_render(&v.name, &variant_keys(&v.body), "this");
            self.indented(|e| e.line(format!("return {r};")));
            self.line("}");
        }
    }

    // ---- Display -----------------------------------------------------------

    fn derive_display(&mut self, target: DeriveTarget<'_>) {
        let ty = Self::type_name(target);
        self.line(format!("{ty}$.display = function() {{"));
        self.indented(|e| match target {
            DeriveTarget::Struct(s) => {
                let r = display_render(&s.name, &struct_keys(&s.body), "this");
                e.line(format!("return {r};"));
            }
            DeriveTarget::Enum(en) => e.display_enum_body(&en.variants),
        });
        self.line("};");
    }

    fn display_enum_body(&mut self, variants: &[ast::Variant]) {
        for v in variants {
            self.line(format!("if (this.__tag === \"{}\") {{", v.name));
            let r = display_render(&v.name, &variant_keys(&v.body), "this");
            self.indented(|e| e.line(format!("return {r};")));
            self.line("}");
        }
    }
}

/// Build the JavaScript concatenation expression for a Debug string of `name`
/// with the given data keys read from `recv`.
///
/// Empty keys yield the bare label (`"Red"`). Named keys yield
/// `Name { f = <f>, … }`; positional keys yield `Name(<0>, <1>)`. Each value is
/// rendered with `String(...)`.
fn debug_render(name: &str, keys: &[DataKey], recv: &str) -> String {
    if keys.is_empty() {
        return js_str(name);
    }
    let positional = matches!(keys.first(), Some(DataKey::Index(_)));
    let mut parts: Vec<String> = Vec::new();
    if positional {
        parts.push(js_str(&format!("{name}(")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(js_str(", "));
            }
            parts.push(format!("String({})", k.access(recv)));
        }
        parts.push(js_str(")"));
    } else {
        parts.push(js_str(&format!("{name} {{ ")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(js_str(", "));
            }
            parts.push(js_str(&format!("{} = ", k.label())));
            parts.push(format!("String({})", k.access(recv)));
        }
        parts.push(js_str(" }"));
    }
    parts.join(" + ")
}

/// Build the JavaScript concatenation expression for a user-facing Display
/// string.
///
/// A value with no data renders as its name. Otherwise the values are joined
/// with `", "`, each rendered with `String(...)`, with no type-name decoration.
fn display_render(name: &str, keys: &[DataKey], recv: &str) -> String {
    if keys.is_empty() {
        return js_str(name);
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            parts.push(js_str(", "));
        }
        parts.push(format!("String({})", k.access(recv)));
    }
    parts.join(" + ")
}

/// A JavaScript double-quoted string literal of `s`. Backslashes and quotes are
/// escaped; the inputs here are identifiers and fixed punctuation, so no other
/// escaping is needed.
fn js_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
