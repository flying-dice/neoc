//! The public library API runtimes embed. Orchestrates parse → AST → emit.
//!
//! The stable seam is [`transpile`]. The type-environment parameter and
//! `parse_ext` from `lang/`'s API sketch arrive with the type-checker stage;
//! today `transpile` runs parse + emit and merges their diagnostics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use neoc_ast as ast;
use neoc_codegen_js as codegen_js;
use neoc_codegen_luau as codegen_luau;
use neoc_codegen_python as codegen_python;
use neoc_codegen_ts as codegen_ts;
use neoc_parser as parser;
use neoc_types as types;

mod project;
pub use project::{Project, discover_project, resolution_root};

/// A failure surfaced by the compiler's project/build orchestration, reported
/// in neoc terms.
///
/// `#[non_exhaustive]`: new variants may be added without a breaking change.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A `neoc.toml` manifest was found but could not be understood: it is not
    /// valid TOML, omits `[package]`, `name`, or a recognised `target`, or
    /// could not be read. The message is host-agnostic and ready to show a
    /// neoc author.
    #[error("{0}")]
    Project(String),
}

/// A backend the compiler can lower to.
///
/// neoc emits the typed supersets and Python only: `luau` (which downgrades to
/// Lua through the Luau toolchain) and `ts` (which downgrades to JavaScript
/// through the TypeScript toolchain). Plain Lua and JavaScript are not
/// selectable targets — obtain them by downgrading the typed superset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// Self-typed Luau (`.luau`): the Lua runtime body with neoc types woven on,
    /// self-contained per module (inlined runtime + inline `Result`/`Option`
    /// type aliases) — no `neoc-lib`, no typings sidecar, no source map yet.
    /// The default target; downgrade to Lua 5.1 with the Luau toolchain.
    #[default]
    Luau,
    /// Typed TypeScript (`.ts`): the JS runtime body with neoc types woven on.
    /// Downgrade to JavaScript with the TypeScript toolchain.
    Ts,
    /// Idiomatic Python 3.10+ (`.py`): the module artifact, with a typed `.pyi`
    /// sidecar and a shared `neoc_lib.{py,pyi}` runtime at the resolution root.
    /// No source map yet.
    Python,
}

impl Target {
    /// The file extension for this target's output.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Target::Ts => "ts",
            Target::Luau => "luau",
            Target::Python => "py",
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Target::Ts => "ts",
            Target::Luau => "luau",
            Target::Python => "py",
        })
    }
}

impl std::str::FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ts" => Ok(Target::Ts),
            "luau" => Ok(Target::Luau),
            "py" | "python" => Ok(Target::Python),
            // `lua` / `js` were dropped as selectable targets: emit the typed
            // superset and downgrade through its own toolchain. Point the author
            // at the right one rather than a bare "unknown".
            "lua" => Err(
                "unknown target `lua` — neoc emits `luau` (downgrade to Lua with the Luau \
                 toolchain); targets are `luau`, `ts`, `py`"
                    .to_string(),
            ),
            "js" => Err(
                "unknown target `js` — neoc emits `ts` (downgrade to JavaScript with the \
                 TypeScript toolchain); targets are `luau`, `ts`, `py`"
                    .to_string(),
            ),
            other => Err(format!(
                "unknown target `{other}` (expected `luau`, `ts`, or `py`)"
            )),
        }
    }
}

/// A problem found anywhere in the pipeline, tagged with its stage.
///
/// `#[non_exhaustive]`: construct these only through the compiler; new fields
/// may be added without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnostic {
    pub stage: Stage,
    pub message: String,
    /// Byte range in the source the diagnostic refers to, when the producing
    /// stage tracks one. The parser records spans; the type stage records them
    /// for most nodes; the emit stage does not, so its span is `None`.
    pub span: Option<ByteSpan>,
    /// Related source locations that explain this diagnostic (where a trait
    /// method was declared, which arm already matches), each with its own
    /// message. Editors surface these as related information. Usually empty.
    pub labels: Vec<DiagnosticLabel>,
}

/// A secondary span attached to a [`Diagnostic`]: a related byte range with a
/// short message of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLabel {
    pub message: String,
    pub span: ByteSpan,
}

/// A half-open byte range `[start, end)` into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Parse,
    Type,
    Emit,
}

impl Diagnostic {
    /// A parse-stage diagnostic. The parser always records the offending byte
    /// range, so it is always spanned.
    fn parse(message: String, start: usize, end: usize) -> Self {
        Self {
            stage: Stage::Parse,
            message,
            span: Some(ByteSpan { start, end }),
            labels: Vec::new(),
        }
    }

    /// A type-stage diagnostic. A located diagnostic carries the byte range of
    /// the node it reports against; the `0..0` fallback (no node span) stays
    /// `None`, so the LSP renders it at the file start rather than mislocating
    /// it.
    fn typed(message: String, start: usize, end: usize) -> Self {
        Self {
            stage: Stage::Type,
            message,
            span: (start != 0 || end != 0).then_some(ByteSpan { start, end }),
            labels: Vec::new(),
        }
    }

