//! AST → idiomatic Python (3.10+) emitter (target #5: python).
//!
//! A from-scratch backend emitting **native Python constructs** — no invented
//! runtime where Python has one of its own:
//!
//! - **struct** → a `@dataclass(frozen=True)` class. Fields are typed; a
//!   newtype/tuple struct stores its payload positionally (`_0`, `_1`, …).
//! - **enum** → one `@dataclass(frozen=True)` class per variant plus a union
//!   alias `E = Union[A, B, …]`. Every value is a native dataclass instance, so
//!   it is matched with Python's structural `match`/`case` class patterns —
//!   `case B(v):`, `case C(x=x):`.
//! - **`Result`/`Option`** → tagged dataclasses `Ok`/`Err`, `Some`/`None_` in a
//!   shared `neoc_lib.py` (Python has no native `Result`), kept `match`-able and
//!   carrying the inherent operations.
//! - **`match`** → Python `match`/`case`: the whole reason for the Python idioms.
//! - **methods** → methods on the dataclass; `self` stays `self`.
//! - **`?`** → a hoisted guard returning the carrier from the enclosing function
//!   on `Err`/`None_`, yielding the payload otherwise.
//!
//! `number` is a double, so it lowers to `float`; `string` → `str`; `()` →
//! `None`; `[T]` → `list[T]`; `fn(A) -> B` → `Callable[[A], B]`. A sibling `.pyi`
//! ([`pyi::module_pyi`]) carries the typed surface.
//!
//! Pure string building over `&ast` nodes; no host facilities, wasm-clean.

mod derive;
mod expr;
mod pat;
mod pyi;

use std::collections::{HashMap, HashSet};

use neoc_ast as ast;

pub use crate::pyi::{module_pyi, py_ident, py_type, variant_class_name};

/// A problem found while emitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
}

/// The result of emitting a module to Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitResult {
    /// The emitted Python 3.10+ module source (`x.py`).
    pub py: String,
    /// The module's typed stub sibling (`x.pyi`): a `@dataclass` header per
    /// `struct`/enum variant, a `def … -> R: ...` per `pub` fn, and a typed name
    /// per `pub const`. Empty when the module declares no surface.
    pub pyi: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// The runtime payload shape of an enum variant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariantShape {
    Unit,
    Tuple,
    Named,
}

/// Enum declarations indexed for use-site resolution: `enum name → (variant
/// name → shape)`. Seeded with the `Result`/`Option` prelude so `Ok`/`Some`/…
/// resolve unqualified.
#[derive(Default)]
pub(crate) struct EnumIndex {
    enums: HashMap<String, HashMap<String, VariantShape>>,
}

impl EnumIndex {
    fn build(module: &ast::Module, imported: &[&ast::EnumDecl]) -> Self {
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
        let local = module.items.iter().filter_map(|item| match &item.kind {
            ast::ItemKind::Enum(e) => Some(e),
            _ => None,
        });
        for e in local.chain(imported.iter().copied()) {
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
            enums.entry(e.name.clone()).or_insert(variants);
        }
        Self { enums }
    }

    fn unqualified_variant(&self, name: &str) -> Option<(String, VariantShape)> {
        ["Result", "Option"].into_iter().find_map(|en| {
            self.enums
                .get(en)
                .and_then(|m| m.get(name))
                .map(|shape| (en.to_string(), *shape))
        })
    }

    fn variant_shape(&self, enum_name: &str, variant: &str) -> Option<VariantShape> {
        self.enums.get(enum_name)?.get(variant).copied()
    }
}

/// The `pub` names in `module` that emit a runtime Python definition — a `fn`,
/// `const`, `struct`, `enum` (data-only types still emit a `@dataclass`), or
/// `trait` (a `Protocol`). A `type` alias produces no Python symbol, so a `use`
/// of one must not emit a dangling `import`; this set tells the emitter which
/// imports to keep. Target-agnostic; non-Python backends ignore it.
#[must_use]
pub fn module_py_bindings(module: &ast::Module) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for item in &module.items {
        if item.vis != ast::Visibility::Public {
            continue;
        }
        match &item.kind {
            ast::ItemKind::Fn(f) => {
                bindings.insert(f.sig.name.clone());
            }
            ast::ItemKind::Const(c) => {
                bindings.insert(c.name.clone());
            }
            ast::ItemKind::Struct(s) => {
                bindings.insert(s.name.clone());
            }
            ast::ItemKind::Enum(e) => {
                bindings.insert(e.name.clone());
            }
            ast::ItemKind::Trait(t) => {
                bindings.insert(t.name.clone());
            }
            _ => {}
        }
    }
    bindings
}

