//! Reads a Python source or stub (`.py` / `.pyi`) and emits a declarations-only
//! `.neoc` header: value items (`fn` / `const`) inside a
//! `#[link("<name>")] ext { … }` block, plus standalone `ext type <Name>;`
//! lines for opaque handles.
//!
//! Parsing uses `tree-sitter` with the `tree-sitter-python` grammar. Only
//! module-level declarations are extracted; private names (leading underscore)
//! and imports are skipped. Type mapping reads PEP 484 hints and is
//! intentionally lossy: anything that does not map onto a neoc primitive,
//! `List<T>`, `fn(..) -> ..`, the unit type `()`, or a class declared in the
//! file collapses to a single opaque `PyValue`, with the original Python type
//! preserved in a trailing `// was: <type>` comment so the loss is visible.

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use tree_sitter::{Node, Parser};

/// Sentinel opaque type used for any Python type that does not map onto a neoc
/// primitive, list, function type, the unit type, or a class declared in the
/// file.
const PY_VALUE: &str = "PyValue";

/// An error raised while turning Python into neoc declarations.
#[derive(Debug)]
pub enum Diagnostic {
    /// The `tree-sitter-python` grammar failed to load.
    Grammar(String),
    /// The source could not be parsed into a syntax tree.
    Parse,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grammar(msg) => write!(f, "failed to load Python grammar: {msg}"),
            Self::Parse => write!(f, "failed to parse Python source"),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Converts a Python source or stub into a declarations-only `.neoc` header
/// string.
///
/// The `module` argument names the emitted `#[link("<module>")]` block; it is
/// typically the input file stem. The function is pure and performs no I/O.
///
/// # Errors
///
/// Returns a [`Diagnostic`] if the Python grammar cannot be loaded or the
/// source cannot be parsed into a syntax tree.
pub fn py_to_neoc(source: &str, module: &str) -> Result<String, Diagnostic> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| Diagnostic::Grammar(e.to_string()))?;
    let tree = parser.parse(source, None).ok_or(Diagnostic::Parse)?;
    let src = source.as_bytes();
    let root = tree.root_node();

    let declared = collect_classes(root, src);
    let mut ctx = Ctx {
        src,
        declared: &declared,
        used_handles: BTreeSet::new(),
        fns: Vec::new(),
        consts: Vec::new(),
    };
    ctx.walk(root);

    Ok(render(module, &ctx))
}

/// Names of public classes declared at module level. A type hint referring to
/// one of these maps to that handle; we then emit an opaque `ext type <Name>;`.
fn collect_classes(root: Node, src: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cursor = root.walk();
    for stmt in root.children(&mut cursor) {
        if stmt.kind() != "class_definition" {
            continue;
        }
        if let Some(name) = stmt.child_by_field_name("name") {
            let name = text(name, src);
            if !is_private(name) {
                out.insert(name.to_string());
            }
        }
    }
    out
}