    /// A type-stage diagnostic mapped from the type checker's own diagnostic,
    /// carrying its span and any related-information labels through.
    fn typed_from(d: &types::Diagnostic) -> Self {
        Self {
            labels: d
                .labels
                .iter()
                .map(|l| DiagnosticLabel {
                    message: l.message.clone(),
                    span: ByteSpan {
                        start: l.start,
                        end: l.end,
                    },
                })
                .collect(),
            ..Self::typed(d.message.clone(), d.start, d.end)
        }
    }

    /// An emit-stage diagnostic. Emit messages carry no source range.
    fn emit(message: String) -> Self {
        Self {
            stage: Stage::Emit,
            message,
            span: None,
            labels: Vec::new(),
        }
    }
}

/// A line-level map from emitted target source back to neoc source.
///
/// No target emits a map today: the only backend that built one was the
/// now-removed direct Lua target. This is the inert core — an always-empty map —
/// kept on the public API so a runtime can translate a fault's emitted line to a
/// neoc position through [`SourceMap::neoc_line`] once a backend grows a real map
/// (see #56). Until then every map is empty and every lookup returns `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {}

impl SourceMap {
    /// Returns the originating neoc line for a one-based emitted line, or `None`
    /// when no correspondence is known. Always `None` today, since no backend
    /// builds a map yet.
    #[must_use]
    pub fn neoc_line(&self, _emitted_line: u32) -> Option<u32> {
        None
    }

    /// Whether this map carries any line correspondence. Always `true` today
    /// (every map is empty), so a caller surfacing a fault falls back to the
    /// emitted line.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.neoc_line(1).is_none()
    }
}

/// The result of compiling a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    /// The emitted source for the chosen [`Target`].
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    /// The line map from emitted source back to neoc source. Empty today (no
    /// backend builds a map yet; see [`SourceMap`]). `build`/`check` ignore it.
    pub source_map: SourceMap,
}

impl CompileOutput {
    /// Whether compilation produced no diagnostics.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Compile neoc source to the given [`Target`].
#[must_use]
pub fn transpile(source: &str, target: Target) -> CompileOutput {
    let parsed = parser::parse(source);
    let mut diagnostics: Vec<Diagnostic> = parsed
        .diagnostics
        .iter()
        .map(|d| Diagnostic::parse(d.message.clone(), d.start, d.end))
        .collect();

    let checked = types::check(&parsed.module);
    diagnostics.extend(checked.diagnostics.iter().map(Diagnostic::typed_from));

    // Mark string-typed `+` as concatenation before emitting. Each backend
    // interprets the rewrite: Lua picks `..`, JS keeps `+` (`lang/DYNAMIC.md`
    // §4.2).
    let table = types::collect(&parsed.module);
    let elaborated = types::elaborate_with_table(&parsed.module, &table);
    // A single-file `transpile` has no resolved module graph, so no cross-module
    // `use` can resolve to a binding or an imported enum — pass empty sets.
    let inputs = EmitInputs::default();
    let emitted = emit(target, &elaborated, source, &inputs);
    diagnostics.extend(emitted.messages.into_iter().map(Diagnostic::emit));

    CompileOutput {
        output: emitted.output,
        diagnostics,
        // No backend emits a source map yet; an empty map maps no lines.
        source_map: SourceMap::default(),
    }
}

/// A diagnostic from a multi-file build, tagged with the source file it came
/// from so the caller can report which module reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    /// The `.neoc` source file the diagnostic refers to.
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

/// The result of compiling a multi-file project from an entry point.
///
/// `files` pairs each module's destination path (the source path with the
/// target's extension) with its emitted source. It is empty when any module
/// reported a diagnostic — a project emits all-or-nothing so a half-written
/// build never lands on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOutput {
    pub files: Vec<(PathBuf, String)>,
    /// One [`SourceMap`] per entry in `files`, positionally aligned, mapping
    /// emitted lines back to neoc source. `build`/`check` ignore it; `neoc run`
    /// uses the entry's map to rewrite runtime faults to neoc positions. Empty
    /// when `files` is (a project that reported a diagnostic emits nothing).
    pub source_maps: Vec<SourceMap>,
    /// Generated sidecar files that are **not** compiled modules and so have no
    /// entry in `source_maps`. No backend emits a source map yet, so there are no
    /// `.map` sidecars and no `sourceMappingURL` trailers. The kinds emitted:
    ///
    /// * On the TS target the shared `neoc-lib.js` + `neoc-lib.d.ts` prelude at
    ///   the resolution root: a `.ts` `import "./neoc-lib"` resolves to the
    ///   `.d.ts` for types and the `.js` at runtime.
    /// * On the Python target the per-module typed stub (`x.pyi`, beside each
    ///   `x.py`) plus, once at the resolution root, the shared runtime
    ///   `neoc_lib.py` and its stub `neoc_lib.pyi` (Python has no native
    ///   `Result`, so this prelude carries the runtime, not just typings).
    /// * On the Luau target none — each module is self-contained.
    ///
    /// Each entry pairs a destination path with its contents, and the path is
    /// already in its final form (extension included) — unlike `files`, it must
    /// **not** have the target extension re-applied. The `.ts` and `.luau` target
    /// modules are self-typed, so they land in `files`, not here. Empty when the
    /// project reported a diagnostic. Kept separate from `files` so the
    /// `files`↔`source_maps` positional contract stays intact.
    pub aux_files: Vec<(PathBuf, String)>,
    pub diagnostics: Vec<ProjectDiagnostic>,
}

