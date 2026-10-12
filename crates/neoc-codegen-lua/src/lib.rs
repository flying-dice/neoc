//! AST → Lua 5.1 emitter. Follows the lowering table in `lang/DYNAMIC.md`.
//!
//! Split by construct family: items here, expressions in [`expr`], patterns in
//! [`pat`]. The central tension — neoc is expression-oriented, Lua 5.1 is
//! statement-oriented — is bridged with immediately-invoked function
//! expressions (IIFEs) for block-like expressions used for their value.
//!
//! Method calls dispatch through Lua metatables: a struct constructor sets
//! `T.__index = T` and `setmetatable` on each instance, so `recv.m(args)`
//! lowers to the colon call `recv:m(args)` and finds the method on the type
//! table. Enum/struct construction and `match` consult an [`EnumIndex`] built
//! from the module's `enum` declarations (plus the `Result`/`Option` prelude)
//! to tell an enum variant apart from an ordinary value or call.

mod binders;
mod concat;
mod expr;
mod pat;
mod validate;

use std::collections::{HashMap, HashSet};

use neoc_ast as ast;

use crate::expr::VariantShape;

/// Enum declarations indexed for use-site resolution: `enum name → (variant
/// name → payload shape)`. Built from the module's `enum` items and seeded
/// with the `Result`/`Option` prelude.
///
/// Kept crate-local (with `TupleStructIndex`) rather than lifted into a shared
/// crate, so the backend can diverge as its target needs without coupling any
/// future backend through a shared dependency.
#[derive(Default)]
pub(crate) struct EnumIndex {
    enums: HashMap<String, HashMap<String, VariantShape>>,
}

impl EnumIndex {
    /// Build the index from a module plus the `pub` enums it imports, seeding the
    /// `Result`/`Option` prelude.
    ///
    /// Imported enums are registered for *resolution* only — so a `match` on, or
    /// a variant construction of, an enum brought in by `use` lowers correctly —
    /// not for emission; their definitions belong to (and are emitted by) the
    /// module that declares them.
    fn build(module: &ast::Module, imported_enums: &[ImportedEnum<'_>]) -> Self {
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
                Self::register(&mut enums, e);
            }
        }
        for e in imported_enums {
            Self::register(&mut enums, e.decl);
        }
        Self { enums }
    }

    /// Records an enum's variants and their shapes under its name.
    fn register(enums: &mut HashMap<String, HashMap<String, VariantShape>>, e: &ast::EnumDecl) {
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

/// A line-level map from emitted Lua back to the originating neoc source.
///
/// Each entry pairs a one-based emitted-Lua line number with the one-based neoc
/// line it was generated from. The list is sorted by Lua line and sparse: only
/// lines whose origin differs from the preceding entry are recorded, so a
/// lookup takes the neoc line of the nearest preceding entry. Boilerplate with
/// no source node (the `Result`/`Option` prelude, `require` preamble) inherits
/// the origin of the first real node, which is the desired behaviour for a
/// runtime traceback.
///
/// The map is side-band metadata: it never affects the emitted Lua text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineMap {
    entries: Vec<(u32, u32)>,
}

impl LineMap {
    /// Builds a sparse map from a dense per-line origin vector, where
    /// `origins[i]` is the neoc line that produced emitted-Lua line `i + 1`.
    /// Consecutive lines sharing an origin collapse to a single entry.
    fn from_origins(origins: &[u32]) -> Self {
        let mut entries = Vec::new();
        let mut last: Option<u32> = None;
        for (i, &neoc) in origins.iter().enumerate() {
            if last != Some(neoc) {
                // One-based Lua line number.
                let lua = u32::try_from(i + 1).unwrap_or(u32::MAX);
                entries.push((lua, neoc));
                last = Some(neoc);
            }
        }
        Self { entries }
    }

    /// Returns the originating neoc line for a one-based emitted-Lua line, using
    /// the nearest preceding mapped entry when the Lua line has no exact match.
    /// Returns `None` only when the map is empty or the Lua line precedes every
    /// recorded entry.
    #[must_use]
    pub fn neoc_line(&self, lua_line: u32) -> Option<u32> {
        let idx = self.entries.partition_point(|(lua, _)| *lua <= lua_line);
        (idx > 0).then(|| self.entries[idx - 1].1)
    }

    /// Whether the map records no line correspondence — the case for a module
    /// lowered without source text (plain [`emit`]). An empty map returns `None`
    /// for every line.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The result of emitting a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitResult {
    pub lua: String,
    pub diagnostics: Vec<Diagnostic>,
    /// The line map from emitted Lua back to neoc source. Empty unless the
    /// module was emitted through [`emit_with_source`], which supplies the
    /// source text needed to translate byte spans to line numbers.
    pub source_map: LineMap,
}

/// A `use` that brings in a type-namespace-only item (a struct). The struct
/// constructor is still a runtime table (`S { .. }` lowers to `S.new`), so when
/// a value binding in the module spells the same — a value import, a
/// module-level `const` / `fn`, or any `let`, parameter or pattern binding —
/// the import binds a fresh local the value cannot capture
/// (`lang/DYNAMIC.md` §3.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeImport {
    /// The `use` path, module segments then the imported item.
    pub path: Vec<ast::Ident>,
    /// The name the `use` binds: its alias, else the item.
    pub binding: ast::Ident,
}

/// A `pub` enum a module imports by `use`, with whether its defining module
/// gives it a method table. The consumer constructs the enum's variants with
/// the same `setmetatable` the defining module does, so `:method` dispatch
/// resolves on an imported value (`lang/DYNAMIC.md` §3.2).
#[derive(Debug, Clone, Copy)]
pub struct ImportedEnum<'a> {
    /// The enum's declaration.
    pub decl: &'a ast::EnumDecl,
    /// Whether the enum carries a method table; see [`method_enum_names`].
    pub has_methods: bool,
}

