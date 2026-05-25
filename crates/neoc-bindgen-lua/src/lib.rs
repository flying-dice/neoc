//! Reads a Luau source file and emits a declarations-only `.neoc` header:
//! the module's public surface as `ext` declarations with no bodies, wrapped
//! in a `#[link("<name>")] ext { … }` block, with opaque `ext type` handles
//! emitted standalone after the block.
//!
//! Parsing uses `tree-sitter` with the published `tree-sitter-luau` grammar.
//! A Luau module exports a table — `return { name = fn, … }`, optionally built
//! up as `local M = {} … function M.f() … return M`. We read that final
//! `return` table to determine the public surface, then resolve each exported
//! key to a function declaration or a typed value collected from the file.
//!
//! Type mapping is intentionally lossy. Luau primitives, the array form `{T}`,
//! function types, the unit `()`, and references to a declared `type` map onto
//! their neoc counterparts. Everything else — record/object types, unions,
//! optionals, `any`, generics, and untyped values — collapses to a single
//! opaque `LuaValue`, and the original Luau is preserved in a trailing
//! `// was: <luau>` comment so the loss is visible.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use tree_sitter::{Node, Parser};

/// Sentinel opaque type used for any Luau type that does not map onto a neoc
/// primitive, list, function type, the unit type, or a declared handle.
const LUA_VALUE: &str = "LuaValue";

/// An error raised while turning Luau into neoc declarations.
#[derive(Debug)]
pub enum Diagnostic {
    /// The `tree-sitter-luau` grammar failed to load.
    Grammar(String),
    /// The source could not be parsed into a syntax tree.
    Parse,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grammar(msg) => write!(f, "failed to load Luau grammar: {msg}"),
            Self::Parse => write!(f, "failed to parse Luau source"),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Converts a Luau source file into a declarations-only `.neoc` module string.
///
/// The `module` argument names the emitted `#[link("<module>")]` block; it is
/// typically the input file stem. The project driver sets the real `require`
/// path later. The function is pure and performs no I/O.
///
/// # Errors
///
/// Returns a [`Diagnostic`] if the Luau grammar cannot be loaded or the source
/// cannot be parsed into a syntax tree.
pub fn luau_to_neoc(source: &str, module: &str) -> Result<String, Diagnostic> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_luau::LANGUAGE.into())
        .map_err(|e| Diagnostic::Grammar(e.to_string()))?;
    let tree = parser.parse(source, None).ok_or(Diagnostic::Parse)?;
    let src = source.as_bytes();
    let root = tree.root_node();

    let declared = collect_declared_types(root, src);
    let mut ctx = Ctx {
        src,
        declared: &declared,
        used_handles: BTreeSet::new(),
        fns: BTreeMap::new(),
        values: BTreeMap::new(),
    };
    ctx.collect_functions(root);
    ctx.collect_values(root);

    let exports = collect_exports(root, src);
    let surface = ctx.resolve_surface(&exports);

    Ok(render(module, &ctx, &surface))
}

/// Names of `type T = …` aliases declared at the top level. A bare identifier
/// in type position that names one of these maps to that name; we then emit an
/// opaque `ext type <Name>;` for it.
fn collect_declared_types(root: Node, src: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cursor = root.walk();
    for stmt in root.children(&mut cursor) {
        if stmt.kind() == "type_definition" {
            if let Some(name) = stmt.child_by_field_name("name") {
                out.insert(text(name, src).to_string());
            }
        }
    }
    out
}

/// A rendered value line (`fn …;` or `const …;`) plus an optional trailing
/// `// was: <luau>` comment recording a lossy mapping.
struct Decl {
    line: String,
    was: Option<String>,
}

/// A collected function signature, keyed by its declared name (the last
/// segment of an `M.f`-style name, or a bare `f`).
struct FnSig {
    params: Vec<(String, String)>,
    ret: String,
    lossy: bool,
    /// The original Luau signature text, for a `// was:` comment.
    source: String,
}

/// A collected value with a type annotation, keyed by its declared name.
struct ValueSig {
    ty: String,
    lossy: bool,
    source: String,
}