impl ProjectOutput {
    /// Whether the project compiled with no diagnostics.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// One module in a resolved project: its source path, parsed AST, and the
/// collected symbol table for its own declarations.
struct LoadedModule {
    path: PathBuf,
    source: String,
    module: ast::Module,
    table: types::SymbolTable,
    /// The module's own parse diagnostics, kept so a focus check reports them
    /// without re-parsing.
    parse_diagnostics: Vec<parser::Diagnostic>,
}

/// Compiles a multi-file neoc project rooted at `entry` to `target`.
///
/// Resolves the transitive `use` graph: a `use a::b::c;` names module `a::b`
/// (the path up to the final segment) and item `c`. The module resolves to
/// `<root>/a/b.neoc`, where `<root>` is the resolution anchor for `entry` (see
/// [`resolution_root`]): a project's source root when `entry` lies in one, else
/// the directory containing `entry`. A
/// single-segment qualifier `use foo::bar;` resolves module `foo` to
/// `<root>/foo.neoc`. Each reachable module is parsed and collected once
/// (cycles are visited at most once), then every module's bodies are checked
/// with the `pub` items its `use` declarations bring in made visible, and
/// finally each module is emitted to its target file.
///
/// Visibility is enforced per `STATIC.md` §2: a `use` of an item that is not
/// `pub` in its source module is a diagnostic, as is a `use` whose module path
/// resolves to no file. An entry file with no `use` declarations resolves to a
/// single-module project equivalent to [`transpile`] on its source.
///
/// # Arguments
///
/// * `entry` — the entry-point `.neoc` file. Module paths resolve against its
///   resolution anchor ([`resolution_root`]): the source root when `entry` is
///   in a project, else `entry`'s own directory.
/// * `target` — the backend to lower every module to.
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] if `entry` (or any module reached
/// from it) cannot be read.
pub fn transpile_project(entry: &Path, target: Target) -> std::io::Result<ProjectOutput> {
    let root = resolution_root(entry);
    let mut loaded: HashMap<PathBuf, LoadedModule> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut diagnostics: Vec<ProjectDiagnostic> = Vec::new();

    // Resolve + parse + collect the transitive `use` graph, visiting each file
    // at most once (cycle-safe). The entry must exist; a `use`-discovered
    // dependency that resolves to no file is left unloaded so the check pass
    // reports it as an `unresolved module` diagnostic rather than aborting.
    let entry_key = canonical_or(entry);
    let mut queue: Vec<PathBuf> = vec![entry_key.clone()];
    while let Some(path) = queue.pop() {
        if loaded.contains_key(&path) {
            continue;
        }
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if path == entry_key => return Err(e),
            Err(_) => continue,
        };
        let parsed = parser::parse(&source);
        for d in &parsed.diagnostics {
            diagnostics.push(ProjectDiagnostic {
                path: path.clone(),
                diagnostic: Diagnostic::parse(d.message.clone(), d.start, d.end),
            });
        }
        let table = types::collect(&parsed.module);
        for item in &parsed.module.items {
            if let ast::ItemKind::Use(u) = &item.kind {
                if let Some(dep) = module_file(&root, &u.path) {
                    let dep = canonical_or(&dep);
                    if !loaded.contains_key(&dep) {
                        queue.push(dep);
                    }
                }
            }
        }
        order.push(path.clone());
        loaded.insert(
            path.clone(),
            LoadedModule {
                path: path.clone(),
                source,
                module: parsed.module,
                table,
                parse_diagnostics: parsed.diagnostics,
            },
        );
    }

    // Check every module's bodies against a table augmented with the `pub`
    // symbols its `use` declarations import. Visibility violations and
    // unresolved module paths are diagnostics.
    let universe = type_universe(&loaded);
    let mut files = Vec::new();
    let mut source_maps = Vec::new();
    let mut aux_files = Vec::new();
    for path in &order {
        let emitted =
            check_and_emit_module(&root, &loaded, &universe, path, target, &mut diagnostics);
        let src_path = &loaded[path].path;
        let dest = src_path.with_extension(target.extension());
        files.push((dest, emitted.output));
        // No backend emits a source map yet, so each module's map is empty; the
        // entry keeps the `files`↔`source_maps` positional contract intact.
        source_maps.push(SourceMap::default());
        // Python target: a non-empty `.pyi` typed stub sibling beside the
        // module's `.py`. `x.neoc` → `x.pyi`.
        if !emitted.pyi.is_empty() {
            aux_files.push((src_path.with_extension("pyi"), emitted.pyi));
        }
    }

    // Once per build, the target's shared prelude at the resolution root.
    aux_files.extend(root_prelude_aux(&root, target));

    if !diagnostics.is_empty() {
        files.clear();
        source_maps.clear();
        aux_files.clear();
    }
    Ok(ProjectOutput {
        files,
        source_maps,
        aux_files,
        diagnostics,
    })
}

