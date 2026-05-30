//! Built-in derives for the Python backend: `Clone`, `Eq`, `Debug`, `Display`.
//!
//! Each derive is **structural** — it reads a `struct` or `enum` declaration
//! straight from the AST and emits a method into the dataclass body. Python
//! dataclasses already provide structural equality (`eq=True` by default), so
//! `Eq` needs no emitted method — the `==` operator works natively (the
//! checker's `==`→`.eq` rewrite is lowered back to Python `==` in
//! [`crate::expr`]). `Clone` returns a shallow copy; `Debug`/`Display` return
//! the golden developer-/user-facing strings (`lang/DYNAMIC.md` §6).
//!
//! A derive on an enum is emitted per variant class — `self_class` names the
//! variant the method body reads — so every value matched out of the union
//! carries the behaviour.

use crate::{DeriveTarget, Emitter};
use neoc_ast as ast;

/// A single data slot: a named field or a positional index (`_<i>`).
#[derive(Clone)]
enum DataKey {
    Named(String),
    Index(usize),
}

impl DataKey {
    /// `self`-relative access for the slot: `self.x` or `self._0`.
    fn access(&self) -> String {
        match self {
            DataKey::Named(name) => format!("self.{name}"),
            DataKey::Index(i) => format!("self._{i}"),
        }
    }

    /// Human-readable label for Debug: the field name, or `_<i>`.
    fn label(&self) -> String {
        match self {
            DataKey::Named(name) => name.clone(),
            DataKey::Index(i) => format!("_{i}"),
        }
    }
}

/// The data keys a struct body carries.
fn struct_keys(body: &ast::StructBody) -> Vec<DataKey> {
    match body {
        ast::StructBody::Unit => Vec::new(),
        ast::StructBody::Tuple(_) => vec![DataKey::Index(0)],
        ast::StructBody::Named(fields) => fields
            .iter()
            .map(|f| DataKey::Named(f.name.clone()))
            .collect(),
    }
}

/// The data keys a variant body carries.
fn variant_keys(body: &ast::VariantBody) -> Vec<DataKey> {
    match body {
        ast::VariantBody::Unit => Vec::new(),
        ast::VariantBody::Tuple(tys) => (0..tys.len()).map(DataKey::Index).collect(),
        ast::VariantBody::Named(fields) => fields
            .iter()
            .map(|f| DataKey::Named(f.name.clone()))
            .collect(),
    }
}

impl Emitter {
    /// Emit the Python implementing `derive_name` for `target` into the current
    /// class body. `self_class` is the class whose instance `self` is — the
    /// struct name, or, for an enum, the variant class currently being emitted.
    pub(crate) fn emit_derive(
        &mut self,
        derive_name: &str,
        target: DeriveTarget<'_>,
        self_class: &str,
    ) {
        match derive_name {
            "Clone" => self.derive_clone(self_class),
            // Dataclasses are `eq` by default; the `==` operator works natively.
            "Eq" => {}
            "Debug" => self.derive_debug(target, self_class),
            "Display" => self.derive_display(target, self_class),
            other => self.error(format!(
                "unknown derive `{other}`; the derivable traits are Clone, Eq, Debug, Display"
            )),
        }
    }

    // ---- Clone -------------------------------------------------------------

    /// `clone` returns a shallow copy. For an immutable (`frozen`) dataclass a
    /// shallow copy is `dataclasses.replace(self)`.
    fn derive_clone(&mut self, self_class: &str) {
        let _ = self_class;
        self.uses_replace = true;
        self.line("def clone(self):");
        self.indented(|e| e.line("return replace(self)"));
    }

    // ---- Debug -------------------------------------------------------------

    fn derive_debug(&mut self, target: DeriveTarget<'_>, self_class: &str) {
        let keys = keys_for(target, self_class);
        let body = debug_render(self_class, &keys);
        self.line("def debug(self) -> str:");
        self.indented(|e| e.line(format!("return {body}")));
    }

    // ---- Display -----------------------------------------------------------

    fn derive_display(&mut self, target: DeriveTarget<'_>, self_class: &str) {
        let keys = keys_for(target, self_class);
        let body = display_render(self_class, &keys);
        self.line("def display(self) -> str:");
        self.indented(|e| e.line(format!("return {body}")));
    }
}

/// The data keys for the class currently being emitted: a struct's body, or the
/// matching variant's body (by class name) for an enum.
fn keys_for(target: DeriveTarget<'_>, self_class: &str) -> Vec<DataKey> {
    match target {
        DeriveTarget::Struct(s) => struct_keys(&s.body),
        DeriveTarget::Enum(e) => e
            .variants
            .iter()
            .find(|v| crate::variant_class_name(&v.name) == self_class)
            .map_or_else(Vec::new, |v| variant_keys(&v.body)),
    }
}

/// Build the Python f-string-free concatenation for a Debug string. Empty keys
/// yield the bare name; positional keys yield `Name(<0>, <1>)`; named keys yield
/// `Name { f = <f>, … }`. Each value is rendered with `str(...)`.
fn debug_render(name: &str, keys: &[DataKey]) -> String {
    if keys.is_empty() {
        return py_str(name);
    }
    let positional = matches!(keys.first(), Some(DataKey::Index(_)));
    let mut parts: Vec<String> = Vec::new();
    if positional {
        parts.push(py_str(&format!("{name}(")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(py_str(", "));
            }
            parts.push(format!("str({})", k.access()));
        }
        parts.push(py_str(")"));
    } else {
        parts.push(py_str(&format!("{name} {{ ")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(py_str(", "));
            }
            parts.push(py_str(&format!("{} = ", k.label())));
            parts.push(format!("str({})", k.access()));
        }
        parts.push(py_str(" }"));
    }
    parts.join(" + ")
}

/// Build the Python concatenation for a user-facing Display string. A value with
/// no data renders as its name; otherwise the values are joined with `", "`.
fn display_render(name: &str, keys: &[DataKey]) -> String {
    if keys.is_empty() {
        return py_str(name);
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            parts.push(py_str(", "));
        }
        parts.push(format!("str({})", k.access()));
    }
    parts.join(" + ")
}

/// A Python double-quoted string literal of `s`.
fn py_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