struct Ctx<'a> {
    src: &'a [u8],
    declared: &'a BTreeSet<String>,
    used_handles: BTreeSet<String>,
    fns: BTreeMap<String, FnSig>,
    values: BTreeMap<String, ValueSig>,
}

impl Ctx<'_> {
    /// Walks every top-level `function_declaration`, recording each by its
    /// declared name. A `function M.f(…)` name is a `dot_index_expression`; we
    /// key on its final `field`. A bare `function f(…)` keys on its identifier.
    fn collect_functions(&mut self, root: Node) {
        let mut cursor = root.walk();
        for stmt in root.children(&mut cursor) {
            if stmt.kind() == "function_declaration" {
                self.collect_function(stmt);
            }
        }
    }

    fn collect_function(&mut self, decl: Node) {
        let Some(name_node) = decl.child_by_field_name("name") else {
            return;
        };
        let Some(name) = declared_name(name_node, self.src) else {
            return;
        };

        let mut lossy = false;
        let mut params = Vec::new();
        if let Some(plist) = decl.child_by_field_name("parameters") {
            let mut cursor = plist.walk();
            for p in plist.children(&mut cursor) {
                if p.kind() != "parameter" {
                    continue;
                }
                let pname = p
                    .named_child(0)
                    .filter(|n| n.kind() == "identifier")
                    .map_or_else(|| "_".to_string(), |n| text(n, self.src).to_string());
                let ty = match param_type(p) {
                    Some(t) => self.map_type(t, &mut lossy),
                    None => self.opaque(&mut lossy),
                };
                params.push((pname, ty));
            }
        }

        let ret =
            return_type(decl).map_or_else(|| "()".to_string(), |t| self.map_return(t, &mut lossy));

        self.fns.insert(
            name,
            FnSig {
                params,
                ret,
                lossy,
                source: signature_text(decl, self.src),
            },
        );
    }

    /// Walks top-level value declarations that carry a type annotation —
    /// `local NAME: T = …` and `NAME: T = …` / `M.NAME: T = …`. Untyped values
    /// are still resolvable through the export table, where they fall back to
    /// `LuaValue`.
    fn collect_values(&mut self, root: Node) {
        let mut cursor = root.walk();
        for stmt in root.children(&mut cursor) {
            match stmt.kind() {
                "variable_declaration" => {
                    if let Some(assign) = first_child_of_kind(stmt, "assignment_statement") {
                        self.collect_assignment(assign);
                    }
                }
                "assignment_statement" => self.collect_assignment(stmt),
                _ => {}
            }
        }
    }

    fn collect_assignment(&mut self, assign: Node) {
        let Some(vlist) = first_child_of_kind(assign, "variable_list") else {
            return;
        };
        let Some(target) = vlist.named_child(0) else {
            return;
        };
        let Some(name) = declared_name(target, self.src) else {
            return;
        };
        // A typed binding annotates the variable: `name: T`. The annotation is
        // the named child of `variable_list` following the target.
        let Some(ty_node) = vlist.named_child(1) else {
            return;
        };
        if !is_type_node(ty_node) {
            return;
        }
        let mut lossy = false;
        let ty = self.map_type(ty_node, &mut lossy);
        self.values.insert(
            name,
            ValueSig {
                ty,
                lossy,
                source: text(assign, self.src).trim().to_string(),
            },
        );
    }

    /// Resolves the module's public surface against the collected declarations.
    /// `exports` is the ordered list of `(key, value-name)` pairs from the final
    /// `return { … }` table. Each key resolves to a function (`fn`) or a value
    /// (`const`); a key with no matching declaration becomes an untyped
    /// `LuaValue` constant.
    fn resolve_surface(&mut self, exports: &[(String, String)]) -> Surface {
        let mut fns = Vec::new();
        let mut consts = Vec::new();
        for (key, value) in exports {
            if let Some(sig) = self.fns.get(value) {
                fns.push(render_fn(key, sig));
            } else if let Some(val) = self.values.get(value) {
                consts.push(render_value(key, val));
            } else {
                // Untyped export — no type information anywhere.
                self.used_handles.insert(LUA_VALUE.to_string());
                consts.push(Decl {
                    line: format!("const {}: {LUA_VALUE};", export_const_name(key)),
                    was: Some(format!("{key} = {value}")),
                });
            }
        }
        // Re-mark handles for the rendered surface (render_fn/render_value note
        // handles via the stored types, not the map walk).
        for decl in fns.iter().chain(&consts) {
            self.note_handles(&decl.line);
        }
        Surface { fns, consts }
    }

    /// Records any opaque handle (`LuaValue`, a declared type) that appears in a
    /// rendered line so it is emitted as a standalone `ext type`.
    fn note_handles(&mut self, line: &str) {
        if line.contains(LUA_VALUE) {
            self.used_handles.insert(LUA_VALUE.to_string());
        }
        for name in self.declared {
            // Word-boundary-ish check: the handle must appear as a whole token.
            if line
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|t| t == name)
            {
                self.used_handles.insert(name.clone());
            }
        }
    }

    /// Maps a Luau type in return position. The unit `()` (`empty_type`) and
    /// `nil` become the neoc unit `()`; otherwise identical to
    /// [`Self::map_type`].
    fn map_return(&mut self, node: Node, lossy: &mut bool) -> String {
        match node.kind() {
            "empty_type" => "()".to_string(),
            "builtin_type" if text(node, self.src) == "nil" => "()".to_string(),
            _ => self.map_type(node, lossy),
        }
    }

    /// Maps a Luau type onto a neoc type. Anything unmappable collapses to
    /// [`LUA_VALUE`] and sets `lossy`.
    fn map_type(&mut self, node: Node, lossy: &mut bool) -> String {
        match node.kind() {
            "builtin_type" => match text(node, self.src) {
                "number" => "number".to_string(),
                "string" => "string".to_string(),
                "boolean" => "bool".to_string(),
                "nil" => "()".to_string(),
                _ => self.opaque(lossy),
            },
            "identifier" => {
                let name = text(node, self.src);
                if self.declared.contains(name) {
                    self.used_handles.insert(name.to_string());
                    name.to_string()
                } else {
                    self.opaque(lossy)
                }
            }
            "object_type" => self.map_object(node, lossy),
            "function_type" => self.map_function_type(node, lossy),
            _ => self.opaque(lossy),
        }
    }

    /// An `object_type` is the array form `{T}` (→ `List<T>`) only when its sole
    /// type child has no preceding field name; a record `{a: T, …}` falls back.
    fn map_object(&mut self, node: Node, lossy: &mut bool) -> String {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        let is_array = children.len() == 1 && is_type_node(children[0]);
        if is_array {
            let inner = self.map_type(children[0], lossy);
            format!("List<{inner}>")
        } else {
            self.opaque(lossy)
        }
    }

    /// Maps `(A, B) -> R` onto `fn(A, B) -> R`. A `()` / `nil` return is
    /// dropped, yielding `fn(A, B)`.
    fn map_function_type(&mut self, node: Node, lossy: &mut bool) -> String {
        let mut params = Vec::new();
        let mut ret = "()".to_string();
        let mut seen_arrow = false;
        for i in 0..node.child_count() {
            let child = node.child(i).unwrap();
            if child.kind() == "->" {
                seen_arrow = true;
                continue;
            }
            if !child.is_named() || !is_type_node(child) {
                continue;
            }
            if seen_arrow {
                ret = self.map_return(child, lossy);
            } else {
                params.push(self.map_type(child, lossy));
            }
        }
        let ret_clause = if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        };
        format!("fn({}){ret_clause}", params.join(", "))
    }

    fn opaque(&mut self, lossy: &mut bool) -> String {
        *lossy = true;
        self.used_handles.insert(LUA_VALUE.to_string());
        LUA_VALUE.to_string()
    }
}

