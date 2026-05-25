//! Reads a TypeScript declaration source (`.d.ts` / `.ts`) and emits a
//! declarations-only `.neoc` file: opaque handles as standalone `ext type`
//! items, and the value declarations (no bodies) inside a `#[link("<name>")]`
//! `ext { ... }` block. Each backend lowers the block into a native import from
//! the named module.
//!
//! Parsing uses `tree-sitter` with the `tree-sitter-typescript` grammar. Only
//! top-level `export`s are extracted; everything else is ignored. Type mapping
//! is intentionally lossy: anything that does not map onto a neoc primitive,
//! `List<T>`, `fn(..) -> ..`, or a declared handle collapses to a single
//! opaque `JsValue`, and the original TypeScript is preserved in a trailing
//! `// was: <ts>` comment so the loss is visible.

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use tree_sitter::{Node, Parser};

/// Sentinel opaque type used for any TypeScript type that does not map onto a
/// neoc primitive, list, function type, or a declared handle.
const JS_VALUE: &str = "JsValue";

/// An error raised while turning TypeScript into neoc declarations.
#[derive(Debug)]
pub enum Diagnostic {
    /// The `tree-sitter-typescript` grammar failed to load.
    Grammar(String),
    /// The source could not be parsed into a syntax tree.
    Parse,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grammar(msg) => write!(f, "failed to load TypeScript grammar: {msg}"),
            Self::Parse => write!(f, "failed to parse TypeScript source"),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Converts a TypeScript declaration source into a declarations-only `.neoc`
/// module string.
///
/// The `module` argument names the emitted `#[link("<module>")]` `ext` block;
/// it is typically the input file stem with any trailing `.d` removed. The
/// function is pure and performs no I/O.
///
/// # Errors
///
/// Returns a [`Diagnostic`] if the TypeScript grammar cannot be loaded or the
/// source cannot be parsed into a syntax tree.
pub fn ts_to_neoc(source: &str, module: &str) -> Result<String, Diagnostic> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|e| Diagnostic::Grammar(e.to_string()))?;
    let tree = parser.parse(source, None).ok_or(Diagnostic::Parse)?;
    let src = source.as_bytes();
    let root = tree.root_node();

    let declared = collect_declared_types(root, src);
    let mut ctx = Ctx {
        src,
        declared: &declared,
        used_handles: BTreeSet::new(),
        fns: Vec::new(),
        consts: Vec::new(),
    };
    ctx.walk_exports(root);

    Ok(render(module, &ctx))
}

