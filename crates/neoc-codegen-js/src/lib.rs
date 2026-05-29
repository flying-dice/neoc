//! AST → JavaScript (ES2022) emitter. Mirrors `neoc-codegen-lua`'s structure.
//!
//! The JavaScript backend is neoc's browser/frontend target — the client half of
//! full-stack neoc, with Lua on the backend. Output is an **ES module**: a `use`
//! lowers to a relative, `.js`-suffixed `import`, and a `pub` item to an
//! `export`. That form loads natively in evergreen browsers and bundlers (Vite,
//! `esbuild`) without a `CommonJS` or Node shim. It also runs under `node` as
//! ESM, which the conformance suite uses as the behavioural oracle.
//!
//! Targets ES2022 — the language level supported across all current Node.js LTS
//! lines (18/20/22) and evergreen browsers.
//!
//! Split by construct family: items here, expressions in [`expr`], patterns in
//! [`pat`]. neoc is expression-oriented and JavaScript is statement-oriented, so
//! block-like expressions used for their value become immediately-invoked arrow
//! functions (IIFEs).
//!
//! # Runtime representation
//!
//! - **struct** → a plain object carrying a `__struct` tag and its fields. A
//!   newtype/tuple struct stores its payload positionally under integer keys
//!   (`_0`, `_1`, …). Named structs store fields by name.
//! - **enum** → a tagged object `{ __enum, __tag, … }`. Tuple payload is stored
//!   under `_0`, `_1`, …; struct-variant payload by field name. The
//!   `Result`/`Option` prelude follows the same shape.
//! - **methods** → every type with an `impl` or `#[derive]` gets a module-level
//!   prototype object `const Type$ = {}`; methods and derives land on it as
//!   functions over the implicit `this` (`Point$.manhattan = function() { … }`).
//!   A constructed value links to its prototype via `__proto__` in the object
//!   literal, so a method call `x.m(a)` is the native `x.m(a)` — `this` binds to
//!   the receiver, dispatch resolves up the prototype chain, no registry. A type
//!   with no methods and no derives gets no prototype and stays a plain tagged
//!   object.
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
///
/// Deliberately parallel to `neoc_codegen_lua`'s `EnumIndex` (and
/// `TupleStructIndex`): the two backends are kept independent rather than
/// sharing a crate, so each can diverge as its target needs without coupling
/// the other through a shared dependency. Keep the two in sync by hand.
#[derive(Default)]
pub(crate) struct EnumIndex {
    enums: HashMap<String, HashMap<String, VariantShape>>,
}

impl EnumIndex {
    /// Build the index from a module's own `enum` items plus the `pub` enums it
    /// brings in by `use` (`imported`), seeding the `Result`/`Option` prelude.
    /// An imported `E::V` must resolve at a use site exactly like a local one,
    /// so both are indexed identically.
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
///
/// Cross-module `use` imports are filtered against `imported_bindings`: only a
/// `use` whose resolved `prefix::last` key is present in that set lowers to a JS
/// `import`. The set keys the imports whose source module emits a runtime
/// binding — a `pub fn`, `pub const`, or `pub` struct/enum carrying a method
/// slot. A type-only item (a data-only `pub` struct/enum, or a `pub type` alias)
/// is absent from the set: its name is erased in JS, so importing it would
/// dangle against a module that exports nothing under that name.
///
/// The key is the resolved `(module, name)` pair (`prefix::last`), not the bare
/// name, so two modules sharing a final segment — one a binding, one type-only —
/// get independent decisions. Pass an empty set when no module graph is resolved
/// (single-file `transpile`), where no cross-module `use` can resolve anyway.
///
/// Method dispatch is prototype-backed: a value carries its prototype across the
/// `use` boundary (the link is made in the defining module), so a `use` of a
/// method-bearing type needs no dispatch wiring in the consumer (`lang/DYNAMIC.md`
/// §3.7, §7.1).
///
/// `imported_method_types` is the subset of `imported_bindings` whose source
/// binding is a method-bearing type (see [`module_js_method_types`]). In JS a
/// method-bearing type's runtime binding is its prototype `Type$`, so a `use` of
/// one imports `Type$` (the consumer's own constructions and associated-function
/// calls reference the prototype). A `pub fn`/`pub const` import keeps its bare
/// name. Same `prefix::last` keying; pass an empty set when no graph is resolved.
///
/// `importer_dir` is the importing module's directory as path segments relative
/// to the project source root (`autosql/list_notes.neoc` → `["autosql"]`; a
/// root-level module → empty). It anchors every cross-module `import`'s relative
/// specifier so a module in a subdirectory imports a sibling as `./b.js` rather
/// than `./a/b.js`. Pass an empty slice for single-file `transpile`, where no
/// cross-module `use` resolves.
#[must_use]
pub fn emit<S: std::hash::BuildHasher>(
    module: &ast::Module,
    imported_bindings: &HashSet<String, S>,
    imported_method_types: &HashSet<String, S>,
    imported_externs: &[ast::ExternItem],
    imported_enums: &[&ast::EnumDecl],
    importer_dir: &[String],
) -> EmitResult {
    // A `use`d extern item lowers to its native import in *this* consumer, not an
    // import from the defining file's emitted module — the defining file emits no
    // runtime value for an extern (`lang/EXTERN.md` §Lowering). Merge the imported
    // extern items into the same maps the module's own `extern` blocks fill, so an
    // imported `Database::new(...)` lowers identically to a local one.
    let mut extern_structs = ast::extern_struct_names(module);
    extern_structs.extend(ast::extern_struct_names_from_items(imported_externs));
    let mut extern_constructors = ast::extern_constructors(module);
    extern_constructors.extend(ast::extern_constructors_from_items(imported_externs));
    let mut extern_struct_ns = ast::extern_struct_namespaces(module);
    extern_struct_ns.extend(ast::extern_struct_namespaces_from_items(imported_externs));
    let mut extern_fn_ns = ast::extern_fn_namespaces(module);
    extern_fn_ns.extend(ast::extern_fn_namespaces_from_items(imported_externs));
    let mut e = Emitter {
        enums: EnumIndex::build(module, imported_enums),
        tuple_structs: TupleStructIndex::build(module),
        traits: ast::trait_index(module),
        imported_bindings: imported_bindings.iter().cloned().collect(),
        imported_method_types: imported_method_types.iter().cloned().collect(),
        extern_structs,
        extern_constructors,
        extern_struct_ns,
        extern_fn_ns,
        imported_externs: imported_externs.to_vec(),
        importer_dir: importer_dir.to_vec(),
        ..Emitter::default()
    };
    e.emit_module(module);
    let diagnostics = std::mem::take(&mut e.diagnostics);
    EmitResult {
        js: e.finish(),
        diagnostics,
    }
}

/// The names `module` emits a runtime JS binding for, reachable by a `use` in
/// another module. A binding is a `pub fn`, a `pub const`, or a `pub`
/// struct/enum that carries a prototype (an `impl` block or a `#[derive]`,
/// emitted as `export const Name$ = {}`). A data-only `pub` struct/enum and a
/// `pub type` alias emit nothing — their names are erased in JS — so they are
/// excluded. The compiler driver intersects this set with each importing
/// module's `use` names to decide which imports to emit.
///
/// Method dispatch is prototype-backed (`lang/DYNAMIC.md` §7.1): a value carries
/// its prototype across the `use` boundary, so the import needs no per-type
/// wiring in the consumer — the plain `import` of the type's name suffices.
#[must_use]
pub fn module_js_bindings(module: &ast::Module) -> HashSet<String> {
    // A `pub` method-bearing type is a binding (a `use` of it emits an import); a
    // `pub fn`/`pub const` adds one too. A data-only type or alias emits nothing.
    let method_types = method_type_names(module);
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
            ast::ItemKind::Struct(s) if method_types.contains(s.name.as_str()) => {
                bindings.insert(s.name.clone());
            }
            ast::ItemKind::Enum(e) if method_types.contains(e.name.as_str()) => {
                bindings.insert(e.name.clone());
            }
            _ => {}
        }
    }
    bindings
}