/// The resolved public surface, split into the two value kinds the output block
/// emits (functions first, then constants).
struct Surface {
    fns: Vec<Decl>,
    consts: Vec<Decl>,
}

/// Renders a function export as `fn name(p: T, …) -> R;`, dropping the
/// `-> ()` clause for a unit return.
fn render_fn(key: &str, sig: &FnSig) -> Decl {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect();
    let ret_clause = if sig.ret == "()" {
        String::new()
    } else {
        format!(" -> {}", sig.ret)
    };
    let line = format!("fn {key}({}){ret_clause};", params.join(", "));
    let was = sig.lossy.then(|| sig.source.clone());
    Decl { line, was }
}

/// Renders a typed value export as `const NAME: T;`.
fn render_value(key: &str, val: &ValueSig) -> Decl {
    let line = format!("const {}: {};", export_const_name(key), val.ty);
    let was = val.lossy.then(|| val.source.clone());
    Decl { line, was }
}

/// Reads the file's final `return { … }` table and returns the ordered list of
/// `(export-key, value-name)` pairs. The value name is the last segment of a
/// `dot_index_expression` value (`M.f` → `f`) or a bare identifier (`PI`).
/// A self-referential field (`f = f`) resolves the key against the same name.
fn collect_exports(root: Node, src: &[u8]) -> Vec<(String, String)> {
    let mut cursor = root.walk();
    let Some(ret) = root
        .children(&mut cursor)
        .filter(|c| c.kind() == "return_statement")
        .last()
    else {
        return Vec::new();
    };
    let Some(list) = first_child_of_kind(ret, "expression_list") else {
        return Vec::new();
    };
    let Some(table) = first_child_of_kind(list, "table_constructor") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut c = table.walk();
    for field in table.named_children(&mut c) {
        if field.kind() != "field" {
            continue;
        }
        let Some(key) = field
            .child_by_field_name("name")
            .map(|n| text(n, src).to_string())
        else {
            continue;
        };
        let value = field
            .child_by_field_name("value")
            .and_then(|v| declared_name(v, src))
            .unwrap_or_else(|| key.clone());
        out.push((key, value));
    }
    out
}