/// The names of the enums in `module` that carry a method table: one with a
/// `#[derive(...)]` or any `impl` block targeting it (`lang/DYNAMIC.md` §3.2).
/// A consumer builds [`ImportedEnum::has_methods`] from the defining module with
/// this, so both sides agree on which enums are method-bearing.
#[must_use]
pub fn method_enum_names(module: &ast::Module) -> HashSet<String> {
    let enum_names: HashSet<&str> = module
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ast::ItemKind::Enum(e) => Some(e.name.as_str()),
            _ => None,
        })
        .collect();
    let mut out = HashSet::new();
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Enum(e) if has_derive(&item.attrs) => {
                out.insert(e.name.clone());
            }
            ast::ItemKind::Impl(b) => {
                if let ast::Type::Path(p) = &b.ty
                    && let Some(seg) = p.segments.last()
                    && enum_names.contains(seg.name.as_str())
                {
                    out.insert(seg.name.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// Lower a module to Lua 5.1 source, with no source map.
///
/// Use [`emit_with_source`] when a runtime needs to translate emitted-Lua line
/// numbers back to neoc positions; the emitted Lua text is identical either way.
#[must_use]
pub fn emit(module: &ast::Module) -> EmitResult {
    emit_inner(module, None, &[], &[], &[], &[], false)
}

/// Lower a module to Lua 5.1 source and build a [`LineMap`] from `source`.
///
/// `source` must be the exact `.neoc` text the `module` was parsed from; the
/// node byte spans index into it to derive line numbers. The emitted Lua text
/// is byte-for-byte identical to [`emit`]'s — only the [`EmitResult::source_map`]
/// differs.
#[must_use]
pub fn emit_with_source(module: &ast::Module, source: &str) -> EmitResult {
    emit_inner(module, Some(source), &[], &[], &[], &[], false)
}

/// Like [`emit_with_source`], but also given the `pub` enum declarations the
/// module imports, so `match` and variant construction on an imported enum
/// lower correctly, and the extern items a `use` brought in from another file,
/// so an imported extern type reconstructs its native `require` and lowering in
/// this consumer rather than importing from the defining file (`lang/EXTERN.md`
/// §Lowering). Both kinds of import are used only to resolve those forms; their
/// definitions are emitted by — or, for externs, *not* emitted by — the module
/// that declares them.
///
/// `type_imports` names the `use`s of type-only items. One whose binding the
/// module's value namespace also takes binds a fresh local that struct-literal /
/// update / associated-path references read; the value keeps the plain name. A
/// type the module declares itself is treated the same way
/// (`lang/DYNAMIC.md` §3.7).
///
/// `imported_traits` pairs each `use` binding that names a trait with that
/// trait's declaration, so an `impl` of an imported trait assigns the defaults
/// it omits from the trait's runtime table (`lang/DYNAMIC.md` §3.4).
///
/// `as_module` selects a requireable library (each `pub` runtime binding emitted
/// as a `local` and a trailing `return { name = name, … }`) over a directly
/// runnable script (globals, no trailing `return`). A project build passes
/// `true`; single-file `transpile` passes `false` (`lang/DYNAMIC.md` §3.7).
#[must_use]
pub fn emit_with_imports(
    module: &ast::Module,
    source: &str,
    imported_enums: &[ImportedEnum<'_>],
    imported_externs: &[ast::ExternItem],
    type_imports: &[TypeImport],
    imported_traits: &[(&str, &ast::TraitDecl)],
    as_module: bool,
) -> EmitResult {
    emit_inner(
        module,
        Some(source),
        imported_enums,
        imported_externs,
        type_imports,
        imported_traits,
        as_module,
    )
}

fn emit_inner(
    module: &ast::Module,
    source: Option<&str>,
    imported_enums: &[ImportedEnum<'_>],
    imported_externs: &[ast::ExternItem],
    type_imports: &[TypeImport],
    imported_traits: &[(&str, &ast::TraitDecl)],
    as_module: bool,
) -> EmitResult {
    // A `use`d extern item lowers to its native `require` in *this* consumer, not
    // an import from the defining file (which emits no runtime value for an
    // extern). Merge the imported extern items into the same maps the module's own
    // `extern` blocks fill (`lang/EXTERN.md` §Lowering).
    let mut extern_structs = ast::extern_struct_names(module);
    extern_structs.extend(ast::extern_struct_names_from_items(imported_externs));
    let mut extern_struct_ns = ast::extern_struct_namespaces(module);
    extern_struct_ns.extend(ast::extern_struct_namespaces_from_items(imported_externs));
    let mut extern_target_names = ast::extern_target_names(module);
    extern_target_names.extend(ast::extern_target_names_from_items(imported_externs));
    let mut extern_fn_ns = ast::extern_fn_namespaces(module);
    extern_fn_ns.extend(ast::extern_fn_namespaces_from_items(imported_externs));
    let mut e = Emitter {
        enums: EnumIndex::build(module, imported_enums),
        tuple_structs: TupleStructIndex::build(module),
        traits: ast::trait_index(module),
        trait_defaults: trait_defaults(module, imported_traits),
        line_index: source.map(LineIndex::build),
        forward_decls: forward_declared_fns(module, as_module),
        validation: validate::plan(module),
        extern_structs,
        extern_struct_ns,
        extern_fn_ns,
        extern_target_names,
        imported_externs: imported_externs.to_vec(),
        imported_extern_names: ast::extern_value_names_from_items(imported_externs),
        as_module,
        type_imports: type_imports.to_vec(),
        source_text: source.unwrap_or_default().to_string(),
        ..Emitter::default()
    };
    e.emit_module(module, imported_enums);
    let diagnostics = std::mem::take(&mut e.diagnostics);
    e.finish_with_map(diagnostics)
}

/// The fresh local a captured type's table is bound to, keyed by its binding.
struct TypeRename {
    /// The `use` path of a type import, to pick it out from a value `use` that
    /// shares its binding. Empty for a type the module declares.
    path: Vec<ast::Ident>,
    /// The emitted local holding the type's runtime table.
    local: String,
}

/// `__type_<binding>`, suffixed `_1`, `_2`, … until `is_taken` returns false.
fn fresh_type_local(binding: &str, is_taken: impl Fn(&str) -> bool) -> String {
    let base = format!("__type_{binding}");
    let mut name = base.clone();
    let mut suffix = 0;
    while is_taken(&name) {
        suffix += 1;
        name = format!("{base}_{suffix}");
    }
    name
}

/// Whether `name` occurs in `source` as a whole identifier token.
fn source_mentions_ident(source: &str, name: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices(name).any(|(at, _)| {
        let before = source[..at].chars().next_back();
        let after = source[at + name.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// A precomputed index of line-start byte offsets, for turning a byte offset
/// into a one-based line number in `O(log n)`.
struct LineIndex {
    /// The byte offset at which each line starts. `starts[0]` is always `0`.
    starts: Vec<usize>,
}

impl LineIndex {
    fn build(source: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// The one-based line number a byte offset falls on.
    fn line(&self, offset: usize) -> u32 {
        let idx = self.starts.partition_point(|&s| s <= offset);
        u32::try_from(idx.max(1)).unwrap_or(u32::MAX)
    }
}

/// Accumulates emitted Lua and diagnostics. Output is built line by line at the
/// current indentation level.
#[derive(Default)]
pub(crate) struct Emitter {
    lines: Vec<String>,
    /// Parallel to `lines`: the one-based neoc line each emitted Lua line came
    /// from. Only populated when emitting with a [`LineIndex`]; otherwise stays
    /// empty and no map is produced.
    line_origins: Vec<u32>,
    /// The neoc line of the node currently being emitted. Lines pushed via
    /// [`Emitter::line`] are tagged with it, so emitted Lua maps back to the
    /// construct that produced it.
    cur_neoc_line: u32,
    /// Line-start index over the neoc source, present only when a source map is
    /// being built. `None` leaves `line()` tagging cost-free.
    line_index: Option<LineIndex>,
    indent: usize,
    diagnostics: Vec<Diagnostic>,
    uses_result: bool,
    /// Module-local helpers the derives call; spliced ahead of the body, once
    /// each, in a fixed order.
    derive_helpers: std::collections::BTreeSet<neoc_macros::Helper>,
    /// Whether any `#[test]` fn registered on `__tests` (gates its decl, §6.5).
    uses_tests: bool,
    /// Enum declarations, used to resolve enum-variant paths and to tell a
    /// variant constructor apart from an ordinary call.
    enums: EnumIndex,
    /// Tuple/newtype struct names, used to tell a struct constructor call
    /// (`N(x)` → `N.new(x)`) apart from an ordinary function call.
    tuple_structs: TupleStructIndex,
    /// Enum names that carry a method table (any `impl` block or `#[derive]`).
    /// Variant construction for these enums is `setmetatable`d onto the table so
    /// `:method` dispatch resolves (§3.2).
    method_enums: std::collections::HashSet<String>,
    /// Per-type trait set (transitively closed over supertraits) for the
    /// `__traits` runtime tag — `Type.__traits = { … }` on the type table
    /// (`DYNAMIC.md`). Inert until the runtime trait-enforcement work (#75).
    traits: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    /// The default-bodied method names of each trait in scope — declared here
    /// or `use`d — by binding. An `impl Trait for Type` that omits one assigns
    /// the trait table's function onto the type's table (`DYNAMIC.md` §3.4).
    trait_defaults: HashMap<String, Vec<String>>,
    /// Trait bindings already emitted (a trait table or a `use`), so an impl's
    /// default assignments can read them.
    bound_traits: std::collections::HashSet<String>,
    /// Default assignments of impls that precede their trait's binding, keyed
    /// by that binding; emitted right after it.
    pending_defaults: HashMap<String, Vec<String>>,
    /// The enclosing-loop stack. Each frame records whether the loop body was
    /// restructured for `continue` (the repeat/break idiom) and, if so, the name
    /// of its outer-break flag — see [`expr`].
    loops: Vec<LoopFrame>,
    /// Counter feeding unique outer-break flag names across `continue` loops.
    break_flag_seq: usize,
    /// Loops (by node address) whose loop-carried string appends accumulate in
    /// a table, mapped to the variables to accumulate — see [`concat`].
    concat_plan: HashMap<usize, Vec<String>>,
    /// The `(variable, accumulator)` pairs of the loops being emitted.
    accumulators: Vec<(String, String)>,
    /// Counter feeding unique accumulator names.
    concat_seq: usize,
    /// Counter feeding unique temporaries for the `?` operator (§4.10).
    try_seq: usize,
    /// Counter feeding unique temporaries for compound-assignment places.
    place_seq: usize,
    /// Target-module imports from `extern` blocks (`lang/EXTERN.md`). Each
    /// non-ambient `mod` becomes a `require` preamble line; ambient mods carry a
    /// global-root path and emit no `require`.
    extern_imports: Vec<ast::ExternImport>,
    /// Names declared as extern `struct`s. A struct literal of such a name
    /// lowers to a bare target table (no `__struct` tag), and construction goes
    /// through the target's own form — the extern value *is* the target value.
    extern_structs: std::collections::HashSet<String>,
    /// Maps an extern `struct` declared inside an extern `mod` to its enclosing
    /// `mod`'s `require` binding. A reference to the struct's *value*
    /// (constructor, associated call) qualifies through this binding
    /// (`sqlite.Database.new(a)`) so it reads the imported module table, not a
    /// nil bare global (`lang/EXTERN.md` §Lowering).
    extern_struct_ns: std::collections::HashMap<String, String>,
    /// Maps a free extern `fn` declared inside an extern `mod` to its enclosing
    /// `mod`'s binding. A call to the fn qualifies through this binding
    /// (`sqlite.open(a)`, `Bun.serve(a)`) so it reads the imported module table /
    /// global root rather than a nil bare global (`lang/EXTERN.md` §Lowering). A
    /// fn with no enclosing `mod` keeps its bare name. Populated from both the
    /// module's own `extern` blocks and `use`-imported items.
    extern_fn_ns: std::collections::HashMap<String, String>,
    /// Maps an extern `struct` / free `fn` with `#[extern(name = "X")]` to its
    /// target identifier `X`; every lowered reference uses `X`, not the neoc name
    /// (`lang/EXTERN.md` §Attributes).
    extern_target_names: std::collections::HashMap<String, String>,
    /// Private free functions referenced before their definition. They are
    /// declared `local` up front and assigned in place, so a caller emitted
    /// earlier closes over the (later-assigned) local rather than a nil global.
    /// A plain `local function` is only in scope after its line.
    forward_decls: std::collections::HashSet<String>,
    /// Type names whose `Type::member` path is a table-field access: a local
    /// `impl` target, or a type brought in by `use`. At a call site
    /// `Type::assoc_fn()` this keeps the `Type.` qualifier instead of dropping
    /// to the bare member name (an unbound global).
    assoc_types: std::collections::HashSet<String>,
    /// Runtime type-enforcement plan: the validators to emit at module top and
    /// the per-site checks at drift points (`DYNAMIC.md` §9, `STATIC.md` §10).
    validation: validate::Validation,
    /// Extern items a `use` brought in from another file. Their `mod` imports are
    /// merged into [`extern_imports`](Self::extern_imports) so the consumer emits
    /// the same native `require` the defining module would, reconstructed from the
    /// imported item's `#[extern(...)]` metadata (`lang/EXTERN.md` §Lowering).
    imported_externs: Vec<ast::ExternItem>,
    /// The names of the `use`-imported extern items (structs and fns). A `use` of
    /// one of these emits **no** `require("<defining file>")`: the value is reached
    /// through its native `require` and `mod`-qualified reference, never the
    /// defining module's emitted output (which holds no runtime value for it).
    imported_extern_names: std::collections::HashSet<String>,
    /// Type-only imports, from the project. A subset becomes `type_renames`.
    type_imports: Vec<TypeImport>,
    /// The module's source text, to keep fresh locals clear of every identifier
    /// the user wrote.
    source_text: String,
    /// Types whose table a value binding of the same name would capture,
    /// binding → fresh local. The declaration (or the `use`) binds the fresh
    /// local, and every reference to the table (`S { .. }`, `S::member`,
    /// `impl` methods, derives) reads it; the plain name stays the value
    /// binding (`lang/DYNAMIC.md` §3.7).
    type_renames: HashMap<String, TypeRename>,
    /// Whether this module is emitted as a requireable library (project build)
    /// rather than a directly runnable script (single-file). When `true`, each
    /// `pub` runtime binding is a `local` and the module ends with a
    /// `return { name = name, … }` export table (`lang/DYNAMIC.md` §3.7).
    as_module: bool,
}

/// A loop being emitted. When `break_flag` is set the loop uses the
/// repeat/break idiom (because its body contains `continue`); `continue` then
/// lowers to a `break` of the inner `repeat` and a neoc `break` sets the flag
/// and breaks out so the outer loop also exits.
pub(crate) struct LoopFrame {
    pub break_flag: Option<String>,
}

/// The line declaring a module's `__tests` table (§6.5), emitted only when the
/// module has a `#[test]` fn — so its presence marks a test-bearing module.
pub const TESTS_TABLE_DECL: &str = "local __tests = {}";

const RESULT_PRELUDE: &str = r#"local Option = {}
Option.__index = Option
local Result = {}
Result.__index = Result
local Ok   = function(v) return setmetatable({ __enum = "Result", __tag = "Ok", v }, Result) end
local Err  = function(e) return setmetatable({ __enum = "Result", __tag = "Err", e }, Result) end
local Some = function(v) return setmetatable({ __enum = "Option", __tag = "Some", v }, Option) end
local None = setmetatable({ __enum = "Option", __tag = "None" }, Option)
function Option.is_some(self) return self.__tag == "Some" end
function Option.is_none(self) return self.__tag == "None" end
function Option.unwrap(self)
  if self.__tag == "Some" then return self[1] end
  error("called unwrap on a None value", 0)
end
function Option.unwrap_or(self, default)
  if self.__tag == "Some" then return self[1] else return default end
end
function Option.map(self, f)
  if self.__tag == "Some" then return Some(f(self[1])) else return self end
end
function Option.ok_or(self, err)
  if self.__tag == "Some" then return Ok(self[1]) else return Err(err) end
end
function Result.is_ok(self) return self.__tag == "Ok" end
function Result.is_err(self) return self.__tag == "Err" end
function Result.unwrap(self)
  if self.__tag == "Ok" then return self[1] end
  error("called unwrap on an Err value", 0)
end
function Result.unwrap_err(self)
  if self.__tag == "Err" then return self[1] end
  error("called unwrap_err on an Ok value", 0)
end
function Result.map(self, f)
  if self.__tag == "Ok" then return Ok(f(self[1])) else return self end
end
function Result.map_err(self, f)
  if self.__tag == "Err" then return Err(f(self[1])) else return self end
end"#;

impl Emitter {
    /// The declaration keyword prefix for a top-level binding of visibility
    /// `vis`. A private item is always `local`; a `pub` item is a global in the
    /// single-file script form but a `local` in a library build (`as_module`),
    /// where the export table re-surfaces it for `require` (`lang/DYNAMIC.md`
    /// §3.7).
    fn decl_prefix(&self, vis: ast::Visibility) -> &'static str {
        if self.as_module || vis != ast::Visibility::Public {
            "local "
        } else {
            ""
        }
    }

    /// The `pub` items that produce a runtime binding, in source order: the names
    /// the library export table (`return { name = name, … }`) carries. A `pub fn`,
    /// `pub const`, any `pub` struct, and a method-bearing `pub` enum each emit a
    /// runtime value a consumer's `require("mod").name` resolves. A plain `pub`
    /// enum (no methods) emits no standalone value — its variants are inline
    /// tagged tables resolved through the consumer's imported-enum index — so it
    /// is not exported; type aliases, traits, and `extern` carry no value either.
    fn exported_names(&self, module: &ast::Module) -> Vec<String> {
        let mut names = Vec::new();
        for item in &module.items {
            if item.vis != ast::Visibility::Public {
                continue;
            }
            match &item.kind {
                ast::ItemKind::Fn(f) => names.push(f.sig.name.clone()),
                ast::ItemKind::Const(c) => names.push(c.name.clone()),
                ast::ItemKind::Struct(s) => names.push(s.name.clone()),
                ast::ItemKind::Trait(t) if !t.default_names().is_empty() => {
                    names.push(t.name.clone());
                }
                ast::ItemKind::Enum(e) if self.enum_has_methods(&e.name) => {
                    names.push(e.name.clone());
                }
                _ => {}
            }
        }
        names
    }

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
            // Keep the origin vector in lockstep with `lines` so the map aligns
            // one-to-one. Only populated when a source map is being built.
            if self.line_index.is_some() {
                self.line_origins.push(self.cur_neoc_line);
            }
        }
    }

    /// Set the current neoc line from a node span, so lines emitted while
    /// `f` runs map back to it. The previous origin is restored afterwards, so
    /// nested nodes don't leak their line to following siblings. A no-op (other
    /// than running `f`) when no source map is being built.
    pub(crate) fn at_span<R>(&mut self, span: ast::Span, f: impl FnOnce(&mut Self) -> R) -> R {
        let Some(index) = self.line_index.as_ref() else {
            return f(self);
        };
        let saved = self.cur_neoc_line;
        self.cur_neoc_line = index.line(span.start);
        let r = f(self);
        self.cur_neoc_line = saved;
        r
    }

    pub(crate) fn indented<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.indent += 1;
        let r = f(self);
        self.indent -= 1;
        r
    }

    /// Assemble the final Lua text and its line map. The prelude and import
    /// preamble are prepended here; their origins inherit the first real node's
    /// line so a traceback into boilerplate still points somewhere sensible.
    fn finish_with_map(mut self, diagnostics: Vec<Diagnostic>) -> EmitResult {
        let tracking = self.line_index.is_some();
        // The neoc line the preamble (prelude / imports) maps to: the first
        // recorded body origin, or 1 when the body produced none.
        let preamble_origin = self.line_origins.first().copied().unwrap_or(1);

        // Derive helpers (`__eq`, `__debug`) sit lowest: declared before any
        // type whose derived methods call them.
        if !self.derive_helpers.is_empty() {
            let mut out: Vec<String> = Vec::new();
            for h in &self.derive_helpers {
                out.extend(h.source().split('\n').map(str::to_string));
                out.push(String::new());
            }
            let added = out.len();
            out.append(&mut self.lines);
            self.lines = out;
            self.prepend_origins(tracking, added, preamble_origin);
        }
        // The module `__tests` table a `#[test]` fn registers on (§6.5), declared
        // ahead of its registrations and of any prelude/imports prepended below.
        if self.uses_tests {
            let mut out = vec![TESTS_TABLE_DECL.to_string(), String::new()];
            out.append(&mut self.lines);
            self.lines = out;
            self.prepend_origins(tracking, 2, preamble_origin);
        }
        if self.uses_result {
            let prelude_lines = RESULT_PRELUDE.split('\n').count() + 1;
            let mut out = vec![RESULT_PRELUDE.to_string(), String::new()];
            out.append(&mut self.lines);
            self.lines = out;
            self.prepend_origins(tracking, prelude_lines, preamble_origin);
        }
        // Extern `mod` imports (`lang/EXTERN.md`): a non-ambient `mod` binds its
        // handle with `require`; an ambient `mod`'s reference resolves against
        // its global-root path, so it binds the local name to that chain.
        let require_imports: Vec<&ast::ExternImport> = self
            .extern_imports
            .iter()
            .filter(|i| !i.ambient || i.local_name != i.path)
            .collect();
        if !require_imports.is_empty() {
            let mut out = Vec::new();
            for imp in &require_imports {
                if imp.ambient {
                    // Ambient: the handle is a global member-access chain.
                    out.push(format!("local {} = {}", imp.local_name, imp.path));
                } else {
                    out.push(format!(
                        "local {} = require(\"{}\")",
                        imp.local_name, imp.path
                    ));
                }
            }
            out.push(String::new());
            let added = out.len();
            out.append(&mut self.lines);
            self.lines = out;
            self.prepend_origins(tracking, added, preamble_origin);
        }

        let source_map = if tracking {
            LineMap::from_origins(&self.line_origins)
        } else {
            LineMap::default()
        };

        let mut s = self.lines.join("\n");
        if !s.is_empty() {
            s.push('\n');
        }
        EmitResult {
            lua: s,
            diagnostics,
            source_map,
        }
    }

    /// Prepend `count` origins (all `origin`) to keep `line_origins` aligned with
    /// `lines` after a preamble is spliced in front. A no-op when not tracking.
    fn prepend_origins(&mut self, tracking: bool, count: usize, origin: u32) {
        if tracking {
            let mut prefix = vec![origin; count];
            prefix.append(&mut self.line_origins);
            self.line_origins = prefix;
        }
    }

    fn emit_module(&mut self, module: &ast::Module, imported_enums: &[ImportedEnum<'_>]) {
        self.collect_method_enums(module, imported_enums);
        self.collect_assoc_types(module);
        self.allocate_type_renames(module);
        self.extern_imports = ast::extern_imports(module);
        // A `use`d extern item emits its native `require` here, in the consumer,
        // not an import from the defining file (`lang/EXTERN.md` §Lowering).
        let imported = std::mem::take(&mut self.imported_externs);
        self.extern_imports
            .extend(ast::extern_imports_from_items(&imported));
        // A `mod`'s neoc binding heads a `mod::item` path; keep its qualifier so
        // the access reads the imported handle (`rusqlite.open`), not a bare
        // global. Ambient mods are reached through their global-root chain.
        for imp in &self.extern_imports {
            self.assoc_types.insert(imp.local_name.clone());
        }
        // An extern struct's `T::assoc(a)` is a static/namespaced target call;
        // keep the `T.` qualifier so it reads the target's own table.
        for name in self.extern_structs.clone() {
            self.assoc_types.insert(name);
        }
        // Hoist the locals of forward-referenced private functions so a caller
        // emitted earlier captures the local, not a nil global.
        if !self.forward_decls.is_empty() {
            let mut names: Vec<String> = self.forward_decls.iter().cloned().collect();
            names.sort_unstable();
            self.line(format!("local {}", names.join(", ")));
        }
        // Runtime type-enforcement preamble: the validators and their helpers,
        // emitted before any `.new` or `pub fn` entry that calls them (§9.1).
        for block in std::mem::take(&mut self.validation.preamble) {
            self.line(block);
        }
        for item in &module.items {
            self.emit_item(item);
        }
        // A library build closes with an export table so a consumer's
        // `require("mod").name` resolves (`lang/DYNAMIC.md` §3.7). A single-file
        // script omits it — the chunk runs directly, and a trailing `return`
        // before an appended `main()` call would be a Lua syntax error.
        if self.as_module {
            let exports = self.exported_names(module);
            if exports.is_empty() {
                self.line("return {}");
            } else {
                let fields = exports
                    .iter()
                    .map(|n| format!("{n} = {}", self.type_local(n)))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.line(format!("return {{ {fields} }}"));
            }
        }
    }

    /// Pick the types whose table a value binding would capture and give each a
    /// fresh local (`lang/DYNAMIC.md` §3.7): every struct, method-bearing enum
    /// and default-bearing trait the module declares, plus each type-only
    /// import, whose name the module also binds as a value — via `let`, a
    /// parameter, a pattern, a closure parameter, a `const` / `fn`, or a value
    /// `use`. Each fresh local avoids every identifier in the source and every
    /// local allocated before it.
    fn allocate_type_renames(&mut self, module: &ast::Module) {
        let binders = binders::value_binders(module);
        let uses: Vec<&ast::UseDecl> = module
            .items
            .iter()
            .filter_map(|i| match &i.kind {
                ast::ItemKind::Use(u) => Some(u),
                _ => None,
            })
            .collect();
        let use_binding = |u: &ast::UseDecl| u.alias.clone().or_else(|| u.path.last().cloned());
        let mut candidates: Vec<(String, Vec<ast::Ident>)> = Vec::new();
        for item in &module.items {
            let name = match &item.kind {
                ast::ItemKind::Struct(s) => &s.name,
                ast::ItemKind::Enum(e) if self.enum_has_methods(&e.name) => &e.name,
                ast::ItemKind::Trait(t) if !t.default_names().is_empty() => &t.name,
                _ => continue,
            };
            candidates.push((name.clone(), Vec::new()));
        }
        candidates.extend(
            self.type_imports
                .iter()
                .map(|t| (t.binding.clone(), t.path.clone())),
        );
        let mut allocated = HashSet::new();
        for (binding, path) in candidates {
            // A value `use` shares the binding when it is a different `use`.
            let value_use = uses
                .iter()
                .any(|u| use_binding(u).as_deref() == Some(&binding) && u.path != path);
            if !binders.contains(&binding) && !value_use {
                continue;
            }
            let local = fresh_type_local(&binding, |name| {
                allocated.contains(name) || source_mentions_ident(&self.source_text, name)
            });
            allocated.insert(local.clone());
            self.type_renames
                .insert(binding, TypeRename { path, local });
        }
    }

    /// Record every enum that needs a method table: one with a `#[derive(...)]`
    /// or any `impl` block targeting it, plus the always method-bearing prelude
    /// enums `Option`/`Result` (`DYNAMIC.md` §7). Variant construction for these
    /// enums is `setmetatable`d so `:method` dispatch resolves (§3.2).
    fn collect_method_enums(&mut self, module: &ast::Module, imported_enums: &[ImportedEnum<'_>]) {
        self.method_enums.extend(method_enum_names(module));
        // An imported enum's table is built by its defining module; the consumer
        // only needs to know it has one to `setmetatable` its variants onto it.
        self.method_enums.extend(
            imported_enums
                .iter()
                .filter(|e| e.has_methods)
                .map(|e| e.decl.name.clone()),
        );
        // The prelude enums are method-bearing (`DYNAMIC.md` §3.2, §7), so inline
        // variant construction (`Option::Some(x)`, `Option::None`) must be
        // `setmetatable`d onto the prelude's metatable to match the constructor
        // functions. The predicate is consulted only at construction sites, which
        // exist only when the prelude is emitted.
        self.method_enums.insert("Option".to_string());
        self.method_enums.insert("Result".to_string());
    }

    /// Record every type name whose `Type::member` access reads a table field:
    /// a local `impl` target, or a type imported by `use`. A 2-segment path
    /// `Type::assoc_fn` then keeps its `Type.` qualifier (the value lives on the
    /// type's table); a path whose head is none of these still drops to its last
    /// segment, as before. An imported *value* (a `use`d fn/const) can never
    /// head a multi-segment path, so recording every `use` binding is safe.
    fn collect_assoc_types(&mut self, module: &ast::Module) {
        for item in &module.items {
            match &item.kind {
                ast::ItemKind::Impl(b) => {
                    if let ast::Type::Path(p) = &b.ty {
                        if let Some(seg) = p.segments.last() {
                            self.assoc_types.insert(seg.name.clone());
                        }
                    }
                }
                ast::ItemKind::Use(u) => {
                    if let Some((last, _)) = u.path.split_last() {
                        let binding = u.alias.as_deref().unwrap_or(last);
                        self.assoc_types.insert(binding.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether `name` is an enum with a method table (§3.2). Used at variant
    /// construction sites to decide whether to `setmetatable` the value.
    pub(crate) fn enum_has_methods(&self, name: &str) -> bool {
        self.method_enums.contains(name)
    }

    /// Whether `name` is a tuple/newtype struct, so a call to it constructs a
    /// value through its generated `.new` (§4.4) rather than calling a function.
    pub(crate) fn is_tuple_struct(&self, name: &str) -> bool {
        self.tuple_structs.contains(name)
    }

    fn emit_item(&mut self, item: &ast::Item) {
        self.at_span(item.span, |e| e.emit_item_kind(item));
    }

    fn emit_item_kind(&mut self, item: &ast::Item) {
        match &item.kind {
            ast::ItemKind::Fn(f) => {
                self.emit_fn(None, f, item.vis);
                // §6.5: a `#[test]` fn additionally registers on `__tests`; the
                // table itself is declared in the preamble (`finish_with_map`).
                if ast::is_test(&item.attrs) {
                    self.uses_tests = true;
                    self.line(format!("__tests[\"{0}\"] = {0}", f.sig.name));
                }
            }
            ast::ItemKind::Struct(s) => {
                self.emit_struct(s, item.vis);
                self.emit_derives(&item.attrs, neoc_macros::DeriveTarget::Struct(s));
                self.emit_traits_tag(&s.name);
            }
            ast::ItemKind::Const(c) => self.emit_const(c, item.vis),
            ast::ItemKind::Impl(b) => self.emit_impl(b),
            ast::ItemKind::Use(u) => self.emit_use(u),
            ast::ItemKind::Enum(e) => {
                // An enum with methods (any `impl` or `#[derive]`) gets a table
                // doubling as the method table and shared metatable, so variant
                // values can dispatch `:method` (§3.2). A plain enum emits no
                // standalone output — its variants are inline tagged tables.
                if self.enum_has_methods(&e.name) {
                    self.emit_enum_table(&e.name, item.vis);
                    self.emit_derives(&item.attrs, neoc_macros::DeriveTarget::Enum(e));
                    self.emit_traits_tag(&e.name);
                }
            }
            // No inline Lua output: aliases erase; an `extern` block describes
            // the target surface — its `mod` imports emit in the preamble, its
            // structs/fns/methods are the target's own values.
            ast::ItemKind::Trait(t) => self.emit_trait(t, item.vis),
            ast::ItemKind::TypeAlias(_) | ast::ItemKind::Extern(_) => {}
        }
    }

    /// Emit a trait's default-bodied methods onto its table (`DYNAMIC.md`
    /// §3.4). A trait without defaults emits nothing.
    fn emit_trait(&mut self, t: &ast::TraitDecl, vis: ast::Visibility) {
        let defaults = t.default_methods();
        if defaults.is_empty() {
            return;
        }
        let local = self.decl_prefix(vis);
        let table = self.type_local(&t.name).to_string();
        self.line(format!("{local}{table} = {{}}"));
        for f in &defaults {
            self.emit_fn(Some(&table), f, ast::Visibility::Public);
        }
        self.bind_trait(&t.name);
    }

    /// Record `binding` as emitted and flush the default assignments waiting
    /// on it.
    fn bind_trait(&mut self, binding: &str) {
        self.bound_traits.insert(binding.to_string());
        for l in self.pending_defaults.remove(binding).unwrap_or_default() {
            self.line(l);
        }
    }

    /// Emit the methods generated by each `#[derive(...)]` on a type, right
    /// after its constructor so they land on the type's table.
    fn emit_derives(&mut self, attrs: &[ast::Attribute], target: neoc_macros::DeriveTarget) {
        let type_name = match target {
            neoc_macros::DeriveTarget::Struct(s) => &s.name,
            neoc_macros::DeriveTarget::Enum(e) => &e.name,
        };
        let table = self.type_local(type_name).to_string();
        for name in ast::derive_names(attrs) {
            match neoc_macros::derive_on(&name, target, &table) {
                Ok(derived) => {
                    self.derive_helpers.extend(derived.helpers);
                    for l in derived.lines {
                        self.line(l);
                    }
                }
                Err(message) => self.error(message),
            }
        }
    }

    fn emit_use(&mut self, u: &ast::UseDecl) {
        let Some((last, prefix)) = u.path.split_last() else {
            return;
        };
        // A `use`d extern item is reached through its *native* `require` (emitted
        // in the preamble from the imported `#[extern]` metadata) and the
        // `mod`-qualified value reference, not through the defining file's emitted
        // module — an extern emits no runtime value there (`lang/EXTERN.md`
        // §Lowering). So emit no `require("<defining file>")` for it.
        if self.imported_extern_names.contains(last) {
            return;
        }
        let binding = u.alias.as_deref().unwrap_or(last);
        let local = self
            .type_renames
            .get(binding)
            .filter(|r| r.path == u.path)
            .map_or(binding, |r| r.local.as_str());
        let module = prefix.join(".");
        self.line(format!("local {local} = require(\"{module}\").{last}"));
        self.bind_trait(binding);
    }

    fn emit_const(&mut self, c: &ast::ConstDecl, vis: ast::Visibility) {
        let value = self.emit_expr(&c.value);
        let kw = self.decl_prefix(vis);
        self.line(format!("{kw}{} = {value}", c.name));
    }

    fn emit_struct(&mut self, s: &ast::StructDecl, vis: ast::Visibility) {
        let local = self.decl_prefix(vis);
        let t = self.type_local(&s.name).to_string();
        match &s.body {
            ast::StructBody::Unit => {
                self.line(format!("{local}{t} = {{ __struct = \"{}\" }}", s.name));
                // The unit struct is also a value; keep its plain name bound so a
                // reference reads it unless a nearer binding shadows it.
                if t != s.name {
                    self.line(format!("local {} = {t}", s.name));
                }
            }
            ast::StructBody::Tuple(_) => {
                self.line(format!("{local}{t} = {{}}"));
                self.line(format!("{t}.__index = {t}"));
                self.line(format!("function {t}.new(value)"));
                self.indented(|e| {
                    let built =
                        format!("setmetatable({{ __struct = \"{}\", value }}, {t})", s.name);
                    e.line(format!("return {}", e.construct_check(&s.name, &built)));
                });
                self.line("end");
            }
            ast::StructBody::Named(_) => {
                self.line(format!("{local}{t} = {{}}"));
                self.line(format!("{t}.__index = {t}"));
                self.line(format!("function {t}.new(fields)"));
                self.indented(|e| {
                    e.line(format!("fields.__struct = \"{}\"", s.name));
                    let built = format!("setmetatable(fields, {t})");
                    e.line(format!("return {}", e.construct_check(&s.name, &built)));
                });
                self.line("end");
            }
        }
    }

    /// Wrap a freshly constructed struct value in its validator (`DRIFT-CONSTRUCT`,
    /// `DYNAMIC.md` §9.3): `__check_T(<built>, "T")`, which validates every field
    /// deep and returns the value. Every named/tuple struct is seeded into the
    /// validator set, so a construction site always has a validator to call.
    fn construct_check(&self, name: &str, built: &str) -> String {
        debug_assert!(
            self.validation.has_validator(name),
            "every constructed struct is seeded into the validator set"
        );
        format!("__check_{name}({built}, \"{name}\")")
    }

    /// Emit the `Type.__traits = { … }` runtime tag on a type's table, listing
    /// every trait it implements (its supertraits included). On the type table —
    /// the metatable for tuple/named structs and method-bearing enums, and the
    /// value itself for a unit struct — so `value.__traits` resolves for any
    /// instance. Skipped for a type implementing no trait. Inert until #75.
    fn emit_traits_tag(&mut self, name: &str) {
        let Some(traits) = self.traits.get(name) else {
            return;
        };
        if traits.is_empty() {
            return;
        }
        let list = traits
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let t = self.type_local(name).to_string();
        self.line(format!("{t}.__traits = {{ {list} }}"));
    }

    /// Emit the method table for an enum that has `impl`/derived methods. The
    /// table doubles as the shared metatable: `E.__index = E` lets variant
    /// values resolve `:method` once they are `setmetatable`d at construction
    /// (§3.2).
    fn emit_enum_table(&mut self, name: &str, vis: ast::Visibility) {
        let local = self.decl_prefix(vis);
        let t = self.type_local(name).to_string();
        self.line(format!("{local}{t} = {{}}"));
        self.line(format!("{t}.__index = {t}"));
    }

    fn emit_impl(&mut self, b: &ast::ImplBlock) {
        let ast::Type::Path(p) = &b.ty else {
            self.error("impl target must be a named type");
            return;
        };
        let Some(target) = p.segments.last().map(|s| s.name.clone()) else {
            return;
        };
        let table = self.type_local(&target).to_string();
        for f in &b.items {
            // A trait impl binds the trait's associated types; project them into
            // each method signature so a `Self::Item` return renders as the bound
            // type rather than a dangling type name (`STATIC.md` §3.4).
            let f = f.project_assoc(&b.assoc_bindings);
            self.emit_fn(Some(&table), &f, ast::Visibility::Public);
        }
        self.emit_inherited_defaults(b, &target);
    }

    /// Assign each default of `b`'s trait that `b` does not override from the
    /// trait's table onto `target`'s, in the trait's declaration order — or,
    /// when the trait's binding comes later in the module, defer them until it
    /// is emitted (`DYNAMIC.md` §3.4).
    fn emit_inherited_defaults(&mut self, b: &ast::ImplBlock, target: &str) {
        let Some(trait_name) = b.trait_.as_ref().and_then(|p| p.segments.last()) else {
            return;
        };
        let trait_name = &trait_name.name;
        let target_table = self.type_local(target).to_string();
        let trait_table = self.type_local(trait_name).to_string();
        let lines: Vec<String> = self
            .trait_defaults
            .get(trait_name)
            .into_iter()
            .flatten()
            .filter(|d| !b.items.iter().any(|f| f.sig.name == **d))
            .map(|d| format!("{target_table}.{d} = {trait_table}.{d}"))
            .collect();
        if self.bound_traits.contains(trait_name) {
            for l in lines {
                self.line(l);
            }
        } else {
            self.pending_defaults
                .entry(trait_name.clone())
                .or_default()
                .extend(lines);
        }
    }

    /// Emit a function. `target` is the type name when this is a method or
    /// associated function inside an impl.
    fn emit_fn(&mut self, target: Option<&str>, f: &ast::FnDecl, vis: ast::Visibility) {
        let param_list = f
            .sig
            .params
            .iter()
            .map(|p| match p {
                ast::Param::Receiver => "self".to_string(),
                ast::Param::Named { name, .. } => name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        // In a module (library) build a `pub` free fn is a `local` like a private
        // one — globals are reserved for the single-file script form. The export
        // table (`emit_module`) re-surfaces it for `require`.
        let module_local = self.as_module || vis != ast::Visibility::Public;
        let header = match target {
            Some(t) => format!("function {t}.{}({param_list})", f.sig.name),
            None if !module_local => {
                format!("function {}({param_list})", f.sig.name)
            }
            // A forward-referenced fn: assign the pre-declared `local` (hoisted in
            // `emit_module`) instead of declaring it here, where a `local function`
            // would be invisible to earlier callers.
            None if self.forward_decls.contains(&f.sig.name) => {
                format!("{} = function({param_list})", f.sig.name)
            }
            None => format!("local function {}({param_list})", f.sig.name),
        };
        self.line(header);
        self.indented(|e| {
            // Untyped boundary params: a `pub` free function is reachable from
            // the host, so it validates each typed parameter at entry
            // (`DRIFT-PARAM`, `STATIC.md` §10.4). Methods and internal-only
            // functions are reached only by vouched neoc callers (`STABLE-PARAM`).
            if target.is_none() && vis == ast::Visibility::Public {
                for check in e.validation.param_checks(&f.sig.name, &f.sig.params) {
                    e.line(check);
                }
            }
            e.emit_block_as_body(&f.body);
        });
        self.line("end");
    }
}

/// The default-bodied method names of every trait in scope, by binding: each
/// trait declared in `module`, then each `use`d one in `imported`. Traits
/// without a default are absent.
fn trait_defaults(
    module: &ast::Module,
    imported: &[(&str, &ast::TraitDecl)],
) -> HashMap<String, Vec<String>> {
    let declared = module.items.iter().filter_map(|item| match &item.kind {
        ast::ItemKind::Trait(t) => Some((t.name.as_str(), t)),
        _ => None,
    });
    declared
        .chain(imported.iter().copied())
        .map(|(binding, t)| (binding.to_string(), t.default_names()))
        .filter(|(_, defaults)| !defaults.is_empty())
        .collect()
}

/// Whether `attrs` contains any `#[derive(...)]`.
fn has_derive(attrs: &[ast::Attribute]) -> bool {
    attrs
        .iter()
        .flat_map(|a| &a.items)
        .any(|i| i.name == "derive")
}

/// The free functions referenced by an item that precedes their definition.
/// Those must be declared `local` up front (see [`Emitter`]'s `forward_decls`);
/// a `local function` is only visible from its own line on, so an earlier caller
/// would otherwise close over a nil global.
///
/// In a single-file script (`as_module` false) `pub` functions emit as globals
/// (visible everywhere), so only private fns need hoisting. In a library build
/// (`as_module` true) every free fn is a `local`, so a forward-referenced `pub`
/// fn needs hoisting too. Methods are table fields and never need this.
fn forward_declared_fns(
    module: &ast::Module,
    as_module: bool,
) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut def_index: HashMap<&str, usize> = HashMap::new();
    for (i, item) in module.items.iter().enumerate() {
        if let ast::ItemKind::Fn(f) = &item.kind
            && (as_module || item.vis == ast::Visibility::Private)
        {
            def_index.entry(f.sig.name.as_str()).or_insert(i);
        }
    }
    let mut forward = HashSet::new();
    for (i, item) in module.items.iter().enumerate() {
        let mut used = HashSet::new();
        item_refs(item, &mut used);
        for name in used {
            if def_index.get(name).is_some_and(|&def| def > i) {
                forward.insert(name.to_string());
            }
        }
    }
    forward
}

/// Collects the names an item's bodies reference (path heads), for forward-
/// reference detection.
fn item_refs<'a>(item: &'a ast::Item, used: &mut std::collections::HashSet<&'a str>) {
    match &item.kind {
        ast::ItemKind::Fn(f) => block_refs(&f.body, used),
        ast::ItemKind::Const(c) => expr_refs(&c.value, used),
        ast::ItemKind::Impl(b) => {
            for method in &b.items {
                block_refs(&method.body, used);
            }
        }
        ast::ItemKind::Trait(t) => {
            for trait_item in &t.items {
                if let Some(body) = &trait_item.default {
                    block_refs(body, used);
                }
            }
        }
        _ => {}
    }
}

fn block_refs<'a>(block: &'a ast::Block, used: &mut std::collections::HashSet<&'a str>) {
    for stmt in &block.stmts {
        match &stmt.kind {
            ast::StmtKind::Let(l) => {
                if let Some(value) = &l.value {
                    expr_refs(value, used);
                }
            }
            ast::StmtKind::Expr(e) => expr_refs(e, used),
            ast::StmtKind::Item(item) => item_refs(item, used),
        }
    }
    if let Some(value) = &block.value {
        expr_refs(value, used);
    }
}

fn expr_refs<'a>(expr: &'a ast::Expr, used: &mut std::collections::HashSet<&'a str>) {
    use ast::ExprKind::{
        Assign, Binary, Block, Bool, Break, Call, Closure, Continue, Field, Float, For, If, Index,
        Int, List, Loop, Match, MethodCall, Path, Range, Return, Str, StructLit, Try, Unary, Unit,
        While,
    };
    match &expr.kind {
        Path(segs) => {
            if let Some(head) = segs.first() {
                used.insert(head.as_str());
            }
        }
        Binary { lhs, rhs, .. } => {
            expr_refs(lhs, used);
            expr_refs(rhs, used);
        }
        Unary { operand, .. } | Try(operand) => expr_refs(operand, used),
        Range { start, end, .. } => {
            expr_refs(start, used);
            expr_refs(end, used);
        }
        Call { callee, args } => {
            expr_refs(callee, used);
            for a in args {
                expr_refs(a, used);
            }
        }
        MethodCall { receiver, args, .. } => {
            expr_refs(receiver, used);
            for a in args {
                expr_refs(a, used);
            }
        }
        Field { base, .. } => expr_refs(base, used),
        Index { base, index } => {
            expr_refs(base, used);
            expr_refs(index, used);
        }
        StructLit { fields, base, .. } => {
            for field in fields {
                if let Some(value) = &field.value {
                    expr_refs(value, used);
                }
            }
            if let Some(base) = base {
                expr_refs(base, used);
            }
        }
        List(items) => {
            for x in items {
                expr_refs(x, used);
            }
        }
        Assign { target, value, .. } => {
            expr_refs(target, used);
            expr_refs(value, used);
        }
        If { cond, then, else_ } => {
            expr_refs(cond, used);
            block_refs(then, used);
            if let Some(e) = else_ {
                expr_refs(e, used);
            }
        }
        Match { scrutinee, arms } => {
            expr_refs(scrutinee, used);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    expr_refs(guard, used);
                }
                expr_refs(&arm.body, used);
            }
        }
        While { cond, body } => {
            expr_refs(cond, used);
            block_refs(body, used);
        }
        For { iter, body, .. } => {
            expr_refs(iter, used);
            block_refs(body, used);
        }
        Loop { body } => block_refs(body, used),
        Closure { body, .. } => expr_refs(body, used),
        Block(b) => block_refs(b, used),
        Return(opt) => {
            if let Some(e) = opt {
                expr_refs(e, used);
            }
        }
        Int(_) | Float(_) | Str(_) | Bool(_) | Unit | Break | Continue => {}
    }
}

#[cfg(test)]
mod module_tests {
    use super::*;

    #[test]
    fn as_module_emits_local_pub_fn_and_export_table() {
        let parsed = neoc_parser::parse("pub fn greet(name: string) -> string { name }");
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[], true).lua;
        // The `pub fn` is a `local`, not a global — globals are the single-file
        // script form (§3.7).
        assert!(
            out.contains("local function greet"),
            "pub fn should be local in a library build:\n{out}"
        );
        assert!(
            out.trim_end().ends_with("return { greet = greet }"),
            "library build must close with an export table:\n{out}"
        );
    }

    #[test]
    fn imported_trait_default_is_assigned_from_the_use_binding() {
        let lib = neoc_parser::parse(
            "pub trait Greet { fn name(self) -> string; fn hi(self) -> string { self.name() } }",
        );
        let ast::ItemKind::Trait(greet) = &lib.module.items[0].kind else {
            panic!("expected a trait");
        };
        let parsed = neoc_parser::parse(
            "use lib::Greet as G; struct D; impl G for D { fn name(self) -> string { \"d\" } }",
        );
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[("G", greet)], true).lua;
        assert!(out.contains("D.hi = G.hi"), "{out}");
        assert!(!out.contains("D.name = G.name"), "{out}");
    }

    #[test]
    fn default_assignment_waits_for_a_later_trait_table() {
        let parsed = neoc_parser::parse(
            "struct D; impl Greet for D {} trait Greet { fn hi(self) -> string { \"x\" } }",
        );
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[], true).lua;
        let table = out.find("local Greet = {}").expect("trait table");
        let assign = out.find("D.hi = Greet.hi").expect("default assignment");
        assert!(table < assign, "{out}");
    }

    #[test]
    fn pub_trait_with_defaults_is_exported() {
        let parsed = neoc_parser::parse("pub trait Greet { fn hi(self) -> string { \"x\" } }");
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[], true).lua;
        assert!(
            out.trim_end().ends_with("return { Greet = Greet }"),
            "{out}"
        );
    }

    #[test]
    fn single_file_keeps_globals_and_no_return_table() {
        let parsed = neoc_parser::parse("pub fn greet(name: string) -> string { name }");
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[], false).lua;
        assert!(
            out.contains("function greet"),
            "single-file pub fn stays a global:\n{out}"
        );
        assert!(
            !out.contains("local function greet"),
            "single-file pub fn must not be local:\n{out}"
        );
        assert!(
            !out.contains("return {"),
            "single-file build must not append an export table:\n{out}"
        );
    }

    #[test]
    fn as_module_exports_pub_const_struct_and_method_enum() {
        let src = "\
pub const MAX: number = 10;
pub struct Point { x: number, y: number }
pub enum Color { Red, Green }
impl Color { fn name(self) -> string { \"c\" } }
enum Hidden { A, B }
fn helper() {}";
        let parsed = neoc_parser::parse(src);
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        let out = emit_with_imports(&parsed.module, "", &[], &[], &[], &[], true).lua;
        let last = out.trim_end().lines().last().unwrap();
        // Pub const, struct, and the method-bearing enum export; the private fn
        // and the plain (no-method) enum do not.
        assert_eq!(last, "return { MAX = MAX, Point = Point, Color = Color }");
    }

    /// Emit `src` as a library with `shadowed` as its shadowed type imports.
    fn emit_shadowed(src: &str, shadowed: &[TypeImport]) -> String {
        let parsed = neoc_parser::parse(src);
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        emit_with_imports(&parsed.module, src, &[], &[], shadowed, &[], true).lua
    }

    fn shadowed(path: &[&str], binding: &str) -> TypeImport {
        TypeImport {
            path: path.iter().map(ToString::to_string).collect(),
            binding: binding.to_string(),
        }
    }

    // `lang/DYNAMIC.md` §3.7: a shadowed type import binds a fresh local; the
    // value keeps the plain name.
    #[test]
    fn shadowed_type_import_binds_fresh_local_and_literal_reads_it() {
        let out = emit_shadowed(
            "use pal::Hue;\nuse cfg::Hue;\npub fn main() -> number { let h = Hue { r: Hue }; h.r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(
            out.contains("local __type_Hue = require(\"pal\").Hue"),
            "{out}"
        );
        assert!(out.contains("local Hue = require(\"cfg\").Hue"), "{out}");
        assert_eq!(out.matches("local Hue =").count(), 1, "{out}");
        assert!(out.contains("__type_Hue.new({ r = Hue })"), "{out}");
    }

    #[test]
    fn shadowed_type_import_after_value_import_still_binds_fresh_local() {
        let out = emit_shadowed(
            "use cfg::Hue;\nuse pal::Hue;\npub fn main() -> number { let h = Hue { r: Hue }; h.r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(out.contains("local Hue = require(\"cfg\").Hue"), "{out}");
        assert!(
            out.contains("local __type_Hue = require(\"pal\").Hue"),
            "{out}"
        );
        assert!(out.contains("__type_Hue.new({ r = Hue })"), "{out}");
    }

    #[test]
    fn shadowed_type_import_struct_update_reads_fresh_local() {
        let out = emit_shadowed(
            "use pal::Hue;\nuse cfg::Hue;\n\
             pub fn main() -> number { let h = Hue { r: 1 }; let g = Hue { r: Hue, ..h }; g.r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert_eq!(out.matches("__type_Hue.new(").count(), 2, "{out}");
        assert!(!out.contains(" Hue.new("), "{out}");
    }

    #[test]
    fn shadowed_type_import_associated_path_reads_fresh_local() {
        let out = emit_shadowed(
            "use pal::Hue;\nuse cfg::Hue;\npub fn main() -> number { Hue::make(Hue).r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(out.contains("__type_Hue.make(Hue)"), "{out}");
    }

    #[test]
    fn shadowed_type_import_alias_binds_fresh_local_for_the_alias() {
        let out = emit_shadowed(
            "use pal::Hue as X;\nuse cfg::Hue as X;\npub fn main() -> number { let h = X { r: X }; h.r }",
            &[shadowed(&["pal", "Hue"], "X")],
        );
        assert!(
            out.contains("local __type_X = require(\"pal\").Hue"),
            "{out}"
        );
        assert!(out.contains("local X = require(\"cfg\").Hue"), "{out}");
        assert!(out.contains("__type_X.new({ r = X })"), "{out}");
    }

    #[test]
    fn shadowed_type_import_from_nested_module_requires_dotted_path() {
        let out = emit_shadowed(
            "use a::pal::Hue;\nuse cfg::Hue;\npub fn main() -> number { let h = Hue { r: Hue }; h.r }",
            &[shadowed(&["a", "pal", "Hue"], "Hue")],
        );
        assert!(
            out.contains("local __type_Hue = require(\"a.pal\").Hue"),
            "{out}"
        );
    }

    #[test]
    fn shadowed_type_import_by_module_const_binds_fresh_local() {
        let out = emit_shadowed(
            "use pal::Hue;\nconst Hue: number = 7;\npub fn main() -> number { let h = Hue { r: Hue }; h.r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(
            out.contains("local __type_Hue = require(\"pal\").Hue"),
            "{out}"
        );
        assert!(out.contains("__type_Hue.new({ r = Hue })"), "{out}");
    }

    #[test]
    fn only_the_shadowed_use_of_a_path_binds_a_fresh_local() {
        // Keyed by `use` decl, not path: `pal::Hue as Q` binds a name nothing
        // else takes, so it keeps it.
        let out = emit_shadowed(
            "use pal::Hue;\nuse pal::Hue as Q;\nuse cfg::Hue;\n\
             pub fn main() -> number { let h = Q { r: Hue }; h.r }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(out.contains("local Q = require(\"pal\").Hue"), "{out}");
        assert!(out.contains("Q.new({ r = Hue })"), "{out}");
        assert!(!out.contains("__type_Q"), "{out}");
    }

    #[test]
    fn fresh_local_skips_names_the_source_spells() {
        let out = emit_shadowed(
            "use pal::Hue;\nuse cfg::Hue;\n\
             pub fn main() -> number { let __type_Hue = 3; let h = Hue { r: Hue }; h.r + __type_Hue }",
            &[shadowed(&["pal", "Hue"], "Hue")],
        );
        assert!(
            out.contains("local __type_Hue_1 = require(\"pal\").Hue"),
            "{out}"
        );
        assert!(out.contains("__type_Hue_1.new({ r = Hue })"), "{out}");
    }

    #[test]
    fn fresh_locals_of_two_shadowed_types_never_coincide() {
        // `__type_Hue` is spelled by the source, so `Hue` steps to
        // `__type_Hue_1` — the name binding `Hue_1` would otherwise take first.
        let out = emit_shadowed(
            "use pal::Hue;\nuse pal::Hue_1;\nuse cfg::Hue;\nuse cfg::Hue_1;\n\
             pub fn main() -> number { let __type_Hue = 3; let h = Hue { r: Hue }; \
             let k = Hue_1 { r: Hue_1 }; h.r + k.r + __type_Hue }",
            &[
                shadowed(&["pal", "Hue"], "Hue"),
                shadowed(&["pal", "Hue_1"], "Hue_1"),
            ],
        );
        assert!(
            out.contains("local __type_Hue_1 = require(\"pal\").Hue\n"),
            "{out}"
        );
        assert!(
            out.contains("local __type_Hue_1_1 = require(\"pal\").Hue_1"),
            "{out}"
        );
        assert!(out.contains("__type_Hue_1.new({ r = Hue })"), "{out}");
        assert!(out.contains("__type_Hue_1_1.new({ r = Hue_1 })"), "{out}");
    }

    #[test]
    fn source_mentions_ident_matches_whole_tokens_only() {
        assert!(source_mentions_ident("let __type_Hue = 1", "__type_Hue"));
        assert!(!source_mentions_ident("let __type_Hue_1 = 1", "__type_Hue"));
        assert!(!source_mentions_ident("let x__type_Hue = 1", "__type_Hue"));
        assert!(!source_mentions_ident("", "__type_Hue"));
    }
}
