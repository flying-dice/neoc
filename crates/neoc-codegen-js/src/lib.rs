//! AST → JavaScript (ES2022) emitter. Mirrors `neoc-codegen-lua`'s structure.
//!
//! Targets ES2022 — the language level supported across all current Node.js LTS
//! lines (18/20/22) and evergreen browsers. Output runs directly under `node`.
//!
//! Split by construct family: items here, expressions in [`expr`], patterns in
//! [`pat`]. neoc is expression-oriented and JavaScript is statement-oriented, so
//! block-like expressions used for their value become immediately-invoked arrow
//! functions (IIFEs).
//!
//! # Runtime representation
//!
//! - **struct** → a plain object carrying a `_struct` tag and its fields. A
//!   newtype/tuple struct stores its payload positionally under integer keys
//!   (`_0`, `_1`, …). Named structs store fields by name.
//! - **enum** → a tagged object `{ _enum, tag, … }`. Tuple payload is stored
//!   under `_0`, `_1`, …; struct-variant payload by field name. The
//!   `Result`/`Option` prelude follows the same shape.
//! - **methods** → every type with an `impl` or `#[derive]` gets an entry in the
//!   global method table `__m` (`__m.Point.manhattan = (self) => …`). A method
//!   call `x.m(a)` dispatches through `__call(x, "m", a)`, which reads the
//!   receiver's `_struct`/`_enum` tag and invokes the stored function with the
//!   receiver as the first argument — the JS analogue of Lua metatable dispatch.
//! - **`?`** → a hoisted guard: `e?` binds `e` to a temp, early-returns the
//!   carrier from the enclosing function on `Err`/`None`, and yields the payload
//!   otherwise.
//! - **derives** — `clone` (shallow copy carrying the tag), `eq` (structural,
//!   returns bool), `debug` / `display` (return the golden strings).

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
    /// enum and shape. Only the prelude enums are in scope unqualified.
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
    pub js: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower a module to JavaScript (ES2022) source.
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
        js: e.finish(),
        diagnostics,
    }
}

/// Accumulates emitted JavaScript and diagnostics. Output is built line by line
/// at the current indentation level.
#[derive(Default)]
pub(crate) struct Emitter {
    lines: Vec<String>,
    indent: usize,
    diagnostics: Vec<Diagnostic>,
    uses_result: bool,
    /// Whether any method dispatch (`__call`) or registry (`__m`) is needed.
    uses_methods: bool,
    /// Enum declarations, used to resolve enum-variant paths and to tell a
    /// variant constructor apart from an ordinary call.
    enums: EnumIndex,
    /// Tuple/newtype struct names, used to tell a struct constructor call
    /// (`N(x)` → `N.new(x)`) apart from an ordinary function call.
    tuple_structs: TupleStructIndex,
    /// Types (struct/enum names) that carry a method-table entry — any `impl`
    /// block or `#[derive]`.
    method_types: HashSet<String>,
    /// Counter feeding unique temporaries for the `?` operator.
    try_seq: usize,
    /// Counter feeding unique temporaries for destructuring / match scrutinees.
    tmp_seq: usize,
    /// Native imports from module-bound `ext` blocks, sorted by specifier.
    imports: Vec<(String, Vec<String>)>,
}