/// The declared name of a function/value target: the final `field` of a
/// `dot_index_expression` (`M.f` → `f`) or a bare `identifier`.
fn declared_name(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, src).to_string()),
        "dot_index_expression" => node
            .child_by_field_name("field")
            .map(|f| text(f, src).to_string()),
        _ => None,
    }
}

/// The neoc constant name for an export key. neoc consts are conventionally
/// `SCREAMING_CASE`; we pass the Luau key through unchanged so the binding name
/// matches the runtime field.
fn export_const_name(key: &str) -> String {
    key.to_string()
}

/// The type annotation of a `parameter`, if any. A parameter is
/// `identifier [: type]`; the type is the parameter's second named child when
/// it is a type node.
fn param_type(param: Node) -> Option<Node> {
    let ty = param.named_child(1)?;
    is_type_node(ty).then_some(ty)
}

/// The return type of a `function_declaration`, if annotated. The grammar
/// places it as a named child after `parameters` with no field name; it is the
/// first type node that is neither `name` nor `parameters` nor `body`.
fn return_type(decl: Node) -> Option<Node> {
    let name = decl.child_by_field_name("name");
    let params = decl.child_by_field_name("parameters");
    let body = decl.child_by_field_name("body");
    let mut cursor = decl.walk();
    decl.named_children(&mut cursor)
        .find(|c| Some(*c) != name && Some(*c) != params && Some(*c) != body && is_type_node(*c))
}

/// Whether a node sits in type position and is one we know how to read. A bare
/// `identifier` counts as a named type reference (`Rgb`); the caller only ever
/// passes nodes drawn from a type slot, so an identifier there is a type name.
fn is_type_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "builtin_type"
            | "object_type"
            | "function_type"
            | "optional_type"
            | "union_type"
            | "empty_type"
            | "generic_type"
            | "type_identifier"
            | "identifier"
    )
}

/// The source text of a function's signature: from `function` through the
/// return type, excluding the body, trimmed and single-lined.
fn signature_text(decl: Node, src: &[u8]) -> String {
    let full = text(decl, src);
    let head = decl.child_by_field_name("body").map_or(full, |body| {
        &full[..(body.start_byte() - decl.start_byte())]
    });
    let collapsed: String = head.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim()
        .trim_end_matches("function")
        .trim()
        .to_string()
}