/// The target's shared prelude files at the resolution `root`, emitted once per
/// build.
///
/// * **TS** — the runtime `neoc-lib.js` + its `neoc-lib.d.ts` types. A `.ts`
///   module's `import "./neoc-lib"` resolves to the `.d.ts` for types and the
///   `.js` at runtime.
/// * **Python** — the runtime `neoc_lib.py` + its stub `neoc_lib.pyi`. Python
///   has no native `Result`, so this prelude carries the runtime, not just
///   typings.
/// * **Luau** — none; each module is self-contained.
fn root_prelude_aux(root: &Path, target: Target) -> Vec<(PathBuf, String)> {
    match target {
        Target::Ts => vec![
            (root.join("neoc-lib.js"), codegen_js::neoc_lib_js()),
            (root.join("neoc-lib.d.ts"), codegen_js::neoc_lib_dts()),
        ],
        Target::Python => vec![
            (root.join("neoc_lib.py"), codegen_python::neoc_lib_py()),
            (root.join("neoc_lib.pyi"), codegen_python::neoc_lib_pyi()),
        ],
        Target::Luau => Vec::new(),
    }
}

/// Diagnoses one module against an in-memory project, for the editor.
///
/// `sources` maps each module's file path to its current text — open editor
/// buffers, authoritative over disk. `focus` is the module to diagnose. The
/// whole set is parsed and collected so the focus module's `use` imports
/// resolve against the real project (no phantom `unresolved module` errors on
/// a non-entry file); only the focus module is type-checked, and nothing is
/// emitted (parse and type diagnostics are target-independent). `root` is the
/// project root that `use` paths resolve against.
///
/// Returns the focus module's diagnostics — parse errors, `use`-resolution and
/// visibility errors, and type errors — attributed to `focus`. An empty result
/// means the module is well-formed in the project; a `focus` not present in
/// `sources` yields no diagnostics.
#[must_use]
pub fn check_module_in_project<H: std::hash::BuildHasher>(
    root: &Path,
    sources: &HashMap<PathBuf, String, H>,
    focus: &Path,
) -> Vec<Diagnostic> {
    let loaded = load_in_memory(sources);
    let universe = type_universe(&loaded);
    focus_diagnostics(root, &loaded, &universe, &canonical_or(focus))
}

/// Diagnoses several modules against one shared in-memory project, for the
/// editor — the batch form of [`check_module_in_project`].
///
/// `sources` maps each module's file path to its current text; `foci` are the
/// modules to diagnose (each must appear in `sources`). The project is parsed
/// and collected **once**, then every focus is checked against that shared set,
/// so re-diagnosing a module and all its importers after an edit costs one
/// universe parse rather than one per file. Returns each focus's diagnostics
/// keyed by the path passed in `foci`; a focus absent from `sources` maps to an
/// empty vector. `root` is the project root `use` paths resolve against.
#[must_use]
pub fn check_modules_in_project<H: std::hash::BuildHasher>(
    root: &Path,
    sources: &HashMap<PathBuf, String, H>,
    foci: &[PathBuf],
) -> HashMap<PathBuf, Vec<Diagnostic>> {
    let loaded = load_in_memory(sources);
    let universe = type_universe(&loaded);
    foci.iter()
        .map(|focus| {
            (
                focus.clone(),
                focus_diagnostics(root, &loaded, &universe, &canonical_or(focus)),
            )
        })
        .collect()
}

/// Parses and collects every in-memory module (no disk I/O), keyed by a
/// canonicalised path so a focus lookup matches regardless of path spelling.
fn load_in_memory<H: std::hash::BuildHasher>(
    sources: &HashMap<PathBuf, String, H>,
) -> HashMap<PathBuf, LoadedModule> {
    let mut loaded: HashMap<PathBuf, LoadedModule> = HashMap::new();
    for (path, source) in sources {
        let key = canonical_or(path);
        let parsed = parser::parse(source);
        let table = types::collect(&parsed.module);
        loaded.insert(
            key.clone(),
            LoadedModule {
                path: key,
                source: source.clone(),
                module: parsed.module,
                table,
                parse_diagnostics: parsed.diagnostics,
            },
        );
    }
    loaded
}

