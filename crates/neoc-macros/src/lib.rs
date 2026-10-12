//! Built-in derive macros for neoc: `Clone`, `Eq`, `Debug`, `Display`.
//!
//! Each derive is **structural** — it reads a `struct` or `enum` declaration's
//! fields and variants straight from the AST and emits Lua 5.1 source that
//! attaches a method to the type's table. No type information is required.
//!
//! The emitted Lua follows the runtime conventions in `lang/DYNAMIC.md` §6–§7:
//! a struct value is `{ __struct = "Name", <fields> }` (newtype field at `[1]`);
//! an enum value is `{ __enum = "Name", __tag = "Variant", <payload> }` (tuple
//! payload at `[1..]`, struct payload by field name); methods are plain
//! functions on the type's table taking the receiver first.
//!
//! # Aggregate fields
//!
//! A `number`/`string`/`bool` field is compared with `==`; a `number`/`bool`
//! field is rendered with `tostring`. Any other field — a nested
//! `struct`/`enum`, `Option<T>`, `[T]`, and for Debug a `string`, which renders
//! quoted — goes through the module-local `__eq` / `__debug` helpers
//! ([`Helper`]), which dispatch on the value's shape and recurse. The caller splices a helper into
//! the module once, only when a derive reports using it. `Display` assumes
//! every field already satisfies `Display`; that obligation is checked by the
//! type checker, not here.

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

/// A module-local Lua helper a derive may call. Emitted once per module, ahead
/// of the types, and only when some derive reports using it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Helper {
    /// `__eq(a, b)` — structural equality over any value (`DYNAMIC.md` §6.2).
    Eq,
    /// `__debug(v)` — `{:?}`-style rendering of any value (`DYNAMIC.md` §6.3).
    Debug,
}

impl Helper {
    /// The Lua source of the helper, no leading indentation.
    #[must_use]
    pub fn source(self) -> &'static str {
        match self {
            Helper::Eq => EQ_HELPER,
            Helper::Debug => DEBUG_HELPER,
        }
    }
}

/// Structural equality. Scalars by `==`; a value whose metatable carries `eq`
/// (a derived or hand-written `Eq` type) by that method; any other table
/// (`Option`/`Result`, a list) key by key, which covers tag-then-payload.
const EQ_HELPER: &str = r#"local function __eq(a, b)
  if a == b then return true end
  if type(a) ~= "table" or type(b) ~= "table" then return false end
  local mt = getmetatable(a)
  if mt and mt.eq then return mt.eq(a, b) end
  for k, v in pairs(a) do
    if not __eq(v, b[k]) then return false end
  end
  for k in pairs(b) do
    if a[k] == nil then return false end
  end
  return true
end"#;

/// `{:?}` rendering. A string double-quoted with `\`, `"`, newline, tab and carriage return escaped;
/// other scalars by `tostring`; a value whose metatable carries `debug` by that
/// method; a tagged payload (`Option`/`Result`) as `Tag(…)`; a list as `[…]`.
/// A value whose type has no `debug` — reachable only through a type
/// parameter, which carries no obligation — renders by shape: a struct or
/// named-payload variant as `Name { .. }` (named keys carry no order), a
/// tuple-payload variant positionally, a unit variant bare.
const DEBUG_HELPER: &str = r#"local function __debug(v)
  if type(v) == "string" then
    local escaped = string.gsub(v, "[\\\"\n\t\r]", { ["\\"] = "\\\\", ["\""] = "\\\"", ["\n"] = "\\n", ["\t"] = "\\t", ["\r"] = "\\r" })
    return "\"" .. escaped .. "\""
  end
  if type(v) ~= "table" then return tostring(v) end
  local mt = getmetatable(v)
  if mt and mt.debug then return mt.debug(v) end
  if v.__struct then return v.__struct .. " { .. }" end
  local parts = {}
  for i = 1, #v do parts[i] = __debug(v[i]) end
  if v.__tag == nil then return "[" .. table.concat(parts, ", ") .. "]" end
  if #parts > 0 then return v.__tag .. "(" .. table.concat(parts, ", ") .. ")" end
  for k in pairs(v) do
    if k ~= "__enum" and k ~= "__tag" then return v.__tag .. " { .. }" end
  end
  return v.__tag
end"#;

/// The output of one derive: the method's Lua and the helpers it calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    /// The function definition, line by line.
    pub lines: Vec<String>,
    /// Helpers the lines call; empty when every field is a scalar.
    pub helpers: Vec<Helper>,
}

