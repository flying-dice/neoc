//! Built-in derive macros for neoc: `Clone`, `Eq`, `Debug`, `Display`.
//!
//! Each derive is **structural** — it reads a `struct` or `enum` declaration's
//! fields and variants straight from the AST and emits Lua 5.1 source that
//! attaches a method to the type's table. No type information is required.
//!
//! The emitted Lua follows the runtime conventions in `lang/DYNAMIC.md` §6–§7:
//! a struct value is `{ __struct = "Name", <fields> }` (newtype field at `[1]`);
//! an enum value is `{ _enum = "Name", tag = "Variant", <payload> }` (tuple
//! payload at `[1..]`, struct payload by field name); methods are plain
//! functions on the type's table taking the receiver first.
//!
//! # Limitations
//!
//! `Eq` compares each field with Lua's `==`. For nested derived types this is
//! reference identity, not structural equality — two distinct tables holding
//! equal fields compare unequal. Structural recursion into nested derived types
//! is deferred to a later version. `Display` assumes every field already
//! satisfies `Display`; that obligation is checked by the type checker, not
//! here.

use neoc_ast as ast;

/// The declaration a derive is applied to.
///
/// A derive reads the structure of its target — a `struct`'s fields or an
/// `enum`'s variants — and emits the matching Lua. Both arms borrow the AST
/// node; the caller retains ownership.
#[derive(Debug, Clone, Copy)]
pub enum DeriveTarget<'a> {
    /// A `struct` declaration.
    Struct(&'a ast::StructDecl),
    /// An `enum` declaration.
    Enum(&'a ast::EnumDecl),
}

impl<'a> DeriveTarget<'a> {
    /// The type name the derived method attaches to.
    fn type_name(self) -> &'a str {
        match self {
            DeriveTarget::Struct(s) => &s.name,
            DeriveTarget::Enum(e) => &e.name,
        }
    }
}

/// Emit the Lua 5.1 lines implementing `derive_name` for `target`.
///
/// The returned lines are the body of a function definition (and the `end`
/// that closes it) with no leading indentation; the emitter indents them and
/// splices them in after the type's constructor.
///
/// # Arguments
///
/// * `derive_name` — the trait being derived. One of `Clone`, `Eq`, `Debug`,
///   or `Display`.
/// * `target` — the `struct` or `enum` the derive is applied to.
///
/// # Errors
///
/// Returns an error message when `derive_name` is not one of the four built-in
/// derives. The message names the unknown derive and lists the supported ones.
pub fn derive(derive_name: &str, target: DeriveTarget) -> Result<Vec<String>, String> {
    match derive_name {
        "Clone" => Ok(derive_clone(target)),
        "Eq" => Ok(derive_eq(target)),
        "Debug" => Ok(derive_debug(target)),
        "Display" => Ok(derive_display(target)),
        other => Err(format!(
            "unknown derive `{other}`; the derivable traits are Clone, Eq, Debug, Display"
        )),
    }
}

/// The Lua keys carrying a struct's data: every named field, or `1` for a
/// newtype. A unit struct has none.
fn struct_keys(body: &ast::StructBody) -> Vec<DataKey<'_>> {
    match body {
        ast::StructBody::Unit => Vec::new(),
        ast::StructBody::Tuple(_) => vec![DataKey::Index(1)],
        ast::StructBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

/// The Lua keys carrying a variant's payload.
fn variant_keys(body: &ast::VariantBody) -> Vec<DataKey<'_>> {
    match body {
        ast::VariantBody::Unit => Vec::new(),
        ast::VariantBody::Tuple(tys) => (1..=tys.len()).map(DataKey::Index).collect(),
        ast::VariantBody::Named(fields) => fields.iter().map(|f| DataKey::Named(&f.name)).collect(),
    }
}

/// A single data slot in a Lua value: either a named field or a positional
/// index (newtype / tuple payload).
#[derive(Clone, Copy)]
enum DataKey<'a> {
    Named(&'a str),
    Index(usize),
}