/// The subset of [`module_js_bindings`] that are method-bearing types: a `pub`
/// struct/enum carrying an `impl` block or `#[derive]`, emitted as
/// `export const Name$ = {}`. A method-bearing type's JS runtime binding is its
/// prototype `Name$` (not the bare type name, which is erased), so a `use` of one
/// imports `Name$`. A `pub fn`/`pub const` binding keeps its bare name and is
/// absent.
#[must_use]
pub fn module_js_method_types(module: &ast::Module) -> HashSet<String> {
    let method_types = method_type_names(module);
    let mut bindings = HashSet::new();
    for item in &module.items {
        if item.vis != ast::Visibility::Public {
            continue;
        }
        match &item.kind {
            ast::ItemKind::Struct(s) if method_types.contains(s.name.as_str()) => {
                bindings.insert(s.name.clone());
            }
            ast::ItemKind::Enum(e) if method_types.contains(e.name.as_str()) => {
                bindings.insert(e.name.clone());
            }
            _ => {}
        }
    }
    bindings
}

/// The struct/enum names in `module` that carry a prototype — any with
/// a `#[derive(...)]` or an `impl` block targeting them. Shared by binding
/// detection ([`module_js_bindings`]) and the emitter's own prototype collection.
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
    let mut method_types = HashSet::new();
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Struct(s) if has_derive(&item.attrs) => {
                method_types.insert(s.name.clone());
            }
            ast::ItemKind::Enum(e) if has_derive(&item.attrs) => {
                method_types.insert(e.name.clone());
            }
            ast::ItemKind::Impl(b) => {
                if let ast::Type::Path(p) = &b.ty {
                    if let Some(seg) = p.segments.last() {
                        if type_names.contains(seg.name.as_str()) {
                            method_types.insert(seg.name.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    method_types
}

/// Builds the ESM import specifier for a cross-module `use`, resolving the
/// target module's file relative to the importing module's directory.
///
/// `importer_dir` is the importing module's directory as root-relative
/// segments (e.g. `["autosql"]`); `target` is the target module's file as
/// root-relative segments (e.g. `["autosql", "ports"]`). The result is an
/// ES2022 relative specifier ending in `.js`, always anchored with `./` or
/// `../` so it never reads as a bare (package) import:
///
/// * same directory → `./ports.js`
/// * subdirectory sibling, importer `["autosql"]`, target `["autosql","ports"]`
///   → `./ports.js`
/// * root importer, target `["autosql","ports"]` → `./autosql/ports.js`
/// * importer `["autosql"]`, target `["shared"]` → `../shared.js`
fn relative_module_specifier(importer_dir: &[String], target: &[&str]) -> String {
    // Drop the shared leading directories common to both paths.
    let mut shared = 0;
    while shared < importer_dir.len()
        && shared + 1 < target.len()
        && importer_dir[shared] == target[shared]
    {
        shared += 1;
    }
    let ups = importer_dir.len() - shared;
    let mut spec = String::new();
    if ups == 0 {
        spec.push_str("./");
    } else {
        for _ in 0..ups {
            spec.push_str("../");
        }
    }
    spec.push_str(&target[shared..].join("/"));
    spec.push_str(".js");
    spec
}

/// Accumulates emitted JavaScript and diagnostics. Output is built line by line
/// at the current indentation level.
#[derive(Default)]
pub(crate) struct Emitter {
    lines: Vec<String>,
    indent: usize,
    diagnostics: Vec<Diagnostic>,
    uses_result: bool,
    /// Enum declarations, used to resolve enum-variant paths and to tell a
    /// variant constructor apart from an ordinary call.
    enums: EnumIndex,
    /// Tuple/newtype struct names, used to tell a struct constructor call
    /// (`N(x)` → `N.new(x)`) apart from an ordinary function call.
    tuple_structs: TupleStructIndex,
    /// Types (struct/enum names) that carry a prototype — any `impl` block or
    /// `#[derive]`. A constructed value of such a type links to `Type$` via
    /// `__proto__`; a type absent here stays a plain tagged object.
    method_types: HashSet<String>,
    /// Whether any `__t` trait-registry entry was emitted (gates its prelude).
    uses_traits: bool,
    /// Per-type trait set (transitively closed over supertraits) for the
    /// `__t` trait registry — `__t.Type = [ … ]` keyed by type name
    /// (`DYNAMIC.md`). Inert until the runtime trait-enforcement work (#54).
    traits: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    /// Counter feeding unique temporaries for the `?` operator.
    try_seq: usize,
    /// Counter feeding unique temporaries for destructuring / match scrutinees.
    tmp_seq: usize,
    /// Target-module imports from `extern` blocks (`lang/EXTERN.md`). A
    /// non-ambient `mod` becomes an ESM `import`; an ambient `mod` carries a
    /// global-root member chain and emits no import.
    extern_imports: Vec<ast::ExternImport>,
    /// Names declared as extern `struct`s. A struct literal of such a name
    /// lowers to a bare target object `{ f: v }` (no `__struct` tag); the extern
    /// value *is* the target value.
    extern_structs: HashSet<String>,
    /// `(type, method)` pairs marked `#[extern(constructor)]`. A call
    /// `T::new(args)` to one of these lowers to `new T(args)`.
    extern_constructors: HashSet<(String, String)>,
    /// Maps an extern `struct` declared inside an extern `mod` to its enclosing
    /// `mod`'s import binding. A reference to the struct's *value* (constructor,
    /// associated call) qualifies through this binding (`new sqlite.Database(a)`)
    /// so it resolves against the imported namespace, not a bare unbound name
    /// (`lang/EXTERN.md` §Lowering).
    extern_struct_ns: std::collections::HashMap<String, String>,
    /// Maps a free extern `fn` declared inside an extern `mod` to its enclosing
    /// `mod`'s import binding. A call to the fn qualifies through this binding
    /// (`Bun.serve(opts)`) so it resolves against the imported namespace rather
    /// than a bare unbound name (`lang/EXTERN.md` §Lowering). A fn with no
    /// enclosing `mod` (top-level) keeps its bare name. Populated from both the
    /// module's own `extern` blocks and `use`-imported extern items, so a `use`d
    /// ambient-mod fn lowers identically to a locally-declared one.
    extern_fn_ns: std::collections::HashMap<String, String>,
    /// Resolved `use` imports whose source module emits a runtime binding, keyed
    /// per import as `prefix::last`. A `use` whose key is in this set lowers to a
    /// JS `import`; a type-only import (absent here) emits none, since its name
    /// is erased in JS. Keying on the resolved `(module, name)` rather than the
    /// bare name keeps two modules' shared final segment independent. See
    /// [`emit`].
    imported_bindings: HashSet<String>,
    /// The subset of [`imported_bindings`](Self::imported_bindings) that resolve
    /// to a method-bearing type. A `use` whose key is here imports the type's
    /// prototype `Type$` (its JS runtime binding) rather than the bare,
    /// erased type name. Same `prefix::last` keying. See [`emit`].
    imported_method_types: HashSet<String>,
    /// Extern items a `use` brought in from another file. Their `mod` imports are
    /// merged into [`extern_imports`](Self::extern_imports) so the consumer emits
    /// the same native `import` the defining module would, reconstructed from the
    /// imported item's `#[extern(...)]` metadata (`lang/EXTERN.md` §Lowering).
    imported_externs: Vec<ast::ExternItem>,
    /// The importing module's own directory, as path segments relative to the
    /// project source root (`autosql/list_notes.neoc` → `["autosql"]`; a
    /// root-level module → empty). A cross-module `use a::b::X` lowers to an ESM
    /// `import` whose specifier is the target module's file (`a/b.js`) resolved
    /// *relative to this directory*, so a module in a subdirectory imports its
    /// sibling as `./b.js`, not `./a/b.js`. Empty for single-file `transpile`,
    /// where no cross-module `use` resolves anyway.
    importer_dir: Vec<String>,
}

/// The `Result`/`Option` prelude: variant constructors as plain tagged objects.
const RESULT_PRELUDE: &str = "\
const Ok = (v) => ({ __enum: \"Result\", __tag: \"Ok\", _0: v });
const Err = (e) => ({ __enum: \"Result\", __tag: \"Err\", _0: e });
const Some = (v) => ({ __enum: \"Option\", __tag: \"Some\", _0: v });
const None = { __enum: \"Option\", __tag: \"None\" };";

/// The trait registry: a table keyed by type name listing the traits each type
/// implements (`__traits` runtime tag). Read as `__t[recv.__struct ?? recv.__enum]`.
const TRAITS_PRELUDE: &str = "const __t = {};";

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
        // Extern `mod` imports (`lang/EXTERN.md`): a non-ambient `mod` imports
        // its handle namespace-wide (`import * as fs from "fs"`), so a
        // `fs::read(a)` reference reads `fs.read(a)` directly. An ambient `mod`
        // is reached through its global-root chain and emits no import.
        for imp in &self.extern_imports {
            if imp.ambient {
                // The reference resolves against the global root; bind the local
                // name to that member chain.
                if imp.local_name != imp.path {
                    preamble.push(format!("const {} = {};", imp.local_name, imp.path));
                }
            } else {
                preamble.push(format!(
                    "import * as {} from \"{}\";",
                    imp.local_name, imp.path
                ));
            }
        }
        if self.uses_traits {
            preamble.push(TRAITS_PRELUDE.to_string());
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
        self.extern_imports = ast::extern_imports(module, ast::Target::Js);
        // A `use`d extern item emits its native import here, in the consumer, not
        // an import from the defining file (`lang/EXTERN.md` §Lowering).
        let imported = std::mem::take(&mut self.imported_externs);
        self.extern_imports
            .extend(ast::extern_imports_from_items(&imported, ast::Target::Js));
        for item in &module.items {
            self.emit_item(item);
        }
    }

    /// Record every type that needs a prototype: a struct/enum with a
    /// `#[derive(...)]` or any `impl` block targeting it.
    fn collect_method_types(&mut self, module: &ast::Module) {
        self.method_types = method_type_names(module);
    }

    /// Whether `name` is a type with a prototype. Used to decide when to emit the
    /// `const name$ = {}` prototype and to link a constructed value to it via
    /// `__proto__`.
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
            ast::ItemKind::Fn(f) => self.emit_fn(None, f, item.vis),
            ast::ItemKind::Struct(s) => {
                self.ensure_method_slot(&s.name, item.vis);
                self.emit_derives(&item.attrs, neoc_ast_derive_target(item));
                self.emit_traits_tag(&s.name);
            }
            ast::ItemKind::Const(c) => self.emit_const(c, item.vis),
            ast::ItemKind::Impl(b) => self.emit_impl(b),
            ast::ItemKind::Use(u) => self.emit_use(u),
            ast::ItemKind::Enum(e) => {
                self.ensure_method_slot(&e.name, item.vis);
                self.emit_derives(&item.attrs, neoc_ast_derive_target(item));
                self.emit_traits_tag(&e.name);
            }
            // No JS output: aliases erase; an `extern` block describes the
            // target surface — its `mod` imports emit in the preamble, its
            // structs/fns/methods are the target's own values.
            ast::ItemKind::TypeAlias(_) | ast::ItemKind::Extern(_) | ast::ItemKind::Trait(_) => {}
        }
    }

    /// Emit the `__t.Type = [ … ]` trait-registry entry for a type, listing
    /// every trait it implements (its supertraits included). Keyed by type name,
    /// so `__t[value.__struct ?? value.__enum]` resolves for any instance.
    /// Skipped for a type implementing no trait. Inert until #54.
    fn emit_traits_tag(&mut self, name: &str) {
        let Some(traits) = self.traits.get(name) else {
            return;
        };
        if traits.is_empty() {
            return;
        }
        self.uses_traits = true;
        let list = traits
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        self.line(format!("__t.{name} = [{list}];"));
    }

    /// Emit the module-level prototype `const name$ = {}` for a type that carries
    /// methods or derives. A constructed value of the type links to it via
    /// `__proto__` (`lang/DYNAMIC.md` §3.1). A `pub` type exports the prototype
    /// so a cross-module trait impl could augment it (§7.2); dispatch itself
    /// needs no export, since a value carries its prototype across the boundary.
    fn ensure_method_slot(&mut self, name: &str, vis: ast::Visibility) {
        if self.type_has_methods(name) {
            let kw = export_kw(vis);
            self.line(format!("{kw}const {name}$ = {{}};"));
        }
    }

    /// Emit the methods generated by each `#[derive(...)]` on a type.
    fn emit_derives(&mut self, attrs: &[ast::Attribute], target: DeriveTarget<'_>) {
        for name in ast::derive_names(attrs) {
            self.emit_derive(&name, target);
        }
    }

    fn emit_use(&mut self, u: &ast::UseDecl) {
        let Some((last, prefix)) = u.path.split_last() else {
            return;
        };
        // A type-only item — a data-only struct/enum or a `pub type` alias —
        // emits no binding in its source module, so importing its name would
        // dangle against an empty export. Such names are erased in JS
        // (constructions lower to inline objects), so emit no import. The keep
        // decision is keyed per resolved import (`prefix::last`), rebuilt here to
        // match the driver's key, so a name shared by two source modules — one a
        // binding, one type-only — is decided per source.
        if !self.imported_bindings.contains(&u.path.join("::")) {
            return;
        }
        // The target module file (`a/b.neoc` → segments `["a", "b"]`), made
        // relative to *this* module's directory so the ESM specifier resolves
        // correctly from a subdirectory.
        let target_segments: Vec<&str> = prefix.iter().map(String::as_str).collect();
        let module = relative_module_specifier(&self.importer_dir, &target_segments);
        // A method-bearing type's JS runtime binding is its prototype `Type$`
        // (the bare type name is erased), so a `use` of one imports `Type$`;
        // the consumer's constructions and associated-function calls reference
        // it, and a value carries dispatch across the boundary with no further
        // wiring (`lang/DYNAMIC.md` §3.7, §7.1). A `pub fn`/`pub const` keeps its
        // bare name.
        let suffix = if self.imported_method_types.contains(&u.path.join("::")) {
            "$"
        } else {
            ""
        };
        let named = match &u.alias {
            Some(alias) => format!("{last}{suffix} as {alias}{suffix}"),
            None => format!("{last}{suffix}"),
        };
        self.line(format!("import {{ {named} }} from \"{module}\";"));
    }

    fn emit_const(&mut self, c: &ast::ConstDecl, vis: ast::Visibility) {
        let value = self.emit_expr(&c.value);
        let kw = export_kw(vis);
        self.line(format!("{kw}const {} = {value};", c.name));
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
            self.emit_fn(Some(&target), f, ast::Visibility::Public);
        }
    }

    /// Emit a function. `target` is the type name when this is a method or
    /// associated function inside an impl: it lands on the type's prototype
    /// `target$`. A method's `self` receiver is dropped — the receiver is the
    /// implicit `this`, and `self` reads as `this` in the body (`lang/DYNAMIC.md`
    /// §3.3). An associated function (no `self`) keeps all its parameters and is
    /// called as `target$.f(args)`. `vis` applies only to a free function — a
    /// `pub` one becomes an exported declaration; impl methods live on the
    /// prototype and are reached through the value, never exported by keyword.
    fn emit_fn(&mut self, target: Option<&str>, f: &ast::FnDecl, vis: ast::Visibility) {
        let mut params = Vec::new();
        for p in &f.sig.params {
            match p {
                // The receiver is the implicit `this`; drop it from the JS
                // parameter list.
                ast::Param::Receiver => {}
                ast::Param::Named { name, .. } => params.push(name.clone()),
            }
        }
        let param_list = params.join(", ");
        if let Some(t) = target {
            self.line(format!("{t}$.{} = function({param_list}) {{", f.sig.name));
        } else {
            let kw = export_kw(vis);
            self.line(format!("{kw}function {}({param_list}) {{", f.sig.name));
        }
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.line("}");
    }
}

/// The leading keyword for a module-level binding: `"export "` for a `pub` item,
/// empty for a module-local one.
fn export_kw(vis: ast::Visibility) -> &'static str {
    if vis == ast::Visibility::Public {
        "export "
    } else {
        ""
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

    /// Emit `module` with no resolved cross-module bindings — the common case
    /// for these single-module fixtures, which carry no cross-module `use`.
    fn emit_solo(module: &ast::Module) -> EmitResult {
        emit(module, &HashSet::new(), &HashSet::new(), &[], &[], &[])
    }

    #[test]
    fn relative_specifier_anchors_to_importer_dir() {
        // Root-level importer keeps the full target path.
        assert_eq!(relative_module_specifier(&[], &["a", "b"]), "./a/b.js");
        // A subdirectory importer reaches a sibling in the same directory.
        assert_eq!(
            relative_module_specifier(&["autosql".to_string()], &["autosql", "ports"]),
            "./ports.js"
        );
        // Crossing out of a subdirectory walks up with `../`.
        assert_eq!(
            relative_module_specifier(&["autosql".to_string()], &["shared"]),
            "../shared.js"
        );
        // Deeper importer, sibling in the same deep directory.
        assert_eq!(
            relative_module_specifier(&["a".to_string(), "b".to_string()], &["a", "b", "c"]),
            "./c.js"
        );
    }

    #[test]
    fn cross_file_use_of_ambient_mod_fn_qualifies_through_its_namespace() {
        // A consumer `use`s a free fn `serve` declared in an ambient
        // `#[extern(ambient, path = "Bun")] mod Bun { fn serve(o: Unknown); }`
        // from another file, and calls it bare: `serve(opts)`. It must lower to
        // `Bun.serve(opts)` — the ambient mod's global root — not a bare,
        // unbound `serve(opts)` (`lang/EXTERN.md` §Cross-file `use`).
        let serve_fn = ast::ExternItem {
            attrs: Vec::new(),
            vis: ast::Visibility::Public,
            kind: ast::ExternItemKind::Fn(ast::FnSig {
                name: "serve".to_string(),
                name_span: ast::Span::default(),
                generics: Vec::new(),
                params: vec![ast::Param::Named {
                    name: "o".to_string(),
                    ty: ast::Type::Unit,
                    span: ast::Span::default(),
                }],
                ret: None,
                where_clause: Vec::new(),
            }),
            span: ast::Span::default(),
        };
        let bun_mod = ast::ExternItem {
            attrs: vec![ast::Attribute {
                items: vec![ast::AttrItem {
                    name: "extern".to_string(),
                    args: vec![
                        ast::AttrArg::Item(ast::AttrItem {
                            name: "ambient".to_string(),
                            args: Vec::new(),
                        }),
                        ast::AttrArg::Named {
                            name: "path".to_string(),
                            value: Box::new(ast::AttrArg::Str("Bun".to_string())),
                        },
                    ],
                }],
            }],
            vis: ast::Visibility::Private,
            kind: ast::ExternItemKind::Mod(ast::ExternMod {
                name: "Bun".to_string(),
                items: vec![serve_fn],
            }),
            span: ast::Span::default(),
        };
        // fn run() { serve(0) }   — `serve` reached via the imported extern items.
        let module = ast::Module {
            items: vec![fn_item(
                "run",
                e(ast::ExprKind::Call {
                    callee: Box::new(e(ast::ExprKind::Path(vec!["serve".to_string()]))),
                    args: vec![int("0")],
                }),
            )],
        };
        let out = emit(
            &module,
            &HashSet::new(),
            &HashSet::new(),
            &[bun_mod],
            &[],
            &[],
        );
        assert!(
            out.js.contains("Bun.serve(0)"),
            "expected `Bun.serve(0)`, got:\n{}",
            out.js
        );
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
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

    fn field(name: &str) -> ast::Field {
        ast::Field {
            attrs: Vec::new(),
            name: name.to_string(),
            name_span: ast::Span::default(),
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
                    name_span: ast::Span::default(),
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

    /// A `fn main() { <stmts> }` item with no trailing value.
    fn main_fn(stmts: Vec<ast::Stmt>) -> ast::Item {
        item(
            ast::ItemKind::Fn(ast::FnDecl {
                attrs: Vec::new(),
                sig: ast::FnSig {
                    name: "main".to_string(),
                    name_span: ast::Span::default(),
                    generics: Vec::new(),
                    params: Vec::new(),
                    ret: None,
                    where_clause: Vec::new(),
                },
                body: ast::Block { stmts, value: None },
            }),
            Vec::new(),
        )
    }

    /// A `let _ = <value>;` wildcard-discard statement.
    fn wildcard_discard(value: ast::Expr) -> ast::Stmt {
        ast::Stmt::new(
            ast::StmtKind::Let(ast::LetStmt {
                mutable: false,
                pattern: ast::Pattern::Wildcard,
                ty: None,
                value: Some(value),
            }),
            ast::Span::default(),
        )
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
                        field: ast::FieldAccess::Named("x".to_string(), ast::Span::default()),
                    }),
                ),
            ],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function make() {\n  \
             return { __struct: \"Point\", x: 1, y: 2 }.x;\n}\n"
        );
    }

    /// A closure whose body is a struct or enum-variant object literal must
    /// paren-wrap the body: `(n) => ({ … })`. A bare `(n) => { … }` parses the
    /// `{ … }` as a function block, not a returned object, so the literal's
    /// fields become broken statements — a runtime `SyntaxError`. Regression for
    /// #68.
    #[test]
    fn closure_returning_object_literal_is_paren_wrapped() {
        // struct Point { x, y }
        // enum Shape { Sq { side } }
        // fn main() {
        //   let mk = |n| Point { x: n, y: n };
        //   let sq = |n| Shape::Sq { side: n };
        // }
        let struct_closure = ast::Stmt::new(
            ast::StmtKind::Let(ast::LetStmt {
                mutable: false,
                pattern: ast::Pattern::Binding {
                    name: "mk".to_string(),
                    span: ast::Span::default(),
                },
                ty: None,
                value: Some(e(ast::ExprKind::Closure {
                    params: vec![ast::ClosureParam {
                        name: "n".to_string(),
                        ty: None,
                        span: ast::Span::default(),
                    }],
                    body: Box::new(e(ast::ExprKind::StructLit {
                        path: vec!["Point".to_string()],
                        fields: vec![
                            ast::FieldInit {
                                name: "x".to_string(),
                                value: Some(e(ast::ExprKind::Path(vec!["n".to_string()]))),
                            },
                            ast::FieldInit {
                                name: "y".to_string(),
                                value: Some(e(ast::ExprKind::Path(vec!["n".to_string()]))),
                            },
                        ],
                        base: None,
                    })),
                })),
            }),
            ast::Span::default(),
        );
        let enum_closure = ast::Stmt::new(
            ast::StmtKind::Let(ast::LetStmt {
                mutable: false,
                pattern: ast::Pattern::Binding {
                    name: "sq".to_string(),
                    span: ast::Span::default(),
                },
                ty: None,
                value: Some(e(ast::ExprKind::Closure {
                    params: vec![ast::ClosureParam {
                        name: "n".to_string(),
                        ty: None,
                        span: ast::Span::default(),
                    }],
                    body: Box::new(e(ast::ExprKind::StructLit {
                        path: vec!["Shape".to_string(), "Sq".to_string()],
                        fields: vec![ast::FieldInit {
                            name: "side".to_string(),
                            value: Some(e(ast::ExprKind::Path(vec!["n".to_string()]))),
                        }],
                        base: None,
                    })),
                })),
            }),
            ast::Span::default(),
        );
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
                item(
                    ast::ItemKind::Enum(ast::EnumDecl {
                        name: "Shape".to_string(),
                        generics: Vec::new(),
                        variants: vec![ast::Variant {
                            attrs: Vec::new(),
                            name: "Sq".to_string(),
                            name_span: ast::Span::default(),
                            body: ast::VariantBody::Named(vec![field("side")]),
                        }],
                    }),
                    Vec::new(),
                ),
                main_fn(vec![struct_closure, enum_closure]),
            ],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(
            out.js
                .contains("let mk = (n) => ({ __struct: \"Point\", x: n, y: n });"),
            "struct-body closure must paren-wrap the object literal, got:\n{}",
            out.js
        );
        assert!(
            out.js
                .contains("let sq = (n) => ({ __enum: \"Shape\", __tag: \"Sq\", side: n });"),
            "enum-variant-body closure must paren-wrap the object literal, got:\n{}",
            out.js
        );
    }

    /// An associated-function call `Type::fn(args)` reads the type's prototype —
    /// `Type$.fn(args)` — not a bare `fn(args)`, which would be an unbound name at
    /// runtime. Regression for #67.
    #[test]
    fn associated_fn_call_reads_method_table() {
        // struct P { x }
        // impl P { fn make(v) -> P { P { x: v } } }
        // fn caller() { P::make(5) }
        let make = ast::FnDecl {
            attrs: Vec::new(),
            sig: ast::FnSig {
                name: "make".to_string(),
                name_span: ast::Span::default(),
                generics: Vec::new(),
                params: vec![ast::Param::Named {
                    name: "v".to_string(),
                    ty: ast::Type::Unit,
                    span: ast::Span::default(),
                }],
                ret: None,
                where_clause: Vec::new(),
            },
            body: ast::Block {
                stmts: Vec::new(),
                value: Some(Box::new(e(ast::ExprKind::StructLit {
                    path: vec!["P".to_string()],
                    fields: vec![ast::FieldInit {
                        name: "x".to_string(),
                        value: Some(e(ast::ExprKind::Path(vec!["v".to_string()]))),
                    }],
                    base: None,
                }))),
            },
        };
        let module = ast::Module {
            items: vec![
                item(
                    ast::ItemKind::Struct(ast::StructDecl {
                        name: "P".to_string(),
                        generics: Vec::new(),
                        body: ast::StructBody::Named(vec![field("x")]),
                    }),
                    Vec::new(),
                ),
                item(
                    ast::ItemKind::Impl(ast::ImplBlock {
                        generics: Vec::new(),
                        trait_: None,
                        ty: ast::Type::Path(ast::TypePath {
                            segments: vec![ast::TypeSegment {
                                name: "P".to_string(),
                                args: Vec::new(),
                            }],
                        }),
                        items: vec![make],
                        assoc_bindings: Vec::new(),
                    }),
                    Vec::new(),
                ),
                fn_item(
                    "caller",
                    e(ast::ExprKind::Call {
                        callee: Box::new(e(ast::ExprKind::Path(vec![
                            "P".to_string(),
                            "make".to_string(),
                        ]))),
                        args: vec![int("5")],
                    }),
                ),
            ],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(
            out.js.contains("P$.make = function(v) {"),
            "associated fn defined on the prototype:\n{}",
            out.js
        );
        assert!(
            out.js.contains("return P$.make(5);"),
            "call site reads the prototype, not a bare name:\n{}",
            out.js
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
                                name_span: ast::Span::default(),
                                body: ast::VariantBody::Unit,
                            },
                            ast::Variant {
                                attrs: Vec::new(),
                                name: "Green".to_string(),
                                name_span: ast::Span::default(),
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
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function pick() {\n  \
             return (() => {\n    \
             const __s1 = { __enum: \"Color\", __tag: \"Red\" };\n    \
             if (__s1.__tag === \"Red\") {\n      \
             return 1;\n    \
             }\n    \
             if (__s1.__tag === \"Green\") {\n      \
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
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "const __t = {};\n\
             \n\
             const Point$ = {};\n\
             Point$.debug = function() {\n  \
             return \"Point { \" + \"x = \" + String(this.x) + \", \" + \
             \"y = \" + String(this.y) + \" }\";\n\
             };\n\
             __t.Point = [\"Debug\"];\n"
        );
    }

    #[test]
    fn derive_clone_struct() {
        // #[derive(Clone)] struct Point { x, y } — a structural copy that
        // re-links the prototype so methods still resolve on the clone.
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Struct(ast::StructDecl {
                    name: "Point".to_string(),
                    generics: Vec::new(),
                    body: ast::StructBody::Named(vec![field("x"), field("y")]),
                }),
                vec![derive_attr("Clone")],
            )],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(out.js.contains("Point$.clone = function() {"), "{}", out.js);
        assert!(
            out.js.contains(
                "return { __struct: \"Point\", x: this.x, y: this.y, __proto__: Point$ };"
            ),
            "{}",
            out.js
        );
    }

    #[test]
    fn derive_eq_struct() {
        // #[derive(Eq)] struct Point { x, y } — tag-then-field structural equality.
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Struct(ast::StructDecl {
                    name: "Point".to_string(),
                    generics: Vec::new(),
                    body: ast::StructBody::Named(vec![field("x"), field("y")]),
                }),
                vec![derive_attr("Eq")],
            )],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(
            out.js.contains("Point$.eq = function(a, b) {"),
            "{}",
            out.js
        );
        assert!(
            out.js
                .contains("return a.__struct === b.__struct && a.x === b.x && a.y === b.y;"),
            "{}",
            out.js
        );
    }

    #[test]
    fn derive_display_struct() {
        // #[derive(Display)] struct Point { x, y } — values joined, no type decoration.
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Struct(ast::StructDecl {
                    name: "Point".to_string(),
                    generics: Vec::new(),
                    body: ast::StructBody::Named(vec![field("x"), field("y")]),
                }),
                vec![derive_attr("Display")],
            )],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(
            out.js.contains("Point$.display = function() {"),
            "{}",
            out.js
        );
        assert!(
            out.js
                .contains("return String(this.x) + \", \" + String(this.y);"),
            "{}",
            out.js
        );
    }

    #[test]
    fn derive_eq_enum_dispatches_on_tag() {
        // An enum Eq guards on enum+tag, then compares the matching variant's data.
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Enum(ast::EnumDecl {
                    name: "Shape".to_string(),
                    generics: Vec::new(),
                    variants: vec![
                        ast::Variant {
                            attrs: Vec::new(),
                            name: "Circle".to_string(),
                            name_span: ast::Span::default(),
                            body: ast::VariantBody::Tuple(vec![ast::Type::Path(ast::TypePath {
                                segments: vec![ast::TypeSegment {
                                    name: "number".into(),
                                    args: vec![],
                                }],
                            })]),
                        },
                        ast::Variant {
                            attrs: Vec::new(),
                            name: "Unit".to_string(),
                            name_span: ast::Span::default(),
                            body: ast::VariantBody::Unit,
                        },
                    ],
                }),
                vec![derive_attr("Eq")],
            )],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert!(
            out.js.contains("Shape$.eq = function(a, b) {"),
            "{}",
            out.js
        );
        assert!(
            out.js
                .contains("if (a.__enum !== b.__enum || a.__tag !== b.__tag) {"),
            "{}",
            out.js
        );
        assert!(
            out.js.contains("if (a.__tag === \"Circle\") {"),
            "{}",
            out.js
        );
    }

    /// A `use a::b::c` of a name with a runtime binding lowers to a relative,
    /// `.js`-suffixed ESM import.
    #[test]
    fn use_of_binding_lowers_to_esm_import() {
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Use(ast::UseDecl {
                    path: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    alias: None,
                }),
                Vec::new(),
            )],
        };
        let bindings = HashSet::from(["a::b::c".to_string()]);
        assert_eq!(
            emit(&module, &bindings, &HashSet::new(), &[], &[], &[]).js,
            "import { c } from \"./a/b.js\";\n"
        );
    }

    /// A `use a::b::c as d` of a name with a runtime binding carries the rename
    /// through as `import { c as d }`.
    #[test]
    fn use_alias_of_binding_lowers_to_renamed_esm_import() {
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Use(ast::UseDecl {
                    path: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    alias: Some("d".to_string()),
                }),
                Vec::new(),
            )],
        };
        let bindings = HashSet::from(["a::b::c".to_string()]);
        assert_eq!(
            emit(&module, &bindings, &HashSet::new(), &[], &[], &[]).js,
            "import { c as d } from \"./a/b.js\";\n"
        );
    }

    /// A `use` of a method-bearing type imports its prototype `Thing$` — the
    /// type's JS runtime binding — and nothing more. Dispatch is prototype-backed,
    /// so a value carries its prototype across the `use` boundary with no further
    /// wiring (`lang/DYNAMIC.md` §3.7, §7.1).
    #[test]
    fn use_of_method_type_imports_prototype() {
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Use(ast::UseDecl {
                    path: vec!["lib".to_string(), "Thing".to_string()],
                    alias: None,
                }),
                Vec::new(),
            )],
        };
        let bindings = HashSet::from(["lib::Thing".to_string()]);
        let method_types = HashSet::from(["lib::Thing".to_string()]);
        assert_eq!(
            emit(&module, &bindings, &method_types, &[], &[], &[]).js,
            "import { Thing$ } from \"./lib.js\";\n"
        );
    }

    /// Aliasing a method-bearing type carries the rename onto the prototype name,
    /// `import { Thing$ as Widget$ }` — the alias's constructions reference
    /// `Widget$`. No registry wiring.
    #[test]
    fn use_of_method_type_alias_imports_renamed_prototype() {
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Use(ast::UseDecl {
                    path: vec!["lib".to_string(), "Thing".to_string()],
                    alias: Some("Widget".to_string()),
                }),
                Vec::new(),
            )],
        };
        let bindings = HashSet::from(["lib::Thing".to_string()]);
        let method_types = HashSet::from(["lib::Thing".to_string()]);
        assert_eq!(
            emit(&module, &bindings, &method_types, &[], &[], &[]).js,
            "import { Thing$ as Widget$ } from \"./lib.js\";\n"
        );
    }

    /// A `use` of a type-only item — one absent from the source module's
    /// bindings (a data-only struct/enum or a `pub type` alias) — emits no
    /// import. Its name is erased in JS, so an import would dangle against an
    /// empty export.
    #[test]
    fn use_of_type_only_item_emits_no_import() {
        let module = ast::Module {
            items: vec![item(
                ast::ItemKind::Use(ast::UseDecl {
                    path: vec!["a".to_string(), "b".to_string(), "Color".to_string()],
                    alias: None,
                }),
                Vec::new(),
            )],
        };
        assert_eq!(emit_solo(&module).js, "");
    }

    /// A `pub fn` becomes an exported declaration; a private one stays local.
    #[test]
    fn pub_fn_is_exported_private_fn_is_not() {
        let mut pub_fn = fn_item("f", int("1"));
        pub_fn.vis = ast::Visibility::Public;
        let priv_fn = fn_item("g", int("2"));
        let module = ast::Module {
            items: vec![pub_fn, priv_fn],
        };
        assert_eq!(
            emit_solo(&module).js,
            "export function f() {\n  return 1;\n}\n\
             function g() {\n  return 2;\n}\n"
        );
    }

    /// A `pub const` is exported; a private one is a plain `const`.
    #[test]
    fn pub_const_is_exported() {
        let mk = |vis| ast::Item {
            attrs: Vec::new(),
            vis,
            kind: ast::ItemKind::Const(ast::ConstDecl {
                name: "MAX".to_string(),
                ty: ast::Type::Unit,
                value: int("10"),
            }),
            span: ast::Span::default(),
        };
        let module = ast::Module {
            items: vec![mk(ast::Visibility::Public)],
        };
        assert_eq!(emit_solo(&module).js, "export const MAX = 10;\n");

        let module = ast::Module {
            items: vec![mk(ast::Visibility::Private)],
        };
        assert_eq!(emit_solo(&module).js, "const MAX = 10;\n");
    }

    /// A `pub` type that carries a prototype exports it under `Name$` so a
    /// cross-module trait impl can augment it (`lang/DYNAMIC.md` §7.2).
    #[test]
    fn pub_type_method_slot_is_exported() {
        let mut s = item(
            ast::ItemKind::Struct(ast::StructDecl {
                name: "Point".to_string(),
                generics: Vec::new(),
                body: ast::StructBody::Named(vec![field("x"), field("y")]),
            }),
            vec![derive_attr("Debug")],
        );
        s.vis = ast::Visibility::Public;
        let module = ast::Module { items: vec![s] };
        let out = emit_solo(&module);
        assert!(out.js.contains("export const Point$ = {};"), "{}", out.js);
    }

    /// `module_js_bindings` reports the names that emit a runtime binding: a
    /// `pub fn`, a `pub const`, and a `pub` struct/enum with a method slot.
    /// A data-only `pub` struct and a private item contribute nothing.
    #[test]
    fn module_js_bindings_reports_only_binding_bearing_items() {
        let mut pub_fn = fn_item("f", int("1"));
        pub_fn.vis = ast::Visibility::Public;

        let priv_fn = fn_item("g", int("2"));

        let mut pub_const = item(
            ast::ItemKind::Const(ast::ConstDecl {
                name: "MAX".to_string(),
                ty: ast::Type::Unit,
                value: int("10"),
            }),
            Vec::new(),
        );
        pub_const.vis = ast::Visibility::Public;

        // A data-only `pub` struct: no derive, no impl → no method slot → no
        // binding.
        let mut data_struct = item(
            ast::ItemKind::Struct(ast::StructDecl {
                name: "Plain".to_string(),
                generics: Vec::new(),
                body: ast::StructBody::Named(vec![field("x")]),
            }),
            Vec::new(),
        );
        data_struct.vis = ast::Visibility::Public;

        // A `pub` struct carrying a method slot via `#[derive]` → binding.
        let mut method_struct = item(
            ast::ItemKind::Struct(ast::StructDecl {
                name: "Point".to_string(),
                generics: Vec::new(),
                body: ast::StructBody::Named(vec![field("x"), field("y")]),
            }),
            vec![derive_attr("Debug")],
        );
        method_struct.vis = ast::Visibility::Public;

        let module = ast::Module {
            items: vec![pub_fn, priv_fn, pub_const, data_struct, method_struct],
        };
        let bindings = module_js_bindings(&module);
        assert_eq!(
            bindings,
            HashSet::from(["f".to_string(), "MAX".to_string(), "Point".to_string()])
        );
    }

    /// Two `let _ = …` discards in one scope must not both lower to `let _`: a
    /// second `_` in the same JS scope is a redeclaration (`SyntaxError`). A
    /// wildcard binds no name, so each discard becomes a bare expression
    /// statement run for effect. Regression for #65.
    #[test]
    fn repeated_wildcard_let_lowers_to_bare_expr_stmt() {
        // fn main() { let _ = g(); let _ = g(); }
        let discard = || {
            wildcard_discard(e(ast::ExprKind::Call {
                callee: Box::new(e(ast::ExprKind::Path(vec!["g".to_string()]))),
                args: Vec::new(),
            }))
        };
        let module = ast::Module {
            items: vec![main_fn(vec![discard(), discard()])],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(out.js, "function main() {\n  g();\n  g();\n}\n");
    }

    /// A wildcard discard of a struct literal must paren-wrap the value: a bare
    /// `{ __struct: … };` at statement start parses as a JS block, not an object
    /// literal, so the module fails with `SyntaxError`. Regression for the #65
    /// follow-up — `let _ = g()` (identifier-prefixed) was safe; a `{`-prefixed
    /// initialiser was not.
    #[test]
    fn wildcard_let_discards_struct_literal_paren_wrapped() {
        // struct Point { x, y } fn main() { let _ = Point { x: 1, y: 2 }; }
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
                main_fn(vec![wildcard_discard(e(ast::ExprKind::StructLit {
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
                }))]),
            ],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function main() {\n  ({ __struct: \"Point\", x: 1, y: 2 });\n}\n"
        );
    }

    /// A wildcard discard of an enum-variant construction is the other
    /// `{`-prefixed value class — `{ __enum: … }` — and must paren-wrap for the
    /// same reason. Regression for the #65 follow-up.
    #[test]
    fn wildcard_let_discards_enum_variant_paren_wrapped() {
        // enum Color { Red, Green } fn main() { let _ = Color::Red; }
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
                                name_span: ast::Span::default(),
                                body: ast::VariantBody::Unit,
                            },
                            ast::Variant {
                                attrs: Vec::new(),
                                name: "Green".to_string(),
                                name_span: ast::Span::default(),
                                body: ast::VariantBody::Unit,
                            },
                        ],
                    }),
                    Vec::new(),
                ),
                main_fn(vec![wildcard_discard(e(ast::ExprKind::Path(vec![
                    "Color".to_string(),
                    "Red".to_string(),
                ])))]),
            ],
        };
        let out = emit_solo(&module);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        assert_eq!(
            out.js,
            "function main() {\n  ({ __enum: \"Color\", __tag: \"Red\" });\n}\n"
        );
    }
}