impl Derived {
    fn plain(lines: Vec<String>) -> Self {
        Self {
            lines,
            helpers: Vec::new(),
        }
    }

    fn with_helper(lines: Vec<String>, helper: Helper) -> Self {
        let uses = lines.iter().any(|l| l.contains(helper.call()));
        Self {
            lines,
            helpers: if uses { vec![helper] } else { Vec::new() },
        }
    }
}

impl Helper {
    /// The call prefix a derive emits to reach the helper.
    fn call(self) -> &'static str {
        match self {
            Helper::Eq => "__eq(",
            Helper::Debug => "__debug(",
        }
    }
}

/// Emit the Lua 5.1 lines implementing `derive_name` for `target`.
///
/// The returned lines are the body of a function definition (and the `end`
/// that closes it) with no leading indentation; the emitter indents them and
/// splices them in after the type's constructor. [`Derived::helpers`] names
/// the module-local helpers those lines call.
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
pub fn derive(derive_name: &str, target: DeriveTarget) -> Result<Derived, String> {
    derive_on(derive_name, target, target.type_name())
}

/// Like [`derive`], but attaches the derived methods to the Lua local `table`
/// instead of the type's own name. The emitter passes a fresh local when a value
/// binding spelled like the type would otherwise capture the type's table
/// (`DYNAMIC.md` §3.7).
///
/// # Arguments
///
/// * `derive_name` — the trait being derived, as for [`derive`].
/// * `target` — the `struct` or `enum` the derive is applied to.
/// * `table` — the Lua identifier holding the type's runtime table. The
///   `__struct` / `__enum` tags still carry the neoc type name.
///
/// # Errors
///
/// Returns an error message when `derive_name` is not a built-in derive.
pub fn derive_on(derive_name: &str, target: DeriveTarget, table: &str) -> Result<Derived, String> {
    match derive_name {
        "Clone" => Ok(Derived::plain(derive_clone(target, table))),
        "Eq" => Ok(Derived::with_helper(derive_eq(target, table), Helper::Eq)),
        "Debug" => Ok(Derived::with_helper(
            derive_debug(target, table),
            Helper::Debug,
        )),
        "Display" => Ok(Derived::plain(derive_display(target, table))),
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
        ast::StructBody::Tuple(ty) => vec![DataKey::Index(1, ty)],
        ast::StructBody::Named(fields) => fields
            .iter()
            .map(|f| DataKey::Named(&f.name, &f.ty))
            .collect(),
    }
}

/// The Lua keys carrying a variant's payload.
fn variant_keys(body: &ast::VariantBody) -> Vec<DataKey<'_>> {
    match body {
        ast::VariantBody::Unit => Vec::new(),
        ast::VariantBody::Tuple(tys) => tys
            .iter()
            .enumerate()
            .map(|(i, ty)| DataKey::Index(i + 1, ty))
            .collect(),
        ast::VariantBody::Named(fields) => fields
            .iter()
            .map(|f| DataKey::Named(&f.name, &f.ty))
            .collect(),
    }
}

/// A single data slot in a Lua value — a named field or a positional index
/// (newtype / tuple payload) — with its declared type.
#[derive(Clone, Copy)]
enum DataKey<'a> {
    Named(&'a str, &'a ast::Type),
    Index(usize, &'a ast::Type),
}