/// The diagnostics attributed to one focus module within an already-loaded
/// project: its stored parse diagnostics, its `use`-resolution and visibility
/// errors, and its type errors. `focus` is the canonicalised path; a focus not
/// in `loaded` yields an empty vector.
fn focus_diagnostics(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    universe: &types::SymbolTable,
    focus: &Path,
) -> Vec<Diagnostic> {
    let Some(me) = loaded.get(focus) else {
        return Vec::new();
    };
    let mut out: Vec<Diagnostic> = me
        .parse_diagnostics
        .iter()
        .map(|d| Diagnostic::parse(d.message.clone(), d.start, d.end))
        .collect();
    // Resolve the focus module's `use` imports against the project, then check
    // its bodies. `resolve_use` reports against the importing (focus) module,
    // so every diagnostic it pushes belongs to the focus.
    let mut table = types::collect(&me.module);
    let mut imports = types::SymbolTable::new();
    let mut use_diags: Vec<ProjectDiagnostic> = Vec::new();
    let mut dest = ImportDest {
        table: &mut table,
        imports: &mut imports,
    };
    for item in &me.module.items {
        if let ast::ItemKind::Use(u) = &item.kind {
            resolve_use(root, loaded, item, u, &mut dest, &mut use_diags, focus);
        }
    }
    out.extend(use_diags.into_iter().map(|pd| pd.diagnostic));
    table.import_referenced_types(&imports, universe);
    let checked = types::check_with_table(&me.module, &table);
    for d in &checked.diagnostics {
        out.push(Diagnostic::typed_from(d));
    }
    out
}

/// Checks one resolved module's bodies (with its `use`-imported symbols in
/// scope) and emits it to `target`, pushing any type/emit diagnostics onto
/// `diagnostics`. Returns the [`Emitted`] result — the source and, on the Python
/// target, the `.pyi` declaration sibling.
fn check_and_emit_module(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    universe: &types::SymbolTable,
    path: &Path,
    target: Target,
    diagnostics: &mut Vec<ProjectDiagnostic>,
) -> Emitted {
    let me = &loaded[path];
    let mut table = types::collect(&me.module);
    let mut imports = types::SymbolTable::new();
    let mut dest = ImportDest {
        table: &mut table,
        imports: &mut imports,
    };
    for item in &me.module.items {
        if let ast::ItemKind::Use(u) = &item.kind {
            resolve_use(root, loaded, item, u, &mut dest, diagnostics, path);
        }
    }
    // Bring in the shape of any type reached only through an imported
    // signature — e.g. accessing a field of a value returned by an imported
    // function whose type the module never names in a `use`. Seeded only from
    // the imported signatures so a type the module names directly stays
    // unresolved unless its own `use` brings it in.
    table.import_referenced_types(&imports, universe);
    let checked = types::check_with_table(&me.module, &table);
    for d in &checked.diagnostics {
        diagnostics.push(ProjectDiagnostic {
            path: path.to_path_buf(),
            diagnostic: Diagnostic::typed_from(d),
        });
    }
    // Elaborate against the same import-enriched table the checker used, so a
    // string `+` whose operand type resolves through a `use` import is rewritten
    // to concatenation rather than left as arithmetic.
    let externs = imported_externs(root, loaded, &me.module);
    let elaborated = types::elaborate_with_table(&me.module, &table);
    let inputs = EmitInputs {
        bindings: js_imported_bindings(root, loaded, &me.module),
        py_bindings: py_imported_bindings(root, loaded, &me.module),
        method_types: js_imported_method_types(root, loaded, &me.module),
        enums: imported_enums(root, loaded, &me.module),
        externs,
        importer_dir: module_dir_segments(root, path),
        // Every module a project build emits is a requireable library — the
        // entry included, since a host consuming the build `require`s it.
        as_module: true,
    };
    let emitted = emit(target, &elaborated, &me.source, &inputs);
    for message in &emitted.messages {
        diagnostics.push(ProjectDiagnostic {
            path: path.to_path_buf(),
            diagnostic: Diagnostic::emit(message.clone()),
        });
    }
    emitted
}

/// The destination a [`resolve_use`] writes a resolved import into: the
/// checker's full table and a record of only the `use`-brought symbols.
///
/// Each import is written to both. `table` is the checker's view (own
/// declarations plus imports); `imports` holds *only* the imported symbols and
/// seeds the transitive-shape pull in
/// [`types::SymbolTable::import_referenced_types`], so only types reached
/// *through* an import's signature are pulled — never a type the focus module
/// names directly.
struct ImportDest<'a> {
    table: &'a mut types::SymbolTable,
    imports: &'a mut types::SymbolTable,
}

/// Resolves one `use` declaration against the loaded module graph, importing
/// the named `pub` item into `dest` or pushing a diagnostic.
///
/// `item` is the `use` item (its span anchors the diagnostic); `from` is the
/// importing module's path.
fn resolve_use(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    item: &ast::Item,
    u: &ast::UseDecl,
    dest: &mut ImportDest,
    diagnostics: &mut Vec<ProjectDiagnostic>,
    from: &Path,
) {
    let Some((last, prefix)) = u.path.split_last() else {
        return;
    };
    if prefix.is_empty() {
        return;
    }
    let module_path = prefix.join("::");
    let push = |diagnostics: &mut Vec<ProjectDiagnostic>, message: String| {
        diagnostics.push(ProjectDiagnostic {
            path: from.to_path_buf(),
            diagnostic: Diagnostic::typed(message, item.span.start, item.span.end),
        });
    };
    let Some(dep_path) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
        push(diagnostics, format!("unresolved module `{module_path}`"));
        return;
    };
    let Some(source) = loaded.get(&dep_path) else {
        push(diagnostics, format!("unresolved module `{module_path}`"));
        return;
    };
    let public = source.table.value_is_public(last) || source.table.type_is_public(last);
    if !public {
        push(
            diagnostics,
            format!("`{last}` is private in module `{module_path}`"),
        );
        return;
    }
    let binding = u.alias.as_deref().unwrap_or(last);
    dest.table.import_from(&source.table, last, binding);
    dest.imports.import_from(&source.table, last, binding);
}