/// Lower a module to idiomatic Python (3.10+) source plus its `.pyi` sibling.
///
/// `imported_enums` carries the `pub` enums a cross-module `use` brought in, so
/// variant construction and `match` on them resolve at use sites exactly like a
/// local enum. Pass an empty slice for a single-file emit.
///
/// `imported_bindings` carries the resolved `use` paths (`a::b::c`) that resolve
/// to a runtime Python definition (see [`module_py_bindings`]); a `use` whose
/// path is absent resolves to a trait or alias and emits no import. Pass an
/// empty slice for a single-file emit — it declares no `use`, so the set is
/// never consulted.
///
/// `importer_dir` is this module's directory as root-relative path segments
/// (`[]` for a top-level module, `["autosql"]` for `autosql/ports.neoc`), used
/// to resolve a `use` to a relative package import. Pass an empty slice for a
/// single-file emit.
#[must_use]
pub fn emit(
    module: &ast::Module,
    imported_enums: &[&ast::EnumDecl],
    importer_dir: &[String],
    imported_bindings: &[String],
) -> EmitResult {
    let imported_enum_variants = imported_enums
        .iter()
        .map(|e| {
            let variants = e.variants.iter().map(|v| variant_class_name(&v.name));
            (e.name.clone(), variants.collect())
        })
        .collect();
    let mut e = Emitter {
        enums: EnumIndex::build(module, imported_enums),
        method_types: method_type_names(module),
        importer_dir: importer_dir.to_vec(),
        imported_enum_variants,
        imported_bindings: imported_bindings.iter().cloned().collect(),
        escaped_values: escaped_value_names(module),
        ..Emitter::default()
    };
    e.emit_module(module);
    let diagnostics = std::mem::take(&mut e.diagnostics);
    let pyi = pyi::module_pyi(module, importer_dir, &e.imported_bindings);
    EmitResult {
        py: e.finish(),
        pyi,
        diagnostics,
    }
}

/// The shared `Result`/`Option` runtime, `neoc_lib.py`. Emitted once per build;
/// every module that references `Result`/`Option` imports its constructors from
/// here. Python has no native `Result`, so these tagged dataclasses are the
/// justified exception to "native constructs only" — and they stay `match`-able
/// (a `case Ok(v):` works because each is a real dataclass). The driver writes
/// the returned source to `neoc_lib.py`.
#[must_use]
pub fn neoc_lib_py() -> String {
    NEOC_LIB_PY.to_string()
}

/// The `.pyi` for the shared `neoc_lib.py`: the typed `Result`/`Option` surface
/// a consumer's stub imports. The driver writes this to `neoc_lib.pyi`.
#[must_use]
pub fn neoc_lib_pyi() -> String {
    NEOC_LIB_PYI.to_string()
}

