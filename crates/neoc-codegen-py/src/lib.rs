//! AST → Python 3 emitter. Follows the lowering table in `lang/DYNAMIC.md`.
//!
//! Targets Python 3.9 — no `match` statement and no `X | Y` patterns, so neoc
//! `match` lowers to an `if`/`elif` chain testing a tag/field. Output runs
//! directly under `python3`.
//!
//! Split by construct family: items here, expressions in [`expr`], patterns in
//! [`pat`]. neoc is expression-oriented while Python statements cannot appear in
//! expression position, so block-like forms used for their value are hoisted to
//! a nested helper `def` and called (the analogue of codegen-lua's IIFE).
//!
//! A struct lowers to a class carrying its fields and methods; `self` is the
//! receiver, so `p.method(args)` is an ordinary Python method call. An enum
//! value is an instance of the runtime [`_Enum`](RUNTIME) class tagged with its
//! variant; a method-carrying enum gets a subclass holding the methods.
//! Enum/struct construction and `match` consult an [`EnumIndex`] built from the
//! module's `enum` declarations (plus the `Result`/`Option` prelude) to tell an
//! enum variant apart from an ordinary value or call.

mod derive;
mod expr;
mod pat;

use std::collections::{HashMap, HashSet};

use neoc_ast as ast;

use crate::expr::VariantShape;

/// Enum declarations indexed for use-site resolution: `enum name → (variant
/// name → payload shape)`. Built from the module's `enum` items and seeded
/// with the `Result`/`Option` prelude.
#[derive(Default)]
pub(crate) struct EnumIndex {
    enums: HashMap<String, HashMap<String, VariantShape>>,
}

impl EnumIndex {
    /// Build the index from a module, seeding the `Result`/`Option` prelude.
    fn build(module: &ast::Module) -> Self {
        let mut enums: HashMap<String, HashMap<String, VariantShape>> = HashMap::new();
        enums.insert(
            "Result".to_string(),
            [
                ("Ok".to_string(), VariantShape::Tuple),
                ("Err".to_string(), VariantShape::Tuple),
            ]
            .into_iter()
            .collect(),
        );
        enums.insert(
            "Option".to_string(),
            [
                ("Some".to_string(), VariantShape::Tuple),
                ("None".to_string(), VariantShape::Unit),
            ]
            .into_iter()
            .collect(),
        );
        for item in &module.items {
            if let ast::ItemKind::Enum(e) = &item.kind {
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        let shape = match v.body {
                            ast::VariantBody::Unit => VariantShape::Unit,
                            ast::VariantBody::Tuple(_) => VariantShape::Tuple,
                            ast::VariantBody::Named(_) => VariantShape::Named,
                        };
                        (v.name.clone(), shape)
                    })
                    .collect();
                enums.insert(e.name.clone(), variants);
            }
        }
        Self { enums }
    }

    /// Resolve an unqualified variant name (`Ok`, `Err`, `Some`, `None`) to its
    /// enum and shape. Only the prelude enums are in scope unqualified; user
    /// enum variants are always written `Enum::Variant`, so they are not
    /// searched here (which also avoids cross-enum name clashes).
    fn unqualified_variant(&self, name: &str) -> Option<(String, VariantShape)> {
        ["Result", "Option"].into_iter().find_map(|en| {
            self.enums
                .get(en)
                .and_then(|m| m.get(name))
                .map(|shape| (en.to_string(), *shape))
        })
    }

    /// The payload shape of `Enum::Variant`, or `None` if `enum_name` is not a
    /// known enum with a variant named `variant`.
    fn variant_shape(&self, enum_name: &str, variant: &str) -> Option<VariantShape> {
        self.enums.get(enum_name)?.get(variant).copied()
    }
}

/// Tuple/newtype struct names, indexed for use-site resolution. A call whose
/// callee is one of these names is a constructor (`N(x)` → `N.new(x)`), not an
/// ordinary function call. Built from the module's `struct` items with a
/// [`StructBody::Tuple`](ast::StructBody::Tuple) body.
#[derive(Default)]
pub(crate) struct TupleStructIndex {
    names: HashSet<String>,
}