impl DataKey<'_> {
    /// Whether the slot holds a `number`/`string`/`bool` (or unit), which
    /// Lua's `==` and `tostring` already handle; anything else is an aggregate
    /// needing the structural helpers.
    fn is_scalar(self) -> bool {
        self.is_one_of(&["number", "string", "bool"])
    }

    /// Whether the slot's Debug form is plain `tostring` — every scalar but
    /// `string`, which the helper quotes.
    fn debugs_with_tostring(self) -> bool {
        self.is_one_of(&["number", "bool"])
    }

    /// Whether the slot's type is unit or a bare primitive named in `names`.
    fn is_one_of(self, names: &[&str]) -> bool {
        let (DataKey::Named(_, ty) | DataKey::Index(_, ty)) = self;
        match ty {
            ast::Type::Unit => true,
            ast::Type::Path(p) => matches!(
                p.segments.as_slice(),
                [seg] if seg.args.is_empty() && names.contains(&seg.name.as_str())
            ),
            _ => false,
        }
    }

    /// Lua expression asserting slot equality between receivers `a` and `b`.
    fn eq_expr(self, a: &str, b: &str) -> String {
        if self.is_scalar() {
            format!("{} == {}", self.access(a), self.access(b))
        } else {
            format!("__eq({}, {})", self.access(a), self.access(b))
        }
    }

    /// Lua expression rendering the slot read from `recv` for Debug output.
    fn debug_expr(self, recv: &str) -> String {
        if self.debugs_with_tostring() {
            format!("tostring({})", self.access(recv))
        } else {
            format!("__debug({})", self.access(recv))
        }
    }

    /// Lua field-access suffix for the key on receiver `recv`, e.g. `recv.x`
    /// or `recv[1]`.
    fn access(self, recv: &str) -> String {
        match self {
            DataKey::Named(name, _) => format!("{recv}.{name}"),
            DataKey::Index(i, _) => format!("{recv}[{i}]"),
        }
    }

    /// Lua table-constructor entry assigning the key from receiver `recv`,
    /// e.g. `x = self.x` or `self[1]`.
    fn copy_entry(self, recv: &str) -> String {
        match self {
            DataKey::Named(name, _) => format!("{name} = {recv}.{name}"),
            DataKey::Index(i, _) => format!("{recv}[{i}]"),
        }
    }

    /// Human-readable label for Debug output: the field name, or `_<i>` for a
    /// positional slot.
    fn label(self) -> String {
        match self {
            DataKey::Named(name, _) => name.to_string(),
            DataKey::Index(i, _) => format!("_{i}"),
        }
    }
}

// ---- Clone -----------------------------------------------------------------

fn derive_clone(target: DeriveTarget, ty: &str) -> Vec<String> {
    let mut out = vec![format!("function {ty}.clone(self)")];
    match target {
        DeriveTarget::Struct(s) => out.extend(clone_struct_body(&s.name, ty, &s.body)),
        DeriveTarget::Enum(e) => out.extend(clone_enum_body(&e.name, ty, &e.variants)),
    }
    out.push("end".to_string());
    out
}

fn clone_struct_body(name: &str, table: &str, body: &ast::StructBody) -> Vec<String> {
    let mut entries = vec![format!("__struct = \"{name}\"")];
    entries.extend(struct_keys(body).into_iter().map(|k| k.copy_entry("self")));
    // Re-attach the type's metatable so the clone is a full value methods can
    // be called on, not a bare table (matches the `.new` constructor).
    vec![format!(
        "  return setmetatable({{ {} }}, {table})",
        entries.join(", ")
    )]
}

fn clone_enum_body(name: &str, table: &str, variants: &[ast::Variant]) -> Vec<String> {
    let mut out = Vec::new();
    for v in variants {
        let mut entries = vec![
            format!("__enum = \"{name}\""),
            format!("__tag = \"{}\"", v.name),
        ];
        entries.extend(
            variant_keys(&v.body)
                .into_iter()
                .map(|k| k.copy_entry("self")),
        );
        out.push(format!("  if self.__tag == \"{}\" then", v.name));
        // Re-attach the enum's metatable (as the struct path does) so the clone
        // keeps its methods and `__traits`.
        out.push(format!(
            "    return setmetatable({{ {} }}, {table})",
            entries.join(", ")
        ));
        out.push("  end".to_string());
    }
    out
}

// ---- Eq --------------------------------------------------------------------

fn derive_eq(target: DeriveTarget, ty: &str) -> Vec<String> {
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
    conds.extend(struct_keys(body).into_iter().map(|k| k.eq_expr("a", "b")));
    vec![format!("  return {}", conds.join(" and "))]
}

fn eq_enum_body(variants: &[ast::Variant]) -> Vec<String> {
    let mut out = vec!["  if a.__enum ~= b.__enum or a.__tag ~= b.__tag then".to_string()];
    out.push("    return false".to_string());
    out.push("  end".to_string());
    for v in variants {
        let keys = variant_keys(&v.body);
        if keys.is_empty() {
            continue;
        }
        let conds: Vec<String> = keys.iter().map(|k| k.eq_expr("a", "b")).collect();
        out.push(format!("  if a.__tag == \"{}\" then", v.name));
        out.push(format!("    return {}", conds.join(" and ")));
        out.push("  end".to_string());
    }
    out.push("  return true".to_string());
    out
}

// ---- Debug -----------------------------------------------------------------