/// Accumulates emitted Python and diagnostics, line by line at the current
/// indentation (Python is whitespace-significant, so the indent is the syntax).
// The several bool fields are independent emit-state flags (each gates one
// preamble import or feature); a flags struct would only obscure them.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub(crate) struct Emitter {
    lines: Vec<String>,
    indent: usize,
    diagnostics: Vec<Diagnostic>,
    uses_result: bool,
    /// Whether `Callable` was referenced in an emitted annotation.
    uses_callable: bool,
    /// Whether the module declares any struct/enum (so `@dataclass` is imported).
    has_dataclass: bool,
    /// Whether the module declares any enum (so `Union` is imported for aliases).
    uses_union: bool,
    /// Whether any declared type carries generic parameters (so `Generic`,
    /// `TypeVar`, and the `TypeVar` declarations land in the preamble).
    uses_generic: bool,
    /// Whether a generic `fn` needs `TypeVar` imported without any `Generic`
    /// use (a free/`impl` fn declares type params but no class carries them).
    needs_typevar: bool,
    /// Whether the module declares a `trait` (emitted as a `Protocol`).
    uses_protocol: bool,
    /// The distinct generic-parameter names declared across the module, each
    /// emitted as a module-level `TypeVar`.
    type_vars: Vec<String>,
    enums: EnumIndex,
    /// Struct/enum names carrying methods (an `impl` or a `#[derive]`). Only
    /// consulted to pick the right `.pyi` shape; the `.py` always emits a class.
    method_types: HashSet<String>,
    /// `impl` methods collected up-front, keyed by their target type name, so a
    /// type's methods fold into its `@dataclass` body when it is emitted (an
    /// `impl` reached at emit time produces no standalone output).
    pending_methods: Vec<(String, ast::FnDecl)>,
    /// Whether a struct functional-update used `dataclasses.replace` (so it is
    /// imported in the preamble).
    uses_replace: bool,
    /// The public surface, accumulated for `__all__`.
    public: Vec<String>,
    try_seq: usize,
    tmp_seq: usize,
    /// This module's directory, as root-relative path segments (`[]` for a
    /// top-level module, `["autosql"]` for `autosql/ports.neoc`). A `use`
    /// resolves its target module relative to this, so the emitted relative
    /// `from . import …` lands on the right file from a subpackage.
    importer_dir: Vec<String>,
    /// Variant class names per `pub` enum brought in by `use`
    /// (`enum name → [variant class, …]`). A `use` of one of these enums also
    /// imports its variant classes, since variant construction emits the bare
    /// variant class (e.g. `Text(body)`), not a namespaced reference.
    imported_enum_variants: HashMap<String, Vec<String>>,
    /// Resolved `use` paths (`a::b::c`) that resolve to a runtime Python
    /// definition. A `use` whose path is absent names a trait or alias (no
    /// Python symbol), so its import is skipped to avoid a dangling reference.
    /// Empty means "import every `use`" (a single-file emit declares no `use`).
    imported_bindings: HashSet<String>,
    /// Module-level value names whose spelling collides with a Python
    /// builtin/keyword and so were escaped at their definition or import
    /// (`list` → `list_`). A bare value-path reference to one of these emits the
    /// escaped form so the call site names the real binding. A **local** of the
    /// same spelling is never in this set — locals are not escaped, so they emit
    /// verbatim. Single-sourced through [`py_ident`]: a name is recorded only
    /// when `py_ident(name) != name`.
    escaped_values: HashSet<String>,
}

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

    /// Run `f`, capturing the lines it emits into a separate buffer (used to
    /// build nested suites). Indentation resets to zero inside.
    pub(crate) fn render<F: FnOnce(&mut Self)>(&mut self, f: F) -> Vec<String> {
        let saved_lines = std::mem::take(&mut self.lines);
        let saved_indent = std::mem::replace(&mut self.indent, 0);
        f(self);
        let captured = std::mem::replace(&mut self.lines, saved_lines);
        self.indent = saved_indent;
        captured
    }

    /// A fresh unique temporary name with the given prefix.
    pub(crate) fn fresh_tmp(&mut self, prefix: &str) -> String {
        self.tmp_seq += 1;
        format!("{prefix}{}", self.tmp_seq)
    }

    /// The number of lines emitted so far in the current buffer — used to tell
    /// whether a suite emitted any statements (so an empty one gets `pass`).
    pub(crate) fn lines_len(&self) -> usize {
        self.lines.len()
    }

    fn finish(mut self) -> String {
        let mut preamble: Vec<String> = vec!["from __future__ import annotations".to_string()];
        if self.has_dataclass || self.uses_replace {
            let imp = if self.uses_replace && self.has_dataclass {
                "from dataclasses import dataclass, replace"
            } else if self.uses_replace {
                "from dataclasses import replace"
            } else {
                "from dataclasses import dataclass"
            };
            preamble.push(imp.to_string());
        }
        let mut typing: Vec<&str> = Vec::new();
        if self.uses_union {
            typing.push("Union");
        }
        if self.uses_callable {
            typing.push("Callable");
        }
        if self.uses_generic {
            typing.push("Generic");
            typing.push("TypeVar");
        }
        if self.needs_typevar {
            typing.push("TypeVar");
        }
        if self.uses_protocol {
            typing.push("Protocol");
        }
        typing.sort_unstable();
        typing.dedup();
        if !typing.is_empty() {
            preamble.push(format!("from typing import {}", typing.join(", ")));
        }
        if self.uses_result {
            // `neoc_lib` sits at the output root, so a module nested in a
            // subpackage reaches it through that many extra parent dots
            // (`autosql/x.py` → `from ..neoc_lib import …`).
            let dots = ".".repeat(1 + self.importer_dir.len());
            preamble.push(format!(
                "from {dots}neoc_lib import Result, Option, Ok, Err, Some, None_"
            ));
        }

        let mut out = preamble;
        out.push(String::new());
        // Module-level `TypeVar` declarations for every generic parameter.
        if !self.type_vars.is_empty() {
            for tv in &self.type_vars {
                out.push(format!("{tv} = TypeVar(\"{tv}\")"));
            }
            out.push(String::new());
        }
        out.append(&mut self.lines);

        if !self.public.is_empty() {
            out.push(String::new());
            let names: Vec<String> = self.public.iter().map(|n| format!("\"{n}\"")).collect();
            out.push(format!("__all__ = [{}]", names.join(", ")));
        }

        let mut s = out.join("\n");
        if !s.is_empty() {
            s.push('\n');
        }
        s
    }

    fn emit_module(&mut self, module: &ast::Module) {
        // Pre-scan determines the import preamble.
        self.has_dataclass = module
            .items
            .iter()
            .any(|i| matches!(i.kind, ast::ItemKind::Struct(_) | ast::ItemKind::Enum(_)));
        self.uses_union = module
            .items
            .iter()
            .any(|i| matches!(i.kind, ast::ItemKind::Enum(_)));
        if pyi::references_lib(module) {
            self.uses_result = true;
        }
        // Distinct generic-parameter names → module-level `TypeVar`s. A
        // type's own generics carry `Generic[...]` on its class; a generic `fn`
        // (`init<E>(db: E)`) only needs the `TypeVar` so its annotations name a
        // declared symbol.
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::Struct(s) => self.note_generics(&s.generics, true),
                ast::ItemKind::Enum(e) => self.note_generics(&e.generics, true),
                ast::ItemKind::Fn(f) => self.note_generics(&f.sig.generics, false),
                ast::ItemKind::Impl(b) => {
                    for f in &b.items {
                        self.note_generics(&f.sig.generics, false);
                    }
                }
                _ => {}
            }
        }
        // Collect `impl` methods up-front, keyed by target type, so they fold
        // into the type's `@dataclass` body regardless of source order.
        for item in &module.items {
            if let ast::ItemKind::Impl(b) = &item.kind
                && let ast::Type::Path(p) = &b.ty
                && let Some(seg) = p.segments.last()
            {
                for f in &b.items {
                    self.pending_methods.push((seg.name.clone(), f.clone()));
                }
            }
        }
        for item in &module.items {
            self.emit_item(item);
        }
    }

    fn emit_item(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Fn(f) => self.emit_fn(None, f, item.vis),
            ast::ItemKind::Struct(s) => self.emit_struct(item, s),
            ast::ItemKind::Enum(e) => self.emit_enum(item, e),
            ast::ItemKind::Const(c) => self.emit_const(c, item.vis),
            ast::ItemKind::Impl(b) => self.emit_impl(b),
            ast::ItemKind::Use(u) => self.emit_use(u),
            ast::ItemKind::Trait(t) => self.emit_trait(item, t),
            // Aliases erase; an `extern` block describes the target surface (its
            // references lower directly).
            ast::ItemKind::TypeAlias(_) | ast::ItemKind::Extern(_) => {}
        }
    }

    fn record_public(&mut self, vis: ast::Visibility, name: &str) {
        if vis == ast::Visibility::Public {
            // `__all__` lists the emitted binding name (escaped if it collides
            // with a Python builtin/keyword).
            self.public.push(py_ident(name));
        }
    }

    /// Emit a `struct` as a `@dataclass(frozen=True)` class. Named fields are
    /// typed attributes; a newtype/tuple struct stores its payload as `_0`,
    /// `_1`, …. Inherent and trait methods (from `impl`, §3.3) and derives
    /// (§6) land in the class body — collected so the class is emitted whole.
    fn emit_struct(&mut self, item: &ast::Item, s: &ast::StructDecl) {
        self.record_public(item.vis, &s.name);
        self.line("@dataclass(frozen=True)");
        self.line(class_header(&s.name, &s.generics));
        let mut wrote = false;
        self.indented(|e| {
            match &s.body {
                ast::StructBody::Unit => {}
                ast::StructBody::Tuple(t) => {
                    e.line(format!("_0: {}", py_type(t)));
                    wrote = true;
                }
                ast::StructBody::Named(fields) => {
                    for f in fields {
                        e.line(format!("{}: {}", f.name, py_type(&f.ty)));
                        wrote = true;
                    }
                }
            }
            // Methods + derives in the same class body.
            wrote |= e.emit_type_members(&item.attrs, DeriveTarget::Struct(s), &s.name);
            if !wrote {
                e.line("pass");
            }
        });
        self.line("");
    }

    /// Emit an `enum` as one `@dataclass(frozen=True)` class per variant plus the
    /// union alias `E = Union[A, B, …]`. Methods and derives land on **every**
    /// variant class, so any value matched out of the union carries the
    /// behaviour and the structural `match` works against the variant classes.
    fn emit_enum(&mut self, item: &ast::Item, en: &ast::EnumDecl) {
        self.record_public(item.vis, &en.name);
        let mut variant_classes: Vec<String> = Vec::new();
        for v in &en.variants {
            let cls = variant_class_name(&v.name);
            variant_classes.push(cls.clone());
            self.line("@dataclass(frozen=True)");
            self.line(format!("class {cls}:"));
            let mut wrote = false;
            self.indented(|e| {
                match &v.body {
                    ast::VariantBody::Unit => {}
                    ast::VariantBody::Tuple(tys) => {
                        for (i, t) in tys.iter().enumerate() {
                            e.line(format!("_{i}: {}", py_type(t)));
                            wrote = true;
                        }
                    }
                    ast::VariantBody::Named(fields) => {
                        for f in fields {
                            e.line(format!("{}: {}", f.name, py_type(&f.ty)));
                            wrote = true;
                        }
                    }
                }
                wrote |= e.emit_type_members(&item.attrs, DeriveTarget::Enum(en), &cls);
                if !wrote {
                    e.line("pass");
                }
            });
            self.line("");
        }
        self.line(format!(
            "{} = Union[{}]",
            py_ident(&en.name),
            variant_classes.join(", ")
        ));
        self.line("");
    }

    /// Emit a type's methods (from any matching `impl`) and derives into the
    /// current class body. Methods are filled in `emit_impl` deferral — but a
    /// dataclass body is closed by the time `impl` is reached, so instead this
    /// pulls the impl methods that target `type_name` from the module. Returns
    /// whether anything was written.
    ///
    /// Derives are emitted against `self_class` — the variant class name for an
    /// enum (each variant gets the derived method) or the struct name.
    fn emit_type_members(
        &mut self,
        attrs: &[ast::Attribute],
        target: DeriveTarget<'_>,
        self_class: &str,
    ) -> bool {
        let mut wrote = false;
        // Inherent / trait methods targeting this type.
        let type_name = match target {
            DeriveTarget::Struct(s) => s.name.clone(),
            DeriveTarget::Enum(e) => e.name.clone(),
        };
        let methods: Vec<ast::FnDecl> = self
            .pending_methods
            .iter()
            .filter(|(ty, _)| *ty == type_name)
            .map(|(_, decl)| decl.clone())
            .collect();
        for decl in &methods {
            self.emit_method(decl);
            wrote = true;
        }
        // Derives.
        for name in ast::derive_names(attrs) {
            self.emit_derive(&name, target, self_class);
            wrote = true;
        }
        wrote
    }

    /// An `impl` produces no standalone output: its methods are collected
    /// up-front (`emit_module`) and folded into the target type's `@dataclass`
    /// body when the type is emitted (`lang/DYNAMIC.md` §3.3). An associated
    /// function (no `self`) becomes a `@staticmethod` in that body.
    fn emit_impl(&mut self, b: &ast::ImplBlock) {
        if !matches!(&b.ty, ast::Type::Path(_)) {
            self.error("impl target must be a named type");
        }
    }

    fn emit_const(&mut self, c: &ast::ConstDecl, vis: ast::Visibility) {
        self.record_public(vis, &c.name);
        let value = self.emit_expr(&c.value);
        self.line(format!("{}: {} = {value}", c.name, py_type(&c.ty)));
    }

    fn emit_use(&mut self, u: &ast::UseDecl) {
        let Some((last, prefix)) = u.path.split_last() else {
            return;
        };
        if prefix.is_empty() {
            // A single-segment `use` names something already in scope.
            return;
        }
        // A `use` of a trait or `type` alias resolves to no Python symbol, so
        // importing its name would dangle. The driver records which resolved
        // `use` paths have a runtime definition; skip the rest. (A single-file
        // emit declares no `use`, so this path is never reached there.)
        if !self.imported_bindings.contains(&u.path.join("::")) {
            return;
        }
        // `use a::b::c` → a relative package import of `c` from the module
        // `a/b`, resolved against *this* module's directory. The prefix is the
        // target module's path segments (`a::b` → `a/b.py`); a leading-dot
        // count of `1 + ups` walks up out of any shared subpackage so the
        // import lands on the right file from a subdirectory.
        let target: Vec<&str> = prefix.iter().map(String::as_str).collect();
        let module = self.relative_module(&target);
        let import_name = py_ident(last);
        match &u.alias {
            Some(alias) => self.line(format!(
                "from {module} import {import_name} as {}",
                py_ident(alias)
            )),
            None => self.line(format!("from {module} import {import_name}")),
        }
        // A `use` of a `pub` enum also imports its variant classes: variant
        // construction emits the bare variant class (`SqlValue::Text(x)` →
        // `Text(x)`), so each variant must be in scope, not just the union alias.
        if let Some(variants) = self.imported_enum_variants.get(last.as_str()) {
            let imports = variants
                .iter()
                .filter(|v| v.as_str() != last.as_str())
                .cloned()
                .collect::<Vec<_>>();
            if !imports.is_empty() {
                self.line(format!("from {module} import {}", imports.join(", ")));
            }
        }
    }

    /// The Python relative-import specifier for a target module given as
    /// root-relative path segments (`["autosql", "ports"]`), resolved against
    /// `self.importer_dir`. Mirrors the ESM relative-specifier logic: drop the
    /// shared leading directories, then emit `1 + ups` leading dots followed by
    /// the remaining segments joined with `.`.
    fn relative_module(&self, target: &[&str]) -> String {
        let importer = &self.importer_dir;
        let mut shared = 0;
        while shared < importer.len()
            && shared + 1 < target.len()
            && importer[shared] == target[shared]
        {
            shared += 1;
        }
        let ups = importer.len() - shared;
        let dots = ".".repeat(1 + ups);
        format!("{dots}{}", target[shared..].join("."))
    }

    /// Emit a `trait` as a `Protocol` — the structural contract a host's `ext`
    /// implementor satisfies. A generic parameter bounded by the trait projects
    /// to this `Protocol`, so a method call through the bound type-checks. Bodies
    /// are `...`; the runtime behaviour lives in the host implementation.
    fn emit_trait(&mut self, item: &ast::Item, t: &ast::TraitDecl) {
        self.record_public(item.vis, &t.name);
        self.uses_protocol = true;
        self.line(format!("class {}(Protocol):", py_ident(&t.name)));
        self.indented(|e| {
            if t.items.is_empty() {
                e.line("...");
                return;
            }
            for ti in &t.items {
                let header = e.fn_header(&ti.sig, true);
                e.line(header);
                e.indented(|e| e.line("..."));
            }
        });
        self.line("");
    }

    /// Emit a free function or a method. `target` is `Some` only when called
    /// from the method path; the receiver is kept as `self`. A `pub` free fn is
    /// recorded for `__all__`.
    fn emit_fn(&mut self, target: Option<&str>, f: &ast::FnDecl, vis: ast::Visibility) {
        if target.is_none() {
            self.record_public(vis, &f.sig.name);
        }
        let header = self.fn_header(&f.sig, target.is_some());
        self.line(header);
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.line("");
    }

    /// Emit an `impl` method into the current (class) body.
    fn emit_method(&mut self, f: &ast::FnDecl) {
        let is_method = f
            .sig
            .params
            .iter()
            .any(|p| matches!(p, ast::Param::Receiver));
        if !is_method {
            self.line("@staticmethod");
        }
        let header = self.fn_header(&f.sig, is_method);
        self.line(header);
        self.indented(|e| e.emit_block_as_body(&f.body));
    }

    /// Build a `def name(params) -> R:` header. `in_class` keeps a `self`
    /// receiver verbatim; a free function drops nothing.
    fn fn_header(&mut self, sig: &ast::FnSig, in_class: bool) -> String {
        let bounds = pyi::single_bound_generics(sig);
        let mut params: Vec<String> = Vec::new();
        for p in &sig.params {
            match p {
                ast::Param::Receiver => params.push("self".to_string()),
                ast::Param::Named { name, ty, .. } => {
                    self.note_type(ty);
                    params.push(format!("{name}: {}", pyi::py_type_bound(ty, &bounds)));
                }
            }
        }
        if let Some(ret) = &sig.ret {
            self.note_type(ret);
        }
        let ret = sig
            .ret
            .as_ref()
            .map_or_else(|| "None".to_string(), |t| pyi::py_type_bound(t, &bounds));
        // A free `fn`'s name is a module binding (escaped on a reserved
        // collision); a method name is an attribute and never collides.
        let name = if in_class {
            sig.name.clone()
        } else {
            py_ident(&sig.name)
        };
        format!("def {name}({}) -> {}:", params.join(", "), ret)
    }

    /// Track that a type annotation references `Callable`/`Result`/`Option`, so
    /// the corresponding import lands in the preamble.
    pub(crate) fn note_type(&mut self, ty: &ast::Type) {
        match ty {
            ast::Type::Fn { params, ret } => {
                self.uses_callable = true;
                for t in params {
                    self.note_type(t);
                }
                if let Some(r) = ret {
                    self.note_type(r);
                }
            }
            ast::Type::List(inner) => self.note_type(inner),
            ast::Type::Path(p) => {
                for seg in &p.segments {
                    if matches!(seg.name.as_str(), "Result" | "Option") {
                        self.uses_result = true;
                    }
                    for t in &seg.args {
                        self.note_type(t);
                    }
                }
            }
            ast::Type::Unit | ast::Type::SelfType => {}
        }
    }

    /// Register each generic parameter as a module-level `TypeVar`. `on_class`
    /// is set when the params decorate a type (a `struct`/`enum`), which also
    /// emits `Generic[...]` on the class header; a generic `fn`'s params only
    /// need the `TypeVar` so its `name: T` annotations resolve.
    fn note_generics(&mut self, generics: &[ast::GenericParam], on_class: bool) {
        for g in generics {
            // A `fn`'s single-bound param projects to its bound's `Protocol`, so
            // it needs no `TypeVar`. A type's params always sit in `Generic[...]`.
            let single_bound =
                !on_class && matches!(g.bounds.as_slice(), [b] if b.segments.last().is_some());
            if single_bound {
                continue;
            }
            if on_class {
                self.uses_generic = true;
            } else if !self.uses_generic {
                // `TypeVar` still has to be imported even with no `Generic` use.
                self.needs_typevar = true;
            }
            if !self.type_vars.contains(&g.name) {
                self.type_vars.push(g.name.clone());
            }
        }
    }

    pub(crate) fn note_prelude(&mut self, name: &str) {
        if matches!(name, "Ok" | "Err" | "Some" | "None" | "Result" | "Option") {
            self.uses_result = true;
        }
    }
}