impl TupleStructIndex {
    /// Build the index from a module's tuple-struct declarations.
    fn build(module: &ast::Module) -> Self {
        let names = module
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ast::ItemKind::Struct(s) if matches!(s.body, ast::StructBody::Tuple(_)) => {
                    Some(s.name.clone())
                }
                _ => None,
            })
            .collect();
        Self { names }
    }

    /// Whether `name` is a tuple/newtype struct (so calling it constructs).
    fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// A problem found while emitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
}

/// The result of emitting a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitResult {
    pub py: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower a module to Python 3 source.
#[must_use]
pub fn emit(module: &ast::Module) -> EmitResult {
    let mut e = Emitter {
        enums: EnumIndex::build(module),
        tuple_structs: TupleStructIndex::build(module),
        ..Emitter::default()
    };
    e.emit_module(module);
    let diagnostics = std::mem::take(&mut e.diagnostics);
    EmitResult {
        py: e.finish(),
        diagnostics,
    }
}

/// Accumulates emitted Python and diagnostics. Output is built line by line at
/// the current indentation level.
#[derive(Default)]
pub(crate) struct Emitter {
    lines: Vec<String>,
    indent: usize,
    diagnostics: Vec<Diagnostic>,
    uses_runtime: bool,
    /// Enum declarations, used to resolve enum-variant paths and to tell a
    /// variant constructor apart from an ordinary call.
    enums: EnumIndex,
    /// Tuple/newtype struct names, used to tell a struct constructor call
    /// (`N(x)` → `N.new(x)`) apart from an ordinary function call.
    tuple_structs: TupleStructIndex,
    /// Enum names that carry a method table (any `impl` block or `#[derive]`).
    /// Variant construction for these enums builds an instance of the enum's
    /// generated subclass so `value.method()` dispatches.
    method_enums: HashSet<String>,
    /// Counter feeding unique helper-def names for block-like value expressions.
    helper_seq: usize,
    /// Counter feeding unique temporaries for the `?` operator (§4.10).
    try_seq: usize,
    /// Counter feeding unique scrutinee/destructure temporaries.
    tmp_seq: usize,
    /// Native imports from module-bound `ext` blocks, sorted by specifier.
    imports: Vec<(String, Vec<String>)>,
}

/// The Python runtime prelude: the base classes for structs and enums plus the
/// `Result`/`Option` constructors. Emitted once when a module uses any of them.
const RUNTIME: &str = "\
def _fmt(v):
    if isinstance(v, bool):
        return \"true\" if v else \"false\"
    if isinstance(v, float):
        return str(int(v)) if v.is_integer() else repr(v)
    return str(v)


class _Struct:
    pass