impl DataKey<'_> {
    /// Lua field-access suffix for the key on receiver `recv`, e.g. `recv.x`
    /// or `recv[1]`.
    fn access(self, recv: &str) -> String {
        match self {
            DataKey::Named(name) => format!("{recv}.{name}"),
            DataKey::Index(i) => format!("{recv}[{i}]"),
        }
    }

    /// Lua table-constructor entry assigning the key from receiver `recv`,
    /// e.g. `x = self.x` or `self[1]`.
    fn copy_entry(self, recv: &str) -> String {
        match self {
            DataKey::Named(name) => format!("{name} = {recv}.{name}"),
            DataKey::Index(i) => format!("{recv}[{i}]"),
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

// ---- Clone -----------------------------------------------------------------

fn derive_clone(target: DeriveTarget) -> Vec<String> {
    let ty = target.type_name();
    let mut out = vec![format!("function {ty}.clone(self)")];
    match target {
        DeriveTarget::Struct(s) => out.extend(clone_struct_body(&s.name, &s.body)),
        DeriveTarget::Enum(e) => out.extend(clone_enum_body(&e.name, &e.variants)),
    }
    out.push("end".to_string());
    out
}

fn clone_struct_body(name: &str, body: &ast::StructBody) -> Vec<String> {
    let mut entries = vec![format!("__struct = \"{name}\"")];
    entries.extend(struct_keys(body).into_iter().map(|k| k.copy_entry("self")));
    // Re-attach the type's metatable so the clone is a full value methods can
    // be called on, not a bare table (matches the `.new` constructor).
    vec![format!(
        "  return setmetatable({{ {} }}, {name})",
        entries.join(", ")
    )]
}

fn clone_enum_body(name: &str, variants: &[ast::Variant]) -> Vec<String> {
    let mut out = Vec::new();
    for v in variants {
        let mut entries = vec![
            format!("_enum = \"{name}\""),
            format!("tag = \"{}\"", v.name),
        ];
        entries.extend(
            variant_keys(&v.body)
                .into_iter()
                .map(|k| k.copy_entry("self")),
        );
        out.push(format!("  if self.tag == \"{}\" then", v.name));
        out.push(format!("    return {{ {} }}", entries.join(", ")));
        out.push("  end".to_string());
    }
    out
}

// ---- Eq --------------------------------------------------------------------

fn derive_eq(target: DeriveTarget) -> Vec<String> {
    let ty = target.type_name();
    let mut out = vec![format!("function {ty}.eq(a, b)")];
    match target {
        DeriveTarget::Struct(s) => out.extend(eq_struct_body(&s.body)),
        DeriveTarget::Enum(e) => out.extend(eq_enum_body(&e.variants)),
    }
    out.push("end".to_string());
    out
}

fn eq_struct_body(body: &ast::StructBody) -> Vec<String> {
    let mut conds = vec!["a.__struct == b.__struct".to_string()];
    for k in struct_keys(body) {
        conds.push(format!("{} == {}", k.access("a"), k.access("b")));
    }
    vec![format!("  return {}", conds.join(" and "))]
}

fn eq_enum_body(variants: &[ast::Variant]) -> Vec<String> {
    let mut out = vec!["  if a._enum ~= b._enum or a.tag ~= b.tag then".to_string()];
    out.push("    return false".to_string());
    out.push("  end".to_string());
    for v in variants {
        let keys = variant_keys(&v.body);
        if keys.is_empty() {
            continue;
        }
        let conds: Vec<String> = keys
            .iter()
            .map(|k| format!("{} == {}", k.access("a"), k.access("b")))
            .collect();
        out.push(format!("  if a.tag == \"{}\" then", v.name));
        out.push(format!("    return {}", conds.join(" and ")));
        out.push("  end".to_string());
    }
    out.push("  return true".to_string());
    out
}

// ---- Debug -----------------------------------------------------------------

fn derive_debug(target: DeriveTarget) -> Vec<String> {
    let ty = target.type_name();
    let mut out = vec![format!("function {ty}.debug(self)")];
    match target {
        DeriveTarget::Struct(s) => out.extend(debug_struct_body(&s.name, &s.body)),
        DeriveTarget::Enum(e) => out.extend(debug_enum_body(&e.variants)),
    }
    out.push("end".to_string());
    out
}

fn debug_struct_body(name: &str, body: &ast::StructBody) -> Vec<String> {
    vec![format!(
        "  return {}",
        debug_render(name, &struct_keys(body), "self")
    )]
}

fn debug_enum_body(variants: &[ast::Variant]) -> Vec<String> {
    let mut out = Vec::new();
    for v in variants {
        out.push(format!("  if self.tag == \"{}\" then", v.name));
        out.push(format!(
            "    return {}",
            debug_render(&v.name, &variant_keys(&v.body), "self")
        ));
        out.push("  end".to_string());
    }
    out
}

/// Build the Lua concatenation expression for a Debug string of `name` with
/// the given data keys read from `recv`.
///
/// Empty keys yield the bare label (`"Red"`). Named keys yield
/// `Name { f = <f>, … }`; positional keys yield `Name(<0>, <1>)`. Each value is
/// rendered with `tostring`.
fn debug_render(name: &str, keys: &[DataKey], recv: &str) -> String {
    if keys.is_empty() {
        return lua_str(name);
    }
    let positional = matches!(keys.first(), Some(DataKey::Index(_)));
    let mut parts: Vec<String> = Vec::new();
    if positional {
        parts.push(lua_str(&format!("{name}(")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(lua_str(", "));
            }
            parts.push(format!("tostring({})", k.access(recv)));
        }
        parts.push(lua_str(")"));
    } else {
        parts.push(lua_str(&format!("{name} {{ ")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(lua_str(", "));
            }
            parts.push(lua_str(&format!("{} = ", k.label())));
            parts.push(format!("tostring({})", k.access(recv)));
        }
        parts.push(lua_str(" }"));
    }
    parts.join(" .. ")
}

// ---- Display ---------------------------------------------------------------

fn derive_display(target: DeriveTarget) -> Vec<String> {
    let ty = target.type_name();
    let mut out = vec![format!("function {ty}.display(self)")];
    match target {
        DeriveTarget::Struct(s) => out.extend(display_struct_body(&s.name, &s.body)),
        DeriveTarget::Enum(e) => out.extend(display_enum_body(&e.variants)),
    }
    out.push("end".to_string());
    out
}

fn display_struct_body(name: &str, body: &ast::StructBody) -> Vec<String> {
    vec![format!(
        "  return {}",
        display_render(name, &struct_keys(body), "self")
    )]
}

fn display_enum_body(variants: &[ast::Variant]) -> Vec<String> {
    let mut out = Vec::new();
    for v in variants {
        out.push(format!("  if self.tag == \"{}\" then", v.name));
        out.push(format!(
            "    return {}",
            display_render(&v.name, &variant_keys(&v.body), "self")
        ));
        out.push("  end".to_string());
    }
    out
}

/// Build the Lua concatenation expression for a user-facing Display string.
///
/// A value with no data renders as its name. Otherwise the values are joined
/// with `", "`, each rendered with `tostring`, with no type-name decoration —
/// `Display` is the user-facing form.
fn display_render(name: &str, keys: &[DataKey], recv: &str) -> String {
    if keys.is_empty() {
        return lua_str(name);
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            parts.push(lua_str(", "));
        }
        parts.push(format!("tostring({})", k.access(recv)));
    }
    parts.join(" .. ")
}

/// A Lua double-quoted string literal of `s`. Backslashes and quotes are
/// escaped; the inputs here are identifiers and fixed punctuation, so no other
/// escaping is needed.
fn lua_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoc_ast as ast;

    fn field(name: &str) -> ast::Field {
        ast::Field {
            attrs: Vec::new(),
            name: name.to_string(),
            ty: ast::Type::Unit,
        }
    }

    fn variant(name: &str, body: ast::VariantBody) -> ast::Variant {
        ast::Variant {
            attrs: Vec::new(),
            name: name.to_string(),
            body,
        }
    }

    /// `struct Point { x: number, y: number }`
    fn point() -> ast::StructDecl {
        ast::StructDecl {
            name: "Point".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("x"), field("y")]),
        }
    }

    /// `struct Meters(number)` — a newtype.
    fn meters() -> ast::StructDecl {
        ast::StructDecl {
            name: "Meters".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Tuple(ast::Type::Unit),
        }
    }

    /// `enum Shape { Dot, Pair(number, number), Named { label: string } }`
    fn shape() -> ast::EnumDecl {
        ast::EnumDecl {
            name: "Shape".to_string(),
            generics: Vec::new(),
            variants: vec![
                variant("Dot", ast::VariantBody::Unit),
                variant(
                    "Pair",
                    ast::VariantBody::Tuple(vec![ast::Type::Unit, ast::Type::Unit]),
                ),
                variant("Named", ast::VariantBody::Named(vec![field("label")])),
            ],
        }
    }

    #[test]
    fn unknown_derive_errors() {
        let p = point();
        let err = derive("Ord", DeriveTarget::Struct(&p)).unwrap_err();
        assert_eq!(
            err,
            "unknown derive `Ord`; the derivable traits are Clone, Eq, Debug, Display"
        );
    }

    #[test]
    fn clone_named_struct() {
        let p = point();
        let lua = derive("Clone", DeriveTarget::Struct(&p)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Point.clone(self)",
                "  return setmetatable({ __struct = \"Point\", x = self.x, y = self.y }, Point)",
                "end",
            ]
        );
    }

    #[test]
    fn clone_newtype_struct() {
        let m = meters();
        let lua = derive("Clone", DeriveTarget::Struct(&m)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Meters.clone(self)",
                "  return setmetatable({ __struct = \"Meters\", self[1] }, Meters)",
                "end",
            ]
        );
    }

    #[test]
    fn clone_enum() {
        let s = shape();
        let lua = derive("Clone", DeriveTarget::Enum(&s)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Shape.clone(self)",
                "  if self.tag == \"Dot\" then",
                "    return { _enum = \"Shape\", tag = \"Dot\" }",
                "  end",
                "  if self.tag == \"Pair\" then",
                "    return { _enum = \"Shape\", tag = \"Pair\", self[1], self[2] }",
                "  end",
                "  if self.tag == \"Named\" then",
                "    return { _enum = \"Shape\", tag = \"Named\", label = self.label }",
                "  end",
                "end",
            ]
        );
    }

    #[test]
    fn eq_named_struct() {
        let p = point();
        let lua = derive("Eq", DeriveTarget::Struct(&p)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Point.eq(a, b)",
                "  return a.__struct == b.__struct and a.x == b.x and a.y == b.y",
                "end",
            ]
        );
    }

    #[test]
    fn eq_newtype_struct() {
        let m = meters();
        let lua = derive("Eq", DeriveTarget::Struct(&m)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Meters.eq(a, b)",
                "  return a.__struct == b.__struct and a[1] == b[1]",
                "end",
            ]
        );
    }

    #[test]
    fn eq_enum() {
        let s = shape();
        let lua = derive("Eq", DeriveTarget::Enum(&s)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Shape.eq(a, b)",
                "  if a._enum ~= b._enum or a.tag ~= b.tag then",
                "    return false",
                "  end",
                "  if a.tag == \"Pair\" then",
                "    return a[1] == b[1] and a[2] == b[2]",
                "  end",
                "  if a.tag == \"Named\" then",
                "    return a.label == b.label",
                "  end",
                "  return true",
                "end",
            ]
        );
    }

    #[test]
    fn debug_named_struct() {
        let p = point();
        let lua = derive("Debug", DeriveTarget::Struct(&p)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Point.debug(self)",
                "  return \"Point { \" .. \"x = \" .. tostring(self.x) .. \", \" .. \"y = \" .. tostring(self.y) .. \" }\"",
                "end",
            ]
        );
    }

    #[test]
    fn debug_newtype_struct() {
        let m = meters();
        let lua = derive("Debug", DeriveTarget::Struct(&m)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Meters.debug(self)",
                "  return \"Meters(\" .. tostring(self[1]) .. \")\"",
                "end",
            ]
        );
    }

    #[test]
    fn debug_enum() {
        let s = shape();
        let lua = derive("Debug", DeriveTarget::Enum(&s)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Shape.debug(self)",
                "  if self.tag == \"Dot\" then",
                "    return \"Dot\"",
                "  end",
                "  if self.tag == \"Pair\" then",
                "    return \"Pair(\" .. tostring(self[1]) .. \", \" .. tostring(self[2]) .. \")\"",
                "  end",
                "  if self.tag == \"Named\" then",
                "    return \"Named { \" .. \"label = \" .. tostring(self.label) .. \" }\"",
                "  end",
                "end",
            ]
        );
    }

    #[test]
    fn display_named_struct() {
        let p = point();
        let lua = derive("Display", DeriveTarget::Struct(&p)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Point.display(self)",
                "  return tostring(self.x) .. \", \" .. tostring(self.y)",
                "end",
            ]
        );
    }

    #[test]
    fn display_enum() {
        let s = shape();
        let lua = derive("Display", DeriveTarget::Enum(&s)).unwrap();
        assert_eq!(
            lua,
            vec![
                "function Shape.display(self)",
                "  if self.tag == \"Dot\" then",
                "    return \"Dot\"",
                "  end",
                "  if self.tag == \"Pair\" then",
                "    return tostring(self[1]) .. \", \" .. tostring(self[2])",
                "  end",
                "  if self.tag == \"Named\" then",
                "    return tostring(self.label)",
                "  end",
                "end",
            ]
        );
    }
}