/// What a derive is applied to.
#[derive(Clone, Copy)]
pub(crate) enum DeriveTarget<'a> {
    Struct(&'a ast::StructDecl),
    Enum(&'a ast::EnumDecl),
}

/// A class header `class Name:` / `class Name(Generic[T]):`.
fn class_header(name: &str, generics: &[ast::GenericParam]) -> String {
    let name = py_ident(name);
    if generics.is_empty() {
        format!("class {name}:")
    } else {
        let ps: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
        format!("class {name}(Generic[{}]):", ps.join(", "))
    }
}

/// The struct/enum names carrying methods — an `impl` block or a `#[derive]`.
fn method_type_names(module: &ast::Module) -> HashSet<String> {
    let type_names: HashSet<&str> = module
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ast::ItemKind::Struct(s) => Some(s.name.as_str()),
            ast::ItemKind::Enum(e) => Some(e.name.as_str()),
            _ => None,
        })
        .collect();
    let mut out = HashSet::new();
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Struct(s) if has_derive(&item.attrs) => {
                out.insert(s.name.clone());
            }
            ast::ItemKind::Enum(e) if has_derive(&item.attrs) => {
                out.insert(e.name.clone());
            }
            ast::ItemKind::Impl(b) => {
                if let ast::Type::Path(p) = &b.ty
                    && let Some(seg) = p.segments.last()
                    && type_names.contains(seg.name.as_str())
                {
                    out.insert(seg.name.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// The module-level value names that were escaped at their definition or
/// import because their spelling collides with a Python builtin/keyword — the
/// free `fn`s, `const`s, and `use`-imported names a bare value path may
/// reference. A name is included only when [`py_ident`] would rewrite it, so
/// the reserved-word rule stays single-sourced. Locals are deliberately absent:
/// they are never escaped, so a local spelled like a reserved word still emits
/// verbatim at its reference.
fn escaped_value_names(module: &ast::Module) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut record = |name: &str| {
        if py_ident(name) != name {
            out.insert(name.to_string());
        }
    };
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Fn(f) => record(&f.sig.name),
            ast::ItemKind::Const(c) => record(&c.name),
            ast::ItemKind::Use(u) => {
                // The name a `use` binds locally is its alias, or the path's
                // final segment. That is the spelling a reference uses, so it is
                // the spelling that gets escaped (the import was emitted escaped).
                let bound = u
                    .alias
                    .as_deref()
                    .or_else(|| u.path.last().map(String::as_str));
                if let Some(name) = bound {
                    record(name);
                }
            }
            _ => {}
        }
    }
    out
}