/// Maps a `use` path to the source file of the module it names, relative to the
/// project `root`.
///
/// The final segment is the imported item, not part of the module path; the
/// preceding segments are the module path (`a::b::Item` → `<root>/a/b.neoc`).
/// Returns `None` for a single-segment path (no module qualifier, so nothing to
/// resolve to a sibling file).
///
/// This is the single source of truth for `use`→file resolution. `neoc build` /
/// `check` walk the disk with it; tooling (the language server's cross-file
/// index) resolves against its own in-memory document set with the same rule, so
/// navigation and the compiler never disagree about where a module lives.
#[must_use]
pub fn module_file(root: &Path, use_path: &[ast::Ident]) -> Option<PathBuf> {
    let (_, prefix) = use_path.split_last()?;
    let (file, dirs) = prefix.split_last()?;
    let mut p = root.to_path_buf();
    for dir in dirs {
        p.push(dir);
    }
    p.push(format!("{file}.neoc"));
    Some(p)
}

/// The directory of module `path`, as path segments relative to the project
/// `root`. A root-level module yields an empty vector; `<root>/autosql/x.neoc`
/// yields `["autosql"]`.
///
/// `path` is the module's canonical path and `root` the resolution anchor (not
/// necessarily canonical), so `root` is canonicalised the same way before
/// stripping. A `path` that does not lie under `root` (it should always, in a
/// project) yields an empty vector — the root-level fallback, so an import never
/// gains a spurious `../`.
fn module_dir_segments(root: &Path, path: &Path) -> Vec<String> {
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Ok(rel) = path.strip_prefix(&canonical_root) else {
        return Vec::new();
    };
    match rel.parent() {
        Some(dir) => dir
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str().map(str::to_string),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Canonicalises a path, falling back to the path itself when it does not yet
/// exist on disk, so unresolved imports still get a stable map key.
fn canonical_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Dispatches emission to the backend for `target`, returning its source, any
/// emit diagnostic messages, and a [`SourceMap`] back to neoc.
///
/// `source` is the neoc text the module was parsed from; the Luau backend uses
/// it to track spans. No backend emits a source map yet (#56), so `emit` always
/// reports an empty [`SourceMap`] through the driver.
///
/// `bindings` are the module's `use`d names whose source module emits a runtime
/// JS binding (see [`codegen_js::module_js_bindings`]); the TS backend (reusing
/// the JS emit) filters its `import` emission against it so a type-only `use`
/// produces no dangling import. `method_types` is the subset whose source
/// binding is a method-bearing type (see [`codegen_js::module_js_method_types`]);
/// for those the TS backend imports the prototype `Type$` rather than the erased
/// bare name. Method dispatch itself is prototype-backed (`lang/DYNAMIC.md`
/// §7.1), so no per-type wiring is needed. The Luau backend ignores both.
/// The cross-module inputs a backend needs beyond the module itself, gathered
/// by the project driver from the resolved `use` graph.
///
/// Each field is target-aware: the Luau backend reads `enums` and `externs`; the
/// TS backend reads `bindings`, `method_types`, and `externs`; the Python
/// backend reads `enums`, `py_bindings`, and `importer_dir`. A field a target
/// ignores is built anyway (cheap) and dropped at the seam.
#[derive(Default)]
struct EmitInputs<'a> {
    /// `use`d names whose source module emits a runtime JS binding (TS only).
    bindings: std::collections::HashSet<String>,
    /// Resolved `use` paths whose source module emits a runtime Python
    /// definition (Python only). A `use` absent here resolves to a trait or
    /// alias — no Python symbol — so its import is dropped.
    py_bindings: Vec<String>,
    /// The subset of `bindings` that are method-bearing types (TS only); their
    /// `use` imports the prototype `Type$`.
    method_types: std::collections::HashSet<String>,
    /// `pub` enum declarations brought in by `use` (Luau resolves variant forms).
    enums: Vec<&'a ast::EnumDecl>,
    /// Extern items brought in by `use`, for native-import reconstruction in the
    /// consumer (Luau/TS; `lang/EXTERN.md` §Lowering).
    externs: Vec<ast::ExternItem>,
    /// The importing module's directory as path segments relative to the source
    /// root (TS/Python). Anchors each cross-module `import`'s relative specifier
    /// so a module in a subdirectory imports a sibling as `./b.js`. Empty for a
    /// root-level module and for single-file `transpile`.
    importer_dir: Vec<String>,
    /// Whether the module is emitted as a requireable library (Luau only). A
    /// project build sets `true` so each module ends with a `return { … }` export
    /// table a consumer can `require`; single-file `transpile` leaves it `false`
    /// so the chunk runs directly (`lang/DYNAMIC.md` §3.7).
    as_module: bool,
}

/// One module's emit, normalised across backends: the target source, any
/// emit-stage messages, and the `.pyi` declaration sibling (Python only; empty
/// otherwise). No backend emits a source map yet, so the driver attaches an
/// empty [`SourceMap`] per module.
struct Emitted {
    output: String,
    messages: Vec<String>,
    /// The module's `.pyi` typed stub (Python target). Empty for the other
    /// targets and for a module that exports nothing.
    pyi: String,
}

fn emit(target: Target, module: &ast::Module, source: &str, inputs: &EmitInputs) -> Emitted {
    match target {
        Target::Ts => {
            // A self-typed `.ts` is the module artifact, not a sidecar: it
            // carries its own types inline (no `.d.ts`). Reuses `codegen-js`'s
            // emit through the TS seam. No source map yet.
            let r = codegen_ts::emit(
                module,
                &inputs.bindings,
                &inputs.method_types,
                &inputs.externs,
                &inputs.enums,
                &inputs.importer_dir,
            );
            Emitted {
                output: r.ts,
                messages: r.diagnostics.into_iter().map(|d| d.message).collect(),
                pyi: String::new(),
            }
        }
        Target::Luau => {
            // A self-typed `.luau` is self-contained: it inlines the runtime and
            // its own `Result`/`Option` type aliases (no `neoc-lib`, no `.d.lua`
            // sidecar). Reuses `codegen-lua`'s lowering through the Luau seam. No
            // source map yet.
            let r = codegen_luau::emit(
                module,
                source,
                &inputs.enums,
                &inputs.externs,
                inputs.as_module,
            );
            Emitted {
                output: r.luau,
                messages: r.diagnostics.into_iter().map(|d| d.message).collect(),
                pyi: String::new(),
            }
        }
        Target::Python => {
            // Python needs the module, imported enums, and the importer dir
            // (so a `use` resolves to a relative package import from a
            // subpackage) — no JS bindings, method types, or externs. The `.py`
            // is the module artifact; the `.pyi` rides in `Emitted::pyi` to the
            // aux wiring. No source map yet.
            let r = codegen_python::emit(
                module,
                &inputs.enums,
                &inputs.importer_dir,
                &inputs.py_bindings,
            );
            Emitted {
                output: r.py,
                messages: r.diagnostics.into_iter().map(|d| d.message).collect(),
                pyi: r.pyi,
            }
        }
    }
}

/// The `use`d imports in `module` whose source module emits a runtime JS
/// binding, keyed per resolved import as `prefix::last` (the full `use` path).
///
/// For each `use a::b::X`, resolves module `a::b` against the loaded graph and
/// keeps the key `a::b::X` only if that source module emits a binding for `X` —
/// a `pub fn`, `pub const`, or `pub` struct/enum carrying a method slot (see
/// [`codegen_js::module_js_bindings`]). A type-only item — a data-only `pub`
/// struct/enum or a `pub type` alias — is excluded: its name is erased in JS,
/// so emitting an `import` for it would dangle against an empty export.
///
/// The key is the resolved `(module, name)` pair, not the bare name: two
/// modules can share a final segment (one a binding, one type-only), and keying
/// on the bare name would let the binding module's keep decision leak onto the
/// type-only module's import and re-emit a dangling import. The JS emitter
/// rebuilds the same `prefix::last` key from each `use` to look up its own
/// per-source decision. Target-agnostic; the Lua backend ignores the result.
fn js_imported_bindings(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    module: &ast::Module,
) -> std::collections::HashSet<String> {
    let mut bindings = std::collections::HashSet::new();
    for item in &module.items {
        let ast::ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let Some((last, _)) = u.path.split_last() else {
            continue;
        };
        let Some(dep) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
            continue;
        };
        let Some(source) = loaded.get(&dep) else {
            continue;
        };
        if codegen_js::module_js_bindings(&source.module).contains(last) {
            bindings.insert(u.path.join("::"));
        }
    }
    bindings
}