class _Enum:
    def __init__(self, tag, *args, **fields):
        self.tag = tag
        for i, a in enumerate(args):
            setattr(self, \"_\" + str(i), a)
        for k, v in fields.items():
            setattr(self, k, v)


def Ok(v):
    return _Enum(\"Ok\", v)


def Err(e):
    return _Enum(\"Err\", e)


def Some(v):
    return _Enum(\"Some\", v)


None_ = _Enum(\"None\")";

impl Emitter {
    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
        });
    }

    /// Push `text` at the current indentation. Multi-line text is re-indented
    /// line by line so nested output stays aligned.
    pub(crate) fn line(&mut self, text: impl AsRef<str>) {
        let pad = "    ".repeat(self.indent);
        for l in text.as_ref().split('\n') {
            if l.is_empty() {
                self.lines.push(String::new());
            } else {
                self.lines.push(format!("{pad}{l}"));
            }
        }
    }

    pub(crate) fn indented<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.indent += 1;
        let r = f(self);
        self.indent -= 1;
        r
    }

    fn finish(mut self) -> String {
        if self.uses_runtime {
            let mut out = vec![RUNTIME.to_string(), String::new(), String::new()];
            out.append(&mut self.lines);
            self.lines = out;
        }
        if !self.imports.is_empty() {
            let mut out: Vec<String> = self
                .imports
                .iter()
                .map(|(spec, names)| format!("from {spec} import {}", names.join(", ")))
                .collect();
            out.push(String::new());
            out.push(String::new());
            out.append(&mut self.lines);
            self.lines = out;
        }
        let mut s = self.lines.join("\n");
        if !s.is_empty() {
            s.push('\n');
        }
        s
    }

    fn emit_module(&mut self, module: &ast::Module) {
        self.collect_method_enums(module);
        self.imports = ast::module_imports(module, ast::Target::Python);
        for item in &module.items {
            self.emit_item(item);
        }
    }

    /// Record every enum that needs a method-carrying subclass: one with a
    /// `#[derive(...)]` or any `impl` block targeting it.
    fn collect_method_enums(&mut self, module: &ast::Module) {
        let enum_names: HashSet<&str> = module
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ast::ItemKind::Enum(e) => Some(e.name.as_str()),
                _ => None,
            })
            .collect();
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::Enum(e) if has_derive(&item.attrs) => {
                    self.method_enums.insert(e.name.clone());
                }
                ast::ItemKind::Impl(b) => {
                    if let ast::Type::Path(p) = &b.ty {
                        if let Some(seg) = p.segments.last() {
                            if enum_names.contains(seg.name.as_str()) {
                                self.method_enums.insert(seg.name.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether `name` is an enum with a method-carrying subclass. Used at
    /// variant construction sites to decide which class to instantiate.
    pub(crate) fn enum_has_methods(&self, name: &str) -> bool {
        self.method_enums.contains(name)
    }

    /// Whether `name` is a tuple/newtype struct, so a call to it constructs a
    /// value through its generated `.new` (§4.4) rather than calling a function.
    pub(crate) fn is_tuple_struct(&self, name: &str) -> bool {
        self.tuple_structs.contains(name)
    }

    fn emit_item(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Fn(f) => self.emit_fn(f),
            ast::ItemKind::Struct(s) => self.emit_struct(s, &item.attrs),
            ast::ItemKind::Const(c) => self.emit_const(c),
            ast::ItemKind::Impl(b) => self.emit_impl(b),
            ast::ItemKind::Use(u) => self.emit_use(u),
            ast::ItemKind::Enum(e) => self.emit_enum(e, &item.attrs),
            // No Python output: aliases erase, ext is provided by the runtime.
            ast::ItemKind::TypeAlias(_)
            | ast::ItemKind::Ext(_)
            | ast::ItemKind::Extern { .. }
            | ast::ItemKind::Trait(_) => {}
        }
    }

    fn emit_use(&mut self, u: &ast::UseDecl) {
        let Some((last, prefix)) = u.path.split_last() else {
            return;
        };
        let binding = u.alias.as_deref().unwrap_or(last);
        let module = prefix.join(".");
        if module.is_empty() {
            self.line(format!("import {last} as {binding}"));
        } else {
            self.line(format!("from {module} import {last} as {binding}"));
        }
    }

    fn emit_const(&mut self, c: &ast::ConstDecl) {
        let value = self.emit_expr(&c.value);
        self.line(format!("{} = {value}", c.name));
    }

    /// Emit a struct as a Python class. A named struct takes keyword fields; a
    /// tuple/newtype struct stores its payload positionally under `_0`,
    /// constructed through `.new`; a unit struct is an empty marker class. The
    /// requested derives attach as methods on the class.
    fn emit_struct(&mut self, s: &ast::StructDecl, attrs: &[ast::Attribute]) {
        self.uses_runtime = true;
        self.line(format!("class {}(_Struct):", s.name));
        self.indented(|e| {
            e.line(format!("_struct = \"{}\"", s.name));
            match &s.body {
                ast::StructBody::Unit => {}
                ast::StructBody::Tuple(_) => {
                    e.line("def __init__(self, value):");
                    e.indented(|e| e.line("self._0 = value"));
                    e.blank();
                    e.line("@staticmethod");
                    e.line("def new(value):");
                    e.indented(|e| e.line(format!("return {}(value)", s.name)));
                }
                ast::StructBody::Named(fields) => {
                    let params: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                    e.line(format!("def __init__(self, {}):", params.join(", ")));
                    e.indented(|e| {
                        for f in fields {
                            e.line(format!("self.{0} = {0}", f.name));
                        }
                    });
                }
            }
            e.emit_struct_derives(s, attrs);
        });
        self.blank();
    }

    /// Emit an enum. A plain enum (no `impl`/`#[derive]`) needs no class — its
    /// variant values are bare runtime [`_Enum`](RUNTIME) instances. A
    /// method-carrying enum gets a subclass holding its derived methods; `impl`
    /// methods land on it via [`emit_impl`](Self::emit_impl).
    fn emit_enum(&mut self, e: &ast::EnumDecl, attrs: &[ast::Attribute]) {
        if !self.enum_has_methods(&e.name) {
            return;
        }
        self.uses_runtime = true;
        self.line(format!("class {}(_Enum):", e.name));
        let has_derive = has_derive(attrs);
        self.indented(|em| {
            if has_derive {
                em.emit_enum_derives(e, attrs);
            } else {
                em.line("pass");
            }
        });
        self.blank();
    }

    fn emit_impl(&mut self, b: &ast::ImplBlock) {
        let ast::Type::Path(p) = &b.ty else {
            self.error("impl target must be a named type");
            return;
        };
        let Some(target) = p.segments.last().map(|s| s.name.clone()) else {
            return;
        };
        for f in &b.items {
            self.emit_method(&target, f);
        }
    }

    /// Emit a free function at module scope.
    fn emit_fn(&mut self, f: &ast::FnDecl) {
        let params = fn_params(&f.sig.params);
        self.line(format!("def {}({}):", f.sig.name, params.join(", ")));
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.blank();
    }

    /// Reopen the class `target` and attach `f` as a method. Each method is
    /// emitted as `Type.name = _name_impl` after a local `def`, so methods in
    /// separate `impl` blocks accumulate without re-declaring the class.
    fn emit_method(&mut self, target: &str, f: &ast::FnDecl) {
        let params = fn_params(&f.sig.params);
        let local = format!("_{target}_{}", f.sig.name);
        self.line(format!("def {local}({}):", params.join(", ")));
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.line(format!("{target}.{} = {local}", f.sig.name));
        self.blank();
    }

    /// Push a blank line, collapsing consecutive blanks so output stays tidy.
    pub(crate) fn blank(&mut self) {
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }
}

/// The Python parameter names for a signature: `self` for the receiver, the
/// declared name otherwise.
fn fn_params(params: &[ast::Param]) -> Vec<String> {
    params
        .iter()
        .map(|p| match p {
            ast::Param::Receiver => "self".to_string(),
            ast::Param::Named { name, .. } => name.clone(),
        })
        .collect()
}

/// The trait names requested by every `#[derive(...)]` in `attrs`, in order.
fn derive_names(attrs: &[ast::Attribute]) -> Vec<String> {
    let mut names = Vec::new();
    for attr in attrs {
        for item in &attr.items {
            if item.name != "derive" {
                continue;
            }
            for arg in &item.args {
                if let ast::AttrArg::Item(inner) = arg {
                    names.push(inner.name.clone());
                }
            }
        }
    }
    names
}

/// Whether `attrs` contains any `#[derive(...)]`.
fn has_derive(attrs: &[ast::Attribute]) -> bool {
    attrs
        .iter()
        .flat_map(|a| &a.items)
        .any(|i| i.name == "derive")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str) -> ast::Field {
        ast::Field {
            attrs: Vec::new(),
            name: name.to_string(),
            ty: ast::Type::Unit,
        }
    }

    fn item(kind: ast::ItemKind, attrs: Vec<ast::Attribute>) -> ast::Item {
        ast::Item {
            attrs,
            vis: ast::Visibility::Private,
            kind,
            span: ast::Span::default(),
        }
    }

    /// Wraps an [`ast::ExprKind`] in an [`ast::Expr`] with a default span, for
    /// the synthetic ASTs these tests build.
    fn e(kind: ast::ExprKind) -> ast::Expr {
        ast::Expr::new(kind, ast::Span::default())
    }

    fn module(items: Vec<ast::Item>) -> ast::Module {
        ast::Module { items }
    }

    /// `#[derive(X)]`.
    fn derive_attr(name: &str) -> ast::Attribute {
        ast::Attribute {
            items: vec![ast::AttrItem {
                name: "derive".to_string(),
                args: vec![ast::AttrArg::Item(ast::AttrItem {
                    name: name.to_string(),
                    args: Vec::new(),
                })],
            }],
        }
    }

    /// Emit a single hand-built item and return the Python (diagnostics must be
    /// empty).
    fn emit_item(it: ast::Item) -> String {
        let r = emit(&module(vec![it]));
        assert!(r.diagnostics.is_empty(), "diagnostics: {:?}", r.diagnostics);
        r.py
    }

    #[test]
    fn named_struct_emits_class_with_init() {
        let s = ast::StructDecl {
            name: "Point".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("x"), field("y")]),
        };
        let py = emit_item(item(ast::ItemKind::Struct(s), Vec::new()));
        assert!(py.contains("class Point(_Struct):"), "{py}");
        assert!(py.contains("_struct = \"Point\""), "{py}");
        assert!(py.contains("def __init__(self, x, y):"), "{py}");
        assert!(py.contains("self.x = x"), "{py}");
        assert!(py.contains("self.y = y"), "{py}");
    }

    #[test]
    fn enum_with_match_lowers_to_if_chain() {
        // enum Dir { N, S } with `impl Dir { fn pick(self) -> number { match
        // self { Dir::N => 1, Dir::S => 2 } } }`.
        let dir = ast::EnumDecl {
            name: "Dir".to_string(),
            generics: Vec::new(),
            variants: vec![
                ast::Variant {
                    attrs: Vec::new(),
                    name: "N".to_string(),
                    body: ast::VariantBody::Unit,
                },
                ast::Variant {
                    attrs: Vec::new(),
                    name: "S".to_string(),
                    body: ast::VariantBody::Unit,
                },
            ],
        };
        let arm = |variant: &str, value: &str| ast::MatchArm {
            pattern: ast::Pattern::Path(vec!["Dir".to_string(), variant.to_string()]),
            guard: None,
            body: e(ast::ExprKind::Int(value.to_string())),
        };
        let body = ast::Block {
            stmts: Vec::new(),
            value: Some(Box::new(e(ast::ExprKind::Match {
                scrutinee: Box::new(e(ast::ExprKind::Path(vec!["self".to_string()]))),
                arms: vec![arm("N", "1"), arm("S", "2")],
            }))),
        };
        let method = ast::FnDecl {
            attrs: Vec::new(),
            sig: ast::FnSig {
                name: "pick".to_string(),
                generics: Vec::new(),
                params: vec![ast::Param::Receiver],
                ret: None,
                where_clause: Vec::new(),
            },
            body,
        };
        let impl_block = ast::ImplBlock {
            generics: Vec::new(),
            trait_: None,
            ty: ast::Type::Path(ast::TypePath {
                segments: vec![ast::TypeSegment {
                    name: "Dir".to_string(),
                    args: Vec::new(),
                }],
            }),
            items: vec![method],
        };
        let r = emit(&module(vec![
            item(ast::ItemKind::Enum(dir), Vec::new()),
            item(ast::ItemKind::Impl(impl_block), Vec::new()),
        ]));
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        let py = r.py;
        // The method-carrying enum gets a subclass.
        assert!(py.contains("class Dir(_Enum):"), "{py}");
        // `match` lowers to a tag-tested `if` chain (no `match` statement).
        assert!(py.contains("if __t0.tag == \"N\":"), "{py}");
        assert!(py.contains("if __t0.tag == \"S\":"), "{py}");
        assert!(!py.contains("match "), "no match statement: {py}");
    }

    #[test]
    fn derive_debug_renders_braced_fields_via_fmt() {
        let s = ast::StructDecl {
            name: "Point".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("x"), field("y")]),
        };
        let py = emit_item(item(ast::ItemKind::Struct(s), vec![derive_attr("Debug")]));
        assert!(py.contains("def debug(self):"), "{py}");
        assert!(
            py.contains(
                "return \"Point { \" + \"x = \" + _fmt(self.x) + \", \" + \"y = \" + _fmt(self.y) + \" }\""
            ),
            "{py}"
        );
    }

    #[test]
    fn derive_eq_compares_fields_structurally() {
        let s = ast::StructDecl {
            name: "Pair".to_string(),
            generics: Vec::new(),
            body: ast::StructBody::Named(vec![field("a"), field("b")]),
        };
        let py = emit_item(item(ast::ItemKind::Struct(s), vec![derive_attr("Eq")]));
        assert!(py.contains("def eq(self, other):"), "{py}");
        assert!(
            py.contains(
                "return self._struct == other._struct and self.a == other.a and self.b == other.b"
            ),
            "{py}"
        );
    }
}