fn has_derive(attrs: &[ast::Attribute]) -> bool {
    attrs
        .iter()
        .flat_map(|a| &a.items)
        .any(|i| i.name == "derive")
}

/// The `neoc_lib.py` body: tagged `Result`/`Option` dataclasses (Python has no
/// native `Result`) with the inherent operations of `lang/DYNAMIC.md` §7,
/// kept `match`-able. `None` is emitted as `None_` to avoid colliding with the
/// Python literal.
const NEOC_LIB_PY: &str = r#"from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Union

# `T`/`E` are covariant: a frozen carrier is read-only, so `Ok[None_]` is an
# `Ok[Option[T]]` — the widening every `Ok(None)` / `Ok(Some(x))` arm relies on.
T = TypeVar("T", covariant=True)
E = TypeVar("E", covariant=True)
U = TypeVar("U")
F = TypeVar("F")
G = TypeVar("G")


@dataclass(frozen=True)
class Ok(Generic[T]):
    _0: T

    def is_ok(self) -> bool:
        return True

    def is_err(self) -> bool:
        return False

    def unwrap(self) -> T:
        return self._0

    def unwrap_err(self) -> object:
        raise Exception("called unwrap_err on an Ok value")

    def map(self, f: Callable[[T], U]) -> "Result[U, object]":
        return Ok(f(self._0))

    def map_err(self, f: Callable[[object], F]) -> "Result[T, F]":
        return self