/// The `Result`/`Option` prelude: variant constructors as plain tagged objects.
const RESULT_PRELUDE: &str = "\
const Ok = (v) => ({ _enum: \"Result\", tag: \"Ok\", _0: v });
const Err = (e) => ({ _enum: \"Result\", tag: \"Err\", _0: e });
const Some = (v) => ({ _enum: \"Option\", tag: \"Some\", _0: v });
const None = { _enum: \"Option\", tag: \"None\" };";

/// The method-dispatch runtime: a registry keyed by type name, and `__call`,
/// which reads the receiver's tag and invokes the stored function with the
/// receiver first — the JS analogue of Lua metatable dispatch.
const METHOD_PRELUDE: &str = "\
const __m = {};
const __call = (recv, name, ...args) => __m[recv._struct ?? recv._enum][name](recv, ...args);";

impl Emitter {
    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            message: message.into(),
        });
    }

    /// Push `text` at the current indentation. Multi-line text (e.g. an IIFE
    /// value) is re-indented line by line so nested output stays aligned.
    pub(crate) fn line(&mut self, text: impl AsRef<str>) {
        let pad = "  ".repeat(self.indent);
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
        let mut preamble = Vec::new();
        for (spec, names) in &self.imports {
            preamble.push(format!(
                "import {{ {} }} from \"{spec}\";",
                names.join(", ")
            ));
        }
        if self.uses_methods {
            preamble.push(METHOD_PRELUDE.to_string());
        }
        if self.uses_result {
            preamble.push(RESULT_PRELUDE.to_string());
        }
        if !preamble.is_empty() {
            let mut out = preamble;
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
        self.collect_method_types(module);
        self.imports = ast::module_imports(module, ast::Target::Js);
        for item in &module.items {
            self.emit_item(item);
        }
    }

    /// Record every type that needs a method-table entry: a struct/enum with a
    /// `#[derive(...)]` or any `impl` block targeting it.
    fn collect_method_types(&mut self, module: &ast::Module) {
        let type_names: HashSet<&str> = module
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ast::ItemKind::Struct(s) => Some(s.name.as_str()),
                ast::ItemKind::Enum(e) => Some(e.name.as_str()),
                _ => None,
            })
            .collect();
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::Struct(s) if has_derive(&item.attrs) => {
                    self.method_types.insert(s.name.clone());
                }
                ast::ItemKind::Enum(e) if has_derive(&item.attrs) => {
                    self.method_types.insert(e.name.clone());
                }
                ast::ItemKind::Impl(b) => {
                    if let ast::Type::Path(p) = &b.ty {
                        if let Some(seg) = p.segments.last() {
                            if type_names.contains(seg.name.as_str()) {
                                self.method_types.insert(seg.name.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether `name` is a type with a method-table entry. Used to decide when
    /// to register the `__m[name] = {}` slot and to gate dispatch.
    pub(crate) fn type_has_methods(&self, name: &str) -> bool {
        self.method_types.contains(name)
    }

    /// Whether `name` is a tuple/newtype struct, so a call to it constructs a
    /// value through its generated `.new` rather than calling a function.
    pub(crate) fn is_tuple_struct(&self, name: &str) -> bool {
        self.tuple_structs.contains(name)
    }

    /// A fresh unique temporary name with the given prefix.
    pub(crate) fn fresh_tmp(&mut self, prefix: &str) -> String {
        self.tmp_seq += 1;
        format!("{prefix}{}", self.tmp_seq)
    }

    fn emit_item(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Fn(f) => self.emit_fn(None, f),
            ast::ItemKind::Struct(s) => {
                self.ensure_method_slot(&s.name);
                self.emit_derives(&item.attrs, neoc_ast_derive_target(item));
            }
            ast::ItemKind::Const(c) => self.emit_const(c),
            ast::ItemKind::Impl(b) => self.emit_impl(b),
            ast::ItemKind::Use(u) => self.emit_use(u),
            ast::ItemKind::Enum(e) => {
                self.ensure_method_slot(&e.name);
                self.emit_derives(&item.attrs, neoc_ast_derive_target(item));
            }
            // No JS output: aliases erase, ext is provided by the runtime harness.
            ast::ItemKind::TypeAlias(_)
            | ast::ItemKind::Ext(_)
            | ast::ItemKind::Extern { .. }
            | ast::ItemKind::Trait(_) => {}
        }
    }

    /// Register the `__m[name] = {}` method slot for a type that carries one.
    fn ensure_method_slot(&mut self, name: &str) {
        if self.type_has_methods(name) {
            self.uses_methods = true;
            self.line(format!("__m.{name} = {{}};"));
        }
    }

    /// Emit the methods generated by each `#[derive(...)]` on a type.
    fn emit_derives(&mut self, attrs: &[ast::Attribute], target: DeriveTarget<'_>) {
        for name in derive_names(attrs) {
            self.uses_methods = true;
            self.emit_derive(&name, target);
        }
    }

    fn emit_use(&mut self, u: &ast::UseDecl) {
        let Some((last, prefix)) = u.path.split_last() else {
            return;
        };
        let binding = u.alias.as_deref().unwrap_or(last);
        let module = prefix.join("/");
        self.line(format!("const {binding} = require(\"{module}\").{last};"));
    }

    fn emit_const(&mut self, c: &ast::ConstDecl) {
        let value = self.emit_expr(&c.value);
        self.line(format!("const {} = {value};", c.name));
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
            self.emit_fn(Some(&target), f);
        }
    }

    /// Emit a function. `target` is the type name when this is a method or
    /// associated function inside an impl: it lands on `__m[target]` taking the
    /// receiver as its first parameter.
    fn emit_fn(&mut self, target: Option<&str>, f: &ast::FnDecl) {
        let mut params = Vec::new();
        for p in &f.sig.params {
            match p {
                ast::Param::Receiver => params.push("self".to_string()),
                ast::Param::Named { name, .. } => params.push(name.clone()),
            }
        }
        let param_list = params.join(", ");
        match target {
            Some(t) => {
                self.uses_methods = true;
                self.line(format!(
                    "__m.{t}.{} = function({param_list}) {{",
                    f.sig.name
                ));
            }
            None => self.line(format!("function {}({param_list}) {{", f.sig.name)),
        }
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.line("}");
    }
}

/// What a derive is applied to. Mirrors the structure the emitter walks to
/// produce `clone`/`eq`/`debug`/`display`.
#[derive(Clone, Copy)]
pub(crate) enum DeriveTarget<'a> {
    Struct(&'a ast::StructDecl),
    Enum(&'a ast::EnumDecl),
}

/// Build a [`DeriveTarget`] from an item known to be a struct or enum.
fn neoc_ast_derive_target(item: &ast::Item) -> DeriveTarget<'_> {
    match &item.kind {
        ast::ItemKind::Struct(s) => DeriveTarget::Struct(s),
        ast::ItemKind::Enum(e) => DeriveTarget::Enum(e),
        _ => unreachable!("derive target is always a struct or enum"),
    }
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

    fn field(name: &str) -> ast::Field {
        ast::Field {
            attrs: Vec::new(),
            name: name.to_string(),
            ty: ast::Type::Unit,
        }
    }

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

    /// A `fn name() { <body> }` item whose body is a single trailing expression.
    fn fn_item(name: &str, value: ast::Expr) -> ast::Item {
        item(
            ast::ItemKind::Fn(ast::FnDecl {
                attrs: Vec::new(),
                sig: ast::FnSig {
                    name: name.to_string(),
                    generics: Vec::new(),
                    params: Vec::new(),
                    ret: None,
                    where_clause: Vec::new(),
                },
                body: ast::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(value)),
                },
            }),
            Vec::new(),
        )
    }

    fn int(n: &str) -> ast::Expr {
        e(ast::ExprKind::Int(n.to_string()))
    }

    #[test]
    fn named_struct_literal_and_field_access() {
        // struct Point { x, y }
        // fn make() { Point { x: 1, y: 2 }.x }
        let module = ast::Module {
            items: vec![
                item(
                    ast::ItemKind::Struct(ast::StructDecl {
                        name: "Point".to_string(),
                        generics: Vec::new(),
                        body: ast::StructBody::Named(vec![field("x"), field("y")]),
                    }),
                    Vec::new(),
                ),
                fn_item(
                    "make",
                    e(ast::ExprKind::Field {
                        base: Box::new(e(ast::ExprKind::StructLit {
                            path: vec!["Point".to_string()],
                            fields: vec![
                                ast::FieldInit {
                                    name: "x".to_string(),
                                    value: Some(int("1")),
                                },
                                ast::FieldInit {
                                    name: "y".to_string(),
                                    value: Some(int("2")),
                                },
                            ],
                            base: None,
                        })),
                        field: ast::FieldAccess::Named("x".to_string()),
                    }),
                ),
            ],
        };
        let out = emit(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function make() {\n  \
             return { _struct: \"Point\", x: 1, y: 2 }.x;\n}\n"
        );
    }

    #[test]
    fn enum_match_on_tag() {
        // enum Color { Red, Green }
        // fn pick() { match Color::Red { Color::Red => 1, Color::Green => 2 } }
        let module = ast::Module {
            items: vec![
                item(
                    ast::ItemKind::Enum(ast::EnumDecl {
                        name: "Color".to_string(),
                        generics: Vec::new(),
                        variants: vec![
                            ast::Variant {
                                attrs: Vec::new(),
                                name: "Red".to_string(),
                                body: ast::VariantBody::Unit,
                            },
                            ast::Variant {
                                attrs: Vec::new(),
                                name: "Green".to_string(),
                                body: ast::VariantBody::Unit,
                            },
                        ],
                    }),
                    Vec::new(),
                ),
                fn_item(
                    "pick",
                    e(ast::ExprKind::Match {
                        scrutinee: Box::new(e(ast::ExprKind::Path(vec![
                            "Color".to_string(),
                            "Red".to_string(),
                        ]))),
                        arms: vec![
                            ast::MatchArm {
                                pattern: ast::Pattern::Path(vec![
                                    "Color".to_string(),
                                    "Red".to_string(),
                                ]),
                                guard: None,
                                body: int("1"),
                            },
                            ast::MatchArm {
                                pattern: ast::Pattern::Path(vec![
                                    "Color".to_string(),
                                    "Green".to_string(),
                                ]),
                                guard: None,
                                body: int("2"),
                            },
                        ],
                    }),
                ),
            ],
        };
        let out = emit(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function pick() {\n  \
             return (() => {\n    \
             const __s1 = { _enum: \"Color\", tag: \"Red\" };\n    \
             if (__s1.tag === \"Red\") {\n      \
             return 1;\n    \
             }\n    \
             if (__s1.tag === \"Green\") {\n      \
             return 2;\n    \
             }\n  \
             })();\n}\n"
        );
    }

    #[test]
    fn derive_debug_struct() {
        // #[derive(Debug)] struct Point { x, y }
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Struct(ast::StructDecl {
                    name: "Point".to_string(),
                    generics: Vec::new(),
                    body: ast::StructBody::Named(vec![field("x"), field("y")]),
                }),
                vec![derive_attr("Debug")],
            )],
        };
        let out = emit(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "const __m = {};\n\
             const __call = (recv, name, ...args) => \
             __m[recv._struct ?? recv._enum][name](recv, ...args);\n\
             \n\
             __m.Point = {};\n\
             __m.Point.debug = function(self) {\n  \
             return \"Point { \" + \"x = \" + String(self.x) + \", \" + \
             \"y = \" + String(self.y) + \" }\";\n\
             };\n"
        );
    }
}