/// Names of exported interfaces, classes, and type aliases declared in the
/// source. A bare identifier that names one of these maps to that name; we then
/// emit an opaque `ext type <Name>;` for it.
fn collect_declared_types(root: Node, src: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cursor = root.walk();
    for stmt in root.children(&mut cursor) {
        if stmt.kind() != "export_statement" {
            continue;
        }
        let Some(decl) = stmt.child_by_field_name("declaration") else {
            continue;
        };
        match decl.kind() {
            "interface_declaration" | "class_declaration" | "type_alias_declaration" => {
                if let Some(name) = decl.child_by_field_name("name") {
                    out.insert(text(name, src).to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// A rendered `ext fn` or `ext const` line, plus an optional trailing
/// `// was: <ts>` comment recording a lossy mapping.
struct Decl {
    line: String,
    was: Option<String>,
}

struct Ctx<'a> {
    src: &'a [u8],
    declared: &'a BTreeSet<String>,
    used_handles: BTreeSet<String>,
    fns: Vec<Decl>,
    consts: Vec<Decl>,
}

impl Ctx<'_> {
    fn walk_exports(&mut self, root: Node) {
        let mut cursor = root.walk();
        for stmt in root.children(&mut cursor) {
            if stmt.kind() != "export_statement" {
                continue;
            }
            let Some(decl) = stmt.child_by_field_name("declaration") else {
                continue;
            };
            match decl.kind() {
                "function_signature" => self.emit_fn(decl),
                "lexical_declaration" | "variable_declaration" => self.emit_const(decl),
                "interface_declaration" | "class_declaration" | "type_alias_declaration" => {
                    if let Some(name) = decl.child_by_field_name("name") {
                        self.used_handles.insert(text(name, self.src).to_string());
                    }
                }
                _ => {}
            }
        }
    }

    fn emit_fn(&mut self, decl: Node) {
        let Some(name) = decl.child_by_field_name("name") else {
            return;
        };
        let name = text(name, self.src);

        let mut lossy = false;
        let mut params = Vec::new();
        if let Some(formal) = decl.child_by_field_name("parameters") {
            let mut cursor = formal.walk();
            for p in formal.children(&mut cursor) {
                if p.kind() != "required_parameter" && p.kind() != "optional_parameter" {
                    continue;
                }
                let pname = p
                    .child_by_field_name("pattern")
                    .map_or_else(|| "_".to_string(), |n| text(n, self.src).to_string());
                let ty = p
                    .child_by_field_name("type")
                    .and_then(|t| t.named_child(0))
                    .map_or_else(|| JS_VALUE.to_string(), |t| self.map_type(t, &mut lossy));
                params.push(format!("{pname}: {ty}"));
            }
        }

        let ret = decl
            .child_by_field_name("return_type")
            .and_then(|t| t.named_child(0))
            .map_or_else(|| "()".to_string(), |t| self.map_return(t, &mut lossy));

        let ret_clause = if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        };
        let line = format!("fn {name}({}){ret_clause};", params.join(", "));
        let was = lossy.then(|| text(decl, self.src).trim().to_string());
        self.fns.push(Decl { line, was });
    }

    fn emit_const(&mut self, decl: Node) {
        let Some(declarator) = first_child_of_kind(decl, "variable_declarator") else {
            return;
        };
        let Some(name) = declarator.child_by_field_name("name") else {
            return;
        };
        let name = text(name, self.src);
        let mut lossy = false;
        let ty = declarator
            .child_by_field_name("type")
            .and_then(|t| t.named_child(0))
            .map_or_else(|| JS_VALUE.to_string(), |t| self.map_type(t, &mut lossy));
        if ty == JS_VALUE {
            self.used_handles.insert(JS_VALUE.to_string());
        }
        let line = format!("const {name}: {ty};");
        let was = lossy.then(|| text(decl, self.src).trim().to_string());
        self.consts.push(Decl { line, was });
    }

    /// Maps a TypeScript type in return position. `void` / `undefined` become
    /// the unit type `()`; otherwise identical to [`Self::map_type`].
    fn map_return(&mut self, node: Node, lossy: &mut bool) -> String {
        if node.kind() == "predefined_type" {
            let t = text(node, self.src);
            if t == "void" || t == "undefined" {
                return "()".to_string();
            }
        }
        self.map_type(node, lossy)
    }

    /// Maps a TypeScript type onto a neoc type. Anything unmappable collapses to
    /// [`JS_VALUE`] and sets `lossy`.
    fn map_type(&mut self, node: Node, lossy: &mut bool) -> String {
        match node.kind() {
            "predefined_type" => match text(node, self.src) {
                "number" => "number".to_string(),
                "string" => "string".to_string(),
                "boolean" => "bool".to_string(),
                _ => self.fallback(lossy),
            },
            "type_identifier" => {
                let name = text(node, self.src);
                if self.declared.contains(name) {
                    self.used_handles.insert(name.to_string());
                    name.to_string()
                } else {
                    self.fallback(lossy)
                }
            }
            "array_type" => {
                // `T[]` — the element type is the first named child.
                let inner = node
                    .named_child(0)
                    .map_or_else(|| JS_VALUE.to_string(), |t| self.map_type(t, lossy));
                if inner == JS_VALUE {
                    self.used_handles.insert(JS_VALUE.to_string());
                }
                format!("List<{inner}>")
            }
            "generic_type" => self.map_generic(node, lossy),
            "function_type" => self.map_function_type(node, lossy),
            _ => self.fallback(lossy),
        }
    }

    /// Handles `Array<T>` / `ReadonlyArray<T>` (-> `List<T>`); every other
    /// generic (`Map`, `Promise`, …) falls back to [`JS_VALUE`].
    fn map_generic(&mut self, node: Node, lossy: &mut bool) -> String {
        let name = node
            .child_by_field_name("name")
            .map_or("", |n| text(n, self.src));
        if name == "Array" || name == "ReadonlyArray" {
            let inner = node
                .child_by_field_name("type_arguments")
                .and_then(|a| a.named_child(0))
                .map_or_else(|| JS_VALUE.to_string(), |t| self.map_type(t, lossy));
            if inner == JS_VALUE {
                self.used_handles.insert(JS_VALUE.to_string());
            }
            format!("List<{inner}>")
        } else {
            self.fallback(lossy)
        }
    }

    /// Maps `(a: A, b: B) => R` onto `fn(A, B) -> R`. A `void` / `undefined`
    /// return is dropped, yielding `fn(A, B)`.
    fn map_function_type(&mut self, node: Node, lossy: &mut bool) -> String {
        let mut params = Vec::new();
        if let Some(formal) = node.child_by_field_name("parameters") {
            let mut cursor = formal.walk();
            for p in formal.children(&mut cursor) {
                if p.kind() != "required_parameter" && p.kind() != "optional_parameter" {
                    continue;
                }
                let ty = p
                    .child_by_field_name("type")
                    .and_then(|t| t.named_child(0))
                    .map_or_else(|| JS_VALUE.to_string(), |t| self.map_type(t, lossy));
                params.push(ty);
            }
        }
        let ret = node
            .child_by_field_name("return_type")
            .map_or_else(|| "()".to_string(), |t| self.map_return(t, lossy));
        let ret_clause = if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        };
        format!("fn({}){ret_clause}", params.join(", "))
    }

    fn fallback(&mut self, lossy: &mut bool) -> String {
        *lossy = true;
        self.used_handles.insert(JS_VALUE.to_string());
        JS_VALUE.to_string()
    }
}

/// Renders the collected declarations into the final `.neoc` module text in the
/// compilable form: opaque handles as standalone `ext type` items (erased, no
/// module), then a `#[link("<name>")]` `ext { ... }` block holding the value
/// declarations (`fn`, then `const`). Each backend lowers the block to a native
/// import from the named module.
fn render(module: &str, ctx: &Ctx) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by neoc-bindgen-ts from {module} — do not edit."
    );

    for handle in &ctx.used_handles {
        let _ = writeln!(out, "ext type {handle};");
    }

    if ctx.fns.is_empty() && ctx.consts.is_empty() {
        return out;
    }

    let _ = writeln!(out, "#[link(\"{module}\")]");
    let _ = writeln!(out, "ext {{");
    for decl in ctx.fns.iter().chain(&ctx.consts) {
        push_decl(&mut out, decl);
    }
    out.push_str("}\n");
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

    /// Maps a single TS type by parsing a `const x: <ty>;` and reading back the
    /// neoc type. Returns `(neoc_type, was_lossy)`.
    fn map_one(ts_type: &str) -> (String, bool) {
        let source = format!("export const x: {ts_type};");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let src = source.as_bytes();
        let declared = collect_declared_types(tree.root_node(), src);
        let mut ctx = Ctx {
            src,
            declared: &declared,
            used_handles: BTreeSet::new(),
            fns: Vec::new(),
            consts: Vec::new(),
        };
        // Reach the type node: export_statement > lexical_declaration >
        // variable_declarator > type_annotation > <type>.
        let stmt = tree.root_node().named_child(0).unwrap();
        let decl = stmt.child_by_field_name("declaration").unwrap();
        let declarator = first_child_of_kind(decl, "variable_declarator").unwrap();
        let ty = declarator
            .child_by_field_name("type")
            .unwrap()
            .named_child(0)
            .unwrap();
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
    fn arrays_map_to_list() {
        assert_eq!(map_one("number[]"), ("List<number>".to_string(), false));
        assert_eq!(
            map_one("Array<string>"),
            ("List<string>".to_string(), false)
        );
        assert_eq!(
            map_one("ReadonlyArray<boolean>"),
            ("List<bool>".to_string(), false)
        );
    }

    #[test]
    fn function_types_map() {
        assert_eq!(
            map_one("(a: number, b: string) => boolean"),
            ("fn(number, string) -> bool".to_string(), false)
        );
        assert_eq!(
            map_one("(a: number) => void"),
            ("fn(number)".to_string(), false)
        );
    }

    #[test]
    fn unmappable_types_fall_back_to_jsvalue() {
        assert_eq!(map_one("any"), (JS_VALUE.to_string(), true));
        assert_eq!(map_one("unknown"), (JS_VALUE.to_string(), true));
        assert_eq!(map_one("A | B"), (JS_VALUE.to_string(), true));
        assert_eq!(map_one("Map<string, number>"), (JS_VALUE.to_string(), true));
        assert_eq!(map_one("Promise<void>"), (JS_VALUE.to_string(), true));
        assert_eq!(map_one("[number, string]"), (JS_VALUE.to_string(), true));
    }

    #[test]
    fn unknown_identifier_falls_back() {
        // `Rgb` isn't declared in this snippet, so it's opaque -> JsValue.
        assert_eq!(map_one("Rgb"), (JS_VALUE.to_string(), true));
    }
}