fn derive_debug(target: DeriveTarget, ty: &str) -> Vec<String> {
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
        out.push(format!("  if self.__tag == \"{}\" then", v.name));
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
/// `Name { f = <f>, … }`; positional keys yield `Name(<0>, <1>)`. A `number`/`bool`
/// value is rendered with `tostring`, anything else with `__debug`.
fn debug_render(name: &str, keys: &[DataKey], recv: &str) -> String {
    if keys.is_empty() {
        return lua_str(name);
    }
    let positional = matches!(keys.first(), Some(DataKey::Index(..)));
    let mut parts: Vec<String> = Vec::new();
    if positional {
        parts.push(lua_str(&format!("{name}(")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(lua_str(", "));
            }
            parts.push(k.debug_expr(recv));
        }
        parts.push(lua_str(")"));
    } else {
        parts.push(lua_str(&format!("{name} {{ ")));
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                parts.push(lua_str(", "));
            }
            parts.push(lua_str(&format!("{} = ", k.label())));
            parts.push(k.debug_expr(recv));
        }
        parts.push(lua_str(" }"));
    }
    parts.join(" .. ")
}

// ---- Display ---------------------------------------------------------------

fn derive_display(target: DeriveTarget, ty: &str) -> Vec<String> {
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
        out.push(format!("  if self.__tag == \"{}\" then", v.name));
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
            name_span: ast::Span::default(),
            ty: scalar("number"),
        }
    }

    fn scalar(name: &str) -> ast::Type {
        named(name, Vec::new())
    }

    fn named(name: &str, args: Vec<ast::Type>) -> ast::Type {
        ast::Type::Path(ast::TypePath {
            segments: vec![ast::TypeSegment {
                name: name.to_string(),
                args,
            }],
        })
    }

    fn typed_field(name: &str, ty: ast::Type) -> ast::Field {
        ast::Field { ty, ..field(name) }
    }

    fn variant(name: &str, body: ast::VariantBody) -> ast::Variant {
        ast::Variant {
            attrs: Vec::new(),
            name: name.to_string(),
            name_span: ast::Span::default(),
            body,
        }
    }

    // `DYNAMIC.md` §3.7: derived methods attach to, and clones re-stamp the
    // metatable of, the given table local — not the type's own name.
    #[test]
    fn derive_on_uses_the_given_table_for_struct_and_enum() {
        let p = point();
        let out = derive_on("Clone", DeriveTarget::Struct(&p), "__type_Point").unwrap();
        let text = out.lines.join("\n");
        assert!(text.contains("function __type_Point.clone(self)"), "{text}");
        assert!(text.contains("}, __type_Point)"), "{text}");
        assert!(!text.contains("}, Point)"), "{text}");
        assert!(text.contains("__struct = \"Point\""), "{text}");

        let sh = shape();
        let out = derive_on("Clone", DeriveTarget::Enum(&sh), "__type_Shape").unwrap();
        let text = out.lines.join("\n");
        assert!(text.contains("}, __type_Shape)"), "{text}");
        assert!(text.contains("__enum = \"Shape\""), "{text}");

        for d in ["Eq", "Debug", "Display"] {
            let out = derive_on(d, DeriveTarget::Struct(&p), "__type_Point").unwrap();
            assert!(
                out.lines[0].contains(" __type_Point."),
                "{:?}",
                out.lines[0]
            );
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
            body: ast::StructBody::Tuple(scalar("number")),
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
                    ast::VariantBody::Tuple(vec![scalar("number"), scalar("number")]),
                ),
                variant("Named", ast::VariantBody::Named(vec![field("label")])),
            ],
        }
    }

    /// `struct Outer { n: number, i: Inner, o: Option<number>, xs: [number] }`
    fn outer() -> ast::StructDecl {
        ast::StructDecl {
            name: "Outer".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![
                field("n"),
                typed_field("i", scalar("Inner")),
                typed_field("o", named("Option", vec![scalar("number")])),
                typed_field("xs", ast::Type::List(Box::new(scalar("number")))),
            ]),
        }
    }

    #[test]
    fn scalar_derives_use_no_helper() {
        let p = point();
        for name in ["Eq", "Debug"] {
            assert!(
                derive(name, DeriveTarget::Struct(&p))
                    .unwrap()
                    .helpers
                    .is_empty()
            );
        }
    }

    #[test]
    fn eq_aggregate_fields_recurse() {
        let o = outer();
        let d = derive("Eq", DeriveTarget::Struct(&o)).unwrap();
        assert_eq!(
            d.lines,
            vec![
                "function Outer.eq(a, b)",
                "  return a.__struct == b.__struct and a.n == b.n and __eq(a.i, b.i) and __eq(a.o, b.o) and __eq(a.xs, b.xs)",
                "end",
            ]
        );
        assert_eq!(d.helpers, vec![Helper::Eq]);
    }

    #[test]
    fn debug_aggregate_fields_recurse() {
        let o = outer();
        let d = derive("Debug", DeriveTarget::Struct(&o)).unwrap();
        assert_eq!(
            d.lines,
            vec![
                "function Outer.debug(self)",
                "  return \"Outer { \" .. \"n = \" .. tostring(self.n) .. \", \" .. \"i = \" .. __debug(self.i) .. \", \" .. \"o = \" .. __debug(self.o) .. \", \" .. \"xs = \" .. __debug(self.xs) .. \" }\"",
                "end",
            ]
        );
        assert_eq!(d.helpers, vec![Helper::Debug]);
    }

    #[test]
    fn debug_string_field_goes_through_helper_eq_does_not() {
        let s = ast::StructDecl {
            name: "Tag".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![typed_field("s", scalar("string"))]),
        };
        let debug = derive("Debug", DeriveTarget::Struct(&s)).unwrap();
        assert_eq!(
            debug.lines[1],
            "  return \"Tag { \" .. \"s = \" .. __debug(self.s) .. \" }\""
        );
        assert_eq!(debug.helpers, vec![Helper::Debug]);
        let eq = derive("Eq", DeriveTarget::Struct(&s)).unwrap();
        assert!(eq.helpers.is_empty());
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
        let lua = derive("Clone", DeriveTarget::Struct(&p)).unwrap().lines;
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
        let lua = derive("Clone", DeriveTarget::Struct(&m)).unwrap().lines;
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
        let lua = derive("Clone", DeriveTarget::Enum(&s)).unwrap().lines;
        assert_eq!(
            lua,
            vec![
                "function Shape.clone(self)",
                "  if self.__tag == \"Dot\" then",
                "    return setmetatable({ __enum = \"Shape\", __tag = \"Dot\" }, Shape)",
                "  end",
                "  if self.__tag == \"Pair\" then",
                "    return setmetatable({ __enum = \"Shape\", __tag = \"Pair\", self[1], self[2] }, Shape)",
                "  end",
                "  if self.__tag == \"Named\" then",
                "    return setmetatable({ __enum = \"Shape\", __tag = \"Named\", label = self.label }, Shape)",
                "  end",
                "end",
            ]
        );
    }

    #[test]
    fn eq_named_struct() {
        let p = point();
        let lua = derive("Eq", DeriveTarget::Struct(&p)).unwrap().lines;
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
        let lua = derive("Eq", DeriveTarget::Struct(&m)).unwrap().lines;
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
        let lua = derive("Eq", DeriveTarget::Enum(&s)).unwrap().lines;
        assert_eq!(
            lua,
            vec![
                "function Shape.eq(a, b)",
                "  if a.__enum ~= b.__enum or a.__tag ~= b.__tag then",
                "    return false",
                "  end",
                "  if a.__tag == \"Pair\" then",
                "    return a[1] == b[1] and a[2] == b[2]",
                "  end",
                "  if a.__tag == \"Named\" then",
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
        let lua = derive("Debug", DeriveTarget::Struct(&p)).unwrap().lines;
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
        let lua = derive("Debug", DeriveTarget::Struct(&m)).unwrap().lines;
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
        let lua = derive("Debug", DeriveTarget::Enum(&s)).unwrap().lines;
        assert_eq!(
            lua,
            vec![
                "function Shape.debug(self)",
                "  if self.__tag == \"Dot\" then",
                "    return \"Dot\"",
                "  end",
                "  if self.__tag == \"Pair\" then",
                "    return \"Pair(\" .. tostring(self[1]) .. \", \" .. tostring(self[2]) .. \")\"",
                "  end",
                "  if self.__tag == \"Named\" then",
                "    return \"Named { \" .. \"label = \" .. tostring(self.label) .. \" }\"",
                "  end",
                "end",
            ]
        );
    }

    #[test]
    fn display_named_struct() {
        let p = point();
        let lua = derive("Display", DeriveTarget::Struct(&p)).unwrap().lines;
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
        let lua = derive("Display", DeriveTarget::Enum(&s)).unwrap().lines;
        assert_eq!(
            lua,
            vec![
                "function Shape.display(self)",
                "  if self.__tag == \"Dot\" then",
                "    return \"Dot\"",
                "  end",
                "  if self.__tag == \"Pair\" then",
                "    return tostring(self[1]) .. \", \" .. tostring(self[2])",
                "  end",
                "  if self.__tag == \"Named\" then",
                "    return tostring(self.label)",
                "  end",
                "end",
            ]
        );
    }
}