/// The resolved `use` paths in `module` (`prefix::last`) that resolve to a
/// runtime Python definition in their source module — a `fn`, `const`,
/// `struct`, or `enum` (see [`codegen_python::module_py_bindings`]). A `use` of
/// a `trait` or `type` alias is excluded: it emits no Python symbol, so an
/// `import` for it would dangle. The Python emitter keeps only the imports
/// present here. Target-agnostic; the other backends ignore the result.
fn py_imported_bindings(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    module: &ast::Module,
) -> Vec<String> {
    let mut bindings = Vec::new();
    for item in &module.items {
        let ast::ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let Some((last, _)) = u.path.split_last() else {
            continue;
        };
        let Some(dep) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
            continue;
        };
        let Some(source) = loaded.get(&dep) else {
            continue;
        };
        if codegen_python::module_py_bindings(&source.module).contains(last) {
            bindings.push(u.path.join("::"));
        }
    }
    bindings
}

/// The `use`d imports in `module` that resolve to a method-bearing type in their
/// source module, keyed per resolved import as `prefix::last`.
///
/// A subset of [`js_imported_bindings`]: a `pub` struct/enum carrying an `impl`
/// or `#[derive]` (see [`codegen_js::module_js_method_types`]). A method-bearing
/// type's JS runtime binding is its prototype `Type$`, so the JS emitter imports
/// `Type$` for each such `use`; dispatch needs no further wiring, since a value
/// carries its prototype across the boundary (`lang/DYNAMIC.md` §7.1). Same
/// per-resolved-import keying as [`js_imported_bindings`]. Target-agnostic; the
/// Lua backend ignores it.
fn js_imported_method_types(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    module: &ast::Module,
) -> std::collections::HashSet<String> {
    let mut method_types = std::collections::HashSet::new();
    for item in &module.items {
        let ast::ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let Some((last, _)) = u.path.split_last() else {
            continue;
        };
        let Some(dep) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
            continue;
        };
        let Some(source) = loaded.get(&dep) else {
            continue;
        };
        if codegen_js::module_js_method_types(&source.module).contains(last) {
            method_types.insert(u.path.join("::"));
        }
    }
    method_types
}