@dataclass(frozen=True)
class Err(Generic[E]):
    _0: E

    def is_ok(self) -> bool:
        return False

    def is_err(self) -> bool:
        return True

    def unwrap(self) -> object:
        raise Exception("called unwrap on an Err value")

    def unwrap_err(self) -> E:
        return self._0

    def map(self, f: Callable[[object], U]) -> "Result[U, E]":
        return self

    def map_err(self, f: Callable[[E], F]) -> "Result[object, F]":
        return Err(f(self._0))


Result = Union[Ok[T], Err[E]]


@dataclass(frozen=True)
class Some(Generic[T]):
    _0: T

    def is_some(self) -> bool:
        return True

    def is_none(self) -> bool:
        return False

    def unwrap(self) -> T:
        return self._0

    def unwrap_or(self, default: object) -> T:
        return self._0

    def map(self, f: Callable[[T], U]) -> "Option[U]":
        return Some(f(self._0))

    def ok_or(self, err: G) -> "Result[T, G]":
        return Ok(self._0)


@dataclass(frozen=True)
class None_:
    def is_some(self) -> bool:
        return False

    def is_none(self) -> bool:
        return True

    def unwrap(self) -> object:
        raise Exception("called unwrap on a None value")

    def unwrap_or(self, default: U) -> U:
        return default

    def map(self, f: Callable[[object], U]) -> "Option[U]":
        return self

    def ok_or(self, err: G) -> "Result[object, G]":
        return Err(err)