/// A rendered `fn` or `const` line, plus an optional trailing `// was: <py>`
/// comment recording a lossy mapping.
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
    fn walk(&mut self, root: Node) {
        let mut cursor = root.walk();
        for stmt in root.children(&mut cursor) {
            match stmt.kind() {
                "function_definition" => self.emit_fn(stmt),
                "expression_statement" => {
                    if let Some(assign) = first_child_of_kind(stmt, "assignment") {
                        self.emit_const(assign);
                    }
                }
                "class_definition" => {
                    if let Some(name) = stmt.child_by_field_name("name") {
                        let name = text(name, self.src);
                        if !is_private(name) {
                            self.used_handles.insert(name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn emit_fn(&mut self, decl: Node) {
        let Some(name_node) = decl.child_by_field_name("name") else {
            return;
        };
        let name = text(name_node, self.src);
        if is_private(name) {
            return;
        }

        let mut lossy = false;
        let mut params = Vec::new();
        if let Some(formal) = decl.child_by_field_name("parameters") {
            let mut cursor = formal.walk();
            for p in formal.named_children(&mut cursor) {
                match p.kind() {
                    "typed_parameter" => {
                        let pname = p
                            .named_child(0)
                            .map_or("_", |n| text(n, self.src))
                            .to_string();
                        let ty = match p.child_by_field_name("type").and_then(|t| t.named_child(0))
                        {
                            Some(t) => self.map_type(t, &mut lossy),
                            None => self.fallback(&mut lossy),
                        };
                        params.push(format!("{pname}: {ty}"));
                    }
                    "identifier" => {
                        // Unannotated parameter — opaque fallback.
                        let pname = text(p, self.src).to_string();
                        let ty = self.fallback(&mut lossy);
                        params.push(format!("{pname}: {ty}"));
                    }
                    _ => {}
                }
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
        let was = lossy.then(|| signature_text(decl, self.src));
        self.fns.push(Decl { line, was });
    }

    fn emit_const(&mut self, assign: Node) {
        let Some(name_node) = assign.child_by_field_name("left") else {
            return;
        };
        if name_node.kind() != "identifier" {
            return;
        }
        let name = text(name_node, self.src);
        if is_private(name) {
            return;
        }
        // Only annotated module vars become consts; bare `X = 1` has no type.
        let Some(ty_node) = assign
            .child_by_field_name("type")
            .and_then(|t| t.named_child(0))
        else {
            return;
        };
        let mut lossy = false;
        let ty = self.map_type(ty_node, &mut lossy);
        let line = format!("const {name}: {ty};");
        let was = lossy.then(|| const_text(assign, self.src));
        self.consts.push(Decl { line, was });
    }

    /// Maps a Python type in return position. `None` becomes the unit type
    /// `()`; otherwise identical to [`Self::map_type`].
    fn map_return(&mut self, node: Node, lossy: &mut bool) -> String {
        if node.kind() == "none" {
            return "()".to_string();
        }
        self.map_type(node, lossy)
    }

    /// Maps a Python type hint onto a neoc type. Anything unmappable collapses
    /// to [`PY_VALUE`] and sets `lossy`.
    fn map_type(&mut self, node: Node, lossy: &mut bool) -> String {
        // `none` (a bare `None` hint), unions, tuples, etc. all hit the
        // wildcard fallback.
        match node.kind() {
            "identifier" => self.map_name(text(node, self.src), lossy),
            "generic_type" => self.map_generic(node, lossy),
            _ => self.fallback(lossy),
        }
    }

    /// Maps a bare type name: primitives map cleanly; a class declared in the
    /// file maps to its handle; everything else falls back to [`PY_VALUE`].
    fn map_name(&mut self, name: &str, lossy: &mut bool) -> String {
        match name {
            "int" | "float" => "number".to_string(),
            "str" => "string".to_string(),
            "bool" => "bool".to_string(),
            _ if self.declared.contains(name) => {
                self.used_handles.insert(name.to_string());
                name.to_string()
            }
            _ => self.fallback(lossy),
        }
    }

    /// Handles `list[T]` / `List[T]` / `Sequence[T]` (-> `List<T>`) and
    /// `Callable[[A, B], R]` (-> `fn(A, B) -> R`); every other generic
    /// (`dict`, `Optional`, `Any`, tuples, …) falls back to [`PY_VALUE`].
    fn map_generic(&mut self, node: Node, lossy: &mut bool) -> String {
        let name = node.named_child(0).map_or("", |n| text(n, self.src));
        let arg = node.named_child(1);
        match name {
            "list" | "List" | "Sequence" => {
                let inner = match arg
                    .and_then(|a| a.named_child(0))
                    .and_then(|t| t.named_child(0))
                {
                    Some(t) => self.map_type(t, lossy),
                    None => self.fallback(lossy),
                };
                format!("List<{inner}>")
            }
            "Callable" => self.map_callable(arg, lossy),
            _ => self.fallback(lossy),
        }
    }

    /// Maps `Callable[[A, B], R]` onto `fn(A, B) -> R`. The `type_parameter`
    /// holds the argument list as its first `(list …)` child and the return
    /// type as its second child.
    fn map_callable(&mut self, arg: Option<Node>, lossy: &mut bool) -> String {
        let Some(tp) = arg else {
            return self.fallback(lossy);
        };
        let mut cursor = tp.walk();
        let children: Vec<Node> = tp.named_children(&mut cursor).collect();
        let mut params = Vec::new();
        // First child wraps the argument list: `(type (list …))`.
        if let Some(arg_list) = children.first().and_then(|t| t.named_child(0)) {
            if arg_list.kind() == "list" {
                let mut c = arg_list.walk();
                for a in arg_list.named_children(&mut c) {
                    params.push(self.map_type(a, lossy));
                }
            } else {
                return self.fallback(lossy);
            }
        } else {
            return self.fallback(lossy);
        }
        // Second child is the return type: `(type …)`.
        let ret = children
            .get(1)
            .and_then(|t| t.named_child(0))
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
        self.used_handles.insert(PY_VALUE.to_string());
        PY_VALUE.to_string()
    }
}

/// Renders the collected declarations into the final `.neoc` header text.
///
/// Value items (`fn` then `const`) go inside the
/// `#[link("<module>")] ext { … }` block; opaque handles follow as standalone
/// `ext type <Name>;` lines. The `ext` block is omitted entirely when there are
/// no value items.
fn render(module: &str, ctx: &Ctx) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by neoc-bindgen-py from {module} — do not edit."
    );

    let has_values = !ctx.fns.is_empty() || !ctx.consts.is_empty();
    if has_values {
        let _ = writeln!(out, "#[link(\"{module}\")]");
        out.push_str("ext {\n");
        for decl in ctx.fns.iter().chain(&ctx.consts) {
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

/// Renders a `// was:` comment for a function: its signature up to (but not
/// including) the body, on a single line.
fn signature_text(decl: Node, src: &[u8]) -> String {
    let name = decl
        .child_by_field_name("name")
        .map_or("", |n| text(n, src));
    let params = decl
        .child_by_field_name("parameters")
        .map_or("()", |n| text(n, src));
    let ret = decl
        .child_by_field_name("return_type")
        .map(|n| format!(" -> {}", text(n, src)));
    format!("def {name}{params}{}", ret.unwrap_or_default())
}

/// Renders a `// was:` comment for an annotated module var: `NAME: <type>`,
/// dropping any initialiser.
fn const_text(assign: Node, src: &[u8]) -> String {
    let name = assign
        .child_by_field_name("left")
        .map_or("", |n| text(n, src));
    let ty = assign
        .child_by_field_name("type")
        .map_or("", |n| text(n, src));
    format!("{name}: {ty}")
}

fn is_private(name: &str) -> bool {
    name.starts_with('_')
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

    /// Maps a single Python type by parsing a `x: <ty>` annotation and reading
    /// back the neoc type. Returns `(neoc_type, was_lossy)`.
    fn map_one(py_type: &str) -> (String, bool) {
        let source = format!("x: {py_type}\n");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let src = source.as_bytes();
        let declared = collect_classes(tree.root_node(), src);
        let mut ctx = Ctx {
            src,
            declared: &declared,
            used_handles: BTreeSet::new(),
            fns: Vec::new(),
            consts: Vec::new(),
        };
        // module > expression_statement > assignment > type > <type>.
        let stmt = tree.root_node().named_child(0).unwrap();
        let assign = first_child_of_kind(stmt, "assignment").unwrap();
        let ty = assign
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
        assert_eq!(map_one("int"), ("number".to_string(), false));
        assert_eq!(map_one("float"), ("number".to_string(), false));
        assert_eq!(map_one("str"), ("string".to_string(), false));
        assert_eq!(map_one("bool"), ("bool".to_string(), false));
    }

    #[test]
    fn lists_map_to_list() {
        assert_eq!(map_one("list[int]"), ("List<number>".to_string(), false));
        assert_eq!(map_one("List[str]"), ("List<string>".to_string(), false));
        assert_eq!(map_one("Sequence[bool]"), ("List<bool>".to_string(), false));
    }

    #[test]
    fn callable_maps_to_fn() {
        assert_eq!(
            map_one("Callable[[int, str], bool]"),
            ("fn(number, string) -> bool".to_string(), false)
        );
        assert_eq!(
            map_one("Callable[[int], None]"),
            ("fn(number)".to_string(), false)
        );
    }

    #[test]
    fn unmappable_types_fall_back_to_pyvalue() {
        assert_eq!(map_one("Any"), (PY_VALUE.to_string(), true));
        assert_eq!(map_one("dict"), (PY_VALUE.to_string(), true));
        assert_eq!(map_one("Optional[int]"), (PY_VALUE.to_string(), true));
        assert_eq!(map_one("int | None"), (PY_VALUE.to_string(), true));
        assert_eq!(map_one("dict[str, int]"), (PY_VALUE.to_string(), true));
        assert_eq!(map_one("tuple[int, str]"), (PY_VALUE.to_string(), true));
    }

    #[test]
    fn unknown_name_falls_back() {
        // `Rgb` isn't declared in this snippet, so it's opaque -> PyValue.
        assert_eq!(map_one("Rgb"), (PY_VALUE.to_string(), true));
    }
}