/// The project-wide pool of every module's `pub` type definitions and their
/// methods, merged into one [`types::SymbolTable`].
///
/// A module that imports a function whose signature mentions a type it never
/// names in a `use` still needs that type's shape to resolve field and method
/// access on the returned value. This pool is the source those shapes are
/// pulled from (see [`types::SymbolTable::import_referenced_types`]). Type names
/// are taken to be project-unique; if two modules declare a `pub` type of the
/// same name, the merge keeps one — neoc has no module-qualified type identity.
fn type_universe(loaded: &HashMap<PathBuf, LoadedModule>) -> types::SymbolTable {
    let mut universe = types::SymbolTable::new();
    for module in loaded.values() {
        let t = &module.table;
        for name in &t.pub_types {
            if let Some(def) = t.types.get(name) {
                universe.types.insert(name.clone(), def.clone());
            }
            for ((ty_name, method), info) in &t.methods {
                if ty_name == name {
                    universe
                        .methods
                        .insert((name.clone(), method.clone()), info.clone());
                }
            }
        }
    }
    universe
}

/// The extern items `module` imports through its `use` graph, each preserving
/// its enclosing extern `mod` so the import path / `ambient` / `name` metadata
/// survives.
///
/// For each `use a::b::X`, resolves module `a::b` and extracts the source
/// module's extern items named `X` (or, for an extern `mod`, the descendant
/// subtree reaching `X`). A `use`d extern item lowers to its **native** import
/// in *this* consumer, not an import from the defining file's emitted module —
/// an extern emits no runtime value there — so the consumer reconstructs the
/// native import and lowering from this metadata (`lang/EXTERN.md` §Lowering).
/// Names are gathered per source module so one `use` of an extern struct carries
/// its `mod`'s path and its constructors together.
fn imported_externs(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    module: &ast::Module,
) -> Vec<ast::ExternItem> {
    // Group the imported item names by their resolved source file, so each
    // source module is scanned once with the full set of names `use`d from it.
    let mut by_source: HashMap<PathBuf, std::collections::HashSet<ast::Ident>> = HashMap::new();
    for item in &module.items {
        let ast::ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let Some((last, _)) = u.path.split_last() else {
            continue;
        };
        let Some(dep) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
            continue;
        };
        by_source.entry(dep).or_default().insert(last.clone());
    }
    let mut out = Vec::new();
    for (dep, names) in &by_source {
        if let Some(source) = loaded.get(dep) {
            out.extend(ast::imported_extern_items(&source.module, names));
        }
    }
    out
}

/// The `pub` enum declarations `module` imports, gathered through its `use`
/// graph. The backend registers these so a `match` on, or a construction of, an
/// enum brought in by `use` lowers correctly; the enum definitions are emitted
/// by the modules that declare them, not here.
fn imported_enums<'a>(
    root: &Path,
    loaded: &'a HashMap<PathBuf, LoadedModule>,
    module: &ast::Module,
) -> Vec<&'a ast::EnumDecl> {
    let mut enums = Vec::new();
    for item in &module.items {
        let ast::ItemKind::Use(u) = &item.kind else {
            continue;
        };
        let Some(dep) = module_file(root, &u.path).map(|p| canonical_or(&p)) else {
            continue;
        };
        let Some(source) = loaded.get(&dep) else {
            continue;
        };
        for dep_item in &source.module.items {
            if let ast::ItemKind::Enum(e) = &dep_item.kind
                && dep_item.vis == ast::Visibility::Public
            {
                enums.push(e);
            }
        }
    }
    enums
}