/// Renders the collected surface into the final `.neoc` module text.
/// Order inside the block: `fn` then `const`. Opaque `ext type` handles follow
/// the block, standalone.
fn render(module: &str, ctx: &Ctx, surface: &Surface) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by neoc-bindgen-lua from {module} — do not edit."
    );

    let has_values = !surface.fns.is_empty() || !surface.consts.is_empty();
    if has_values {
        let _ = writeln!(out, "#[link(\"{module}\")]");
        let _ = writeln!(out, "ext {{");
        for decl in surface.fns.iter().chain(&surface.consts) {
            push_decl(&mut out, decl);
        }
        out.push_str("}\n");
    }

    if !ctx.used_handles.is_empty() {
        if has_values {
            out.push('\n');
        }
        for handle in &ctx.used_handles {
            let _ = writeln!(out, "ext type {handle};");
        }
    }

    out
}

fn push_decl(out: &mut String, decl: &Decl) {
    out.push_str("    ");
    out.push_str(&decl.line);
    if let Some(was) = &decl.was {
        let _ = write!(out, " // was: {was}");
    }
    out.push('\n');
}

fn first_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Maps a single Luau type by parsing a `function f(x: <ty>) end` and
    /// reading back the neoc type. Returns `(neoc_type, was_lossy)`.
    fn map_one(luau_type: &str) -> (String, bool) {
        let source = format!("function f(x: {luau_type}) end");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let src = source.as_bytes();
        let declared = collect_declared_types(tree.root_node(), src);
        let mut ctx = Ctx {
            src,
            declared: &declared,
            used_handles: BTreeSet::new(),
            fns: BTreeMap::new(),
            values: BTreeMap::new(),
        };
        // chunk > function_declaration > parameters > parameter > <type>.
        let func = tree.root_node().named_child(0).unwrap();
        let plist = func.child_by_field_name("parameters").unwrap();
        let param = first_child_of_kind(plist, "parameter").unwrap();
        let ty = param_type(param).unwrap();
        let mut lossy = false;
        let mapped = ctx.map_type(ty, &mut lossy);
        (mapped, lossy)
    }

    #[test]
    fn primitives_map_cleanly() {
        assert_eq!(map_one("number"), ("number".to_string(), false));
        assert_eq!(map_one("string"), ("string".to_string(), false));
        assert_eq!(map_one("boolean"), ("bool".to_string(), false));
    }

    #[test]
    fn array_object_maps_to_list() {
        assert_eq!(map_one("{string}"), ("List<string>".to_string(), false));
        assert_eq!(map_one("{number}"), ("List<number>".to_string(), false));
    }

    #[test]
    fn function_types_map() {
        assert_eq!(
            map_one("(number, string) -> boolean"),
            ("fn(number, string) -> bool".to_string(), false)
        );
        assert_eq!(map_one("(number) -> ()"), ("fn(number)".to_string(), false));
    }

    #[test]
    fn declared_type_maps_to_handle() {
        // A type declared in the same source resolves to its name.
        let source = "type Rgb = { r: number }\nfunction f(x: Rgb) end";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let src = source.as_bytes();
        let declared = collect_declared_types(tree.root_node(), src);
        assert!(declared.contains("Rgb"));
        let mut ctx = Ctx {
            src,
            declared: &declared,
            used_handles: BTreeSet::new(),
            fns: BTreeMap::new(),
            values: BTreeMap::new(),
        };
        let func = tree.root_node().named_child(1).unwrap();
        let plist = func.child_by_field_name("parameters").unwrap();
        let param = first_child_of_kind(plist, "parameter").unwrap();
        let ty = param_type(param).unwrap();
        let mut lossy = false;
        assert_eq!(ctx.map_type(ty, &mut lossy), "Rgb".to_string());
        assert!(!lossy);
    }

    #[test]
    fn unmappable_types_fall_back_to_luavalue() {
        assert_eq!(map_one("any"), (LUA_VALUE.to_string(), true));
        assert_eq!(map_one("number?"), (LUA_VALUE.to_string(), true));
        assert_eq!(map_one("number | string"), (LUA_VALUE.to_string(), true));
        assert_eq!(map_one("{ r: number }"), (LUA_VALUE.to_string(), true));
    }

    #[test]
    fn unknown_identifier_falls_back() {
        // `Color` isn't declared here, so it's opaque -> LuaValue.
        assert_eq!(map_one("Color"), (LUA_VALUE.to_string(), true));
    }
}