Option = Union[Some[T], None_]


__all__ = ["Ok", "Err", "Result", "Some", "None_", "Option"]
"#;

/// The `neoc_lib.pyi`: the typed `Result`/`Option` surface a consumer's stub
/// imports.
const NEOC_LIB_PYI: &str = r#"from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Union

T = TypeVar("T", covariant=True)
E = TypeVar("E", covariant=True)
U = TypeVar("U")
F = TypeVar("F")
G = TypeVar("G")


@dataclass(frozen=True)
class Ok(Generic[T]):
    _0: T
    def is_ok(self) -> bool: ...
    def is_err(self) -> bool: ...
    def unwrap(self) -> T: ...
    def unwrap_err(self) -> object: ...
    def map(self, f: Callable[[T], U]) -> Result[U, object]: ...
    def map_err(self, f: Callable[[object], F]) -> Result[T, F]: ...


@dataclass(frozen=True)
class Err(Generic[E]):
    _0: E
    def is_ok(self) -> bool: ...
    def is_err(self) -> bool: ...
    def unwrap(self) -> object: ...
    def unwrap_err(self) -> E: ...
    def map(self, f: Callable[[object], U]) -> Result[U, E]: ...
    def map_err(self, f: Callable[[E], F]) -> Result[object, F]: ...


Result = Union[Ok[T], Err[E]]


@dataclass(frozen=True)
class Some(Generic[T]):
    _0: T
    def is_some(self) -> bool: ...
    def is_none(self) -> bool: ...
    def unwrap(self) -> T: ...
    def unwrap_or(self, default: object) -> T: ...
    def map(self, f: Callable[[T], U]) -> Option[U]: ...
    def ok_or(self, err: G) -> Result[T, G]: ...


@dataclass(frozen=True)
class None_:
    def is_some(self) -> bool: ...
    def is_none(self) -> bool: ...
    def unwrap(self) -> object: ...
    def unwrap_or(self, default: U) -> U: ...
    def map(self, f: Callable[[object], U]) -> Option[U]: ...
    def ok_or(self, err: G) -> Result[object, G]: ...


Option = Union[Some[T], None_]
"#;

#[cfg(test)]
mod tests;
