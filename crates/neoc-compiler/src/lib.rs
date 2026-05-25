//! The public library API runtimes embed. Orchestrates parse → AST → emit.
//!
//! The stable seam is [`transpile`]. The type-environment parameter and
//! `parse_ext` from `lang/`'s API sketch arrive with the type-checker stage;
//! today `transpile` runs parse + emit and merges their diagnostics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use neoc_ast as ast;
use neoc_codegen_js as codegen_js;
use neoc_codegen_lua as codegen_lua;
use neoc_codegen_py as codegen_py;
use neoc_parser as parser;
use neoc_types as types;

/// A backend the compiler can lower to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// Lua 5.1 (`.lua`).
    #[default]
    Lua,
    /// JavaScript, ES2022 (`.js`).
    Js,
    /// Python 3 (`.py`).
    Python,
}

impl Target {
    /// The file extension for this target's output.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Target::Lua => "lua",
            Target::Js => "js",
            Target::Python => "py",
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Target::Lua => "lua",
            Target::Js => "js",
            Target::Python => "python",
        })
    }
}

impl std::str::FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "lua" => Ok(Target::Lua),
            "js" | "javascript" => Ok(Target::Js),
            "py" | "python" => Ok(Target::Python),
            other => Err(format!(
                "unknown target `{other}` (expected `lua`, `js`, or `python`)"
            )),
        }
    }
}

/// A problem found anywhere in the pipeline, tagged with its stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: Stage,
    pub message: String,
    /// Byte range in the source the diagnostic refers to, when the producing
    /// stage tracks one. The parser records spans; the type and emit stages do
    /// not yet, so theirs is `None`.
    pub span: Option<ByteSpan>,
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

/// A line-level map from emitted target source back to neoc source.
///
/// Built only for [`Target::Lua`] today: [`neoc_codegen_lua::LineMap`] is the
/// backing representation. Other targets leave it empty until their backends
/// grow a source map (see #56). A runtime that surfaces target-line numbers in
/// a fault translates them to neoc positions through [`SourceMap::neoc_line`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    lua: codegen_lua::LineMap,
}

impl SourceMap {
    /// Returns the originating neoc line for a one-based emitted line, using the
    /// nearest preceding mapped entry on an exact miss. Returns `None` when the
    /// map is empty (no source was supplied, or the target has no map yet).
    #[must_use]
    pub fn neoc_line(&self, emitted_line: u32) -> Option<u32> {
        self.lua.neoc_line(emitted_line)
    }
}

/// The result of compiling a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    /// The emitted source for the chosen [`Target`].
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    /// The line map from emitted source back to neoc source. Empty for targets
    /// without a source map yet (everything but Lua). `build`/`check` ignore it.
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
        .map(|d| Diagnostic {
            stage: Stage::Parse,
            message: d.message.clone(),
            span: Some(ByteSpan {
                start: d.start,
                end: d.end,
            }),
        })
        .collect();

    let checked = types::check(&parsed.module);
    diagnostics.extend(checked.diagnostics.iter().map(|d| Diagnostic {
        stage: Stage::Type,
        message: d.message.clone(),
        // A located type diagnostic carries the byte range of the node it
        // reports against; the `0..0` fallback (no node span) stays `None` so
        // the LSP renders it at the start of the file rather than mislocating
        // it.
        span: (d.start != 0 || d.end != 0).then_some(ByteSpan {
            start: d.start,
            end: d.end,
        }),
    }));

    // Mark string-typed `+` as concatenation before emitting. Each backend
    // interprets it: Lua picks `..`, JS and Python keep `+` (`lang/DYNAMIC.md`
    // §4.2).
    let elaborated = types::elaborate(&parsed.module);
    let (output, emit_messages, source_map) = emit(target, &elaborated, source);
    diagnostics.extend(emit_messages.into_iter().map(|message| Diagnostic {
        stage: Stage::Emit,
        message,
        span: None,
    }));

    CompileOutput {
        output,
        diagnostics,
        source_map,
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
}

/// Compiles a multi-file neoc project rooted at `entry` to `target`.
///
/// Resolves the transitive `use` graph: a `use a::b::c;` names module `a::b`
/// (the path up to the final segment) and item `c`. The module resolves to
/// `<root>/a/b.neoc`, where `<root>` is the directory containing `entry`; a
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
/// * `entry` — the entry-point `.neoc` file. Its directory is the project root
///   against which module paths resolve.
/// * `target` — the backend to lower every module to.
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] if `entry` (or any module reached
/// from it) cannot be read.
pub fn transpile_project(entry: &Path, target: Target) -> std::io::Result<ProjectOutput> {
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
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
                diagnostic: Diagnostic {
                    stage: Stage::Parse,
                    message: d.message.clone(),
                    span: Some(ByteSpan {
                        start: d.start,
                        end: d.end,
                    }),
                },
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
            },
        );
    }

    // Check every module's bodies against a table augmented with the `pub`
    // symbols its `use` declarations import. Visibility violations and
    // unresolved module paths are diagnostics.
    let mut files = Vec::new();
    let mut source_maps = Vec::new();
    for path in &order {
        let (output, source_map) =
            check_and_emit_module(&root, &loaded, path, target, &mut diagnostics);
        files.push((loaded[path].path.with_extension(target.extension()), output));
        source_maps.push(source_map);
    }

    if !diagnostics.is_empty() {
        files.clear();
        source_maps.clear();
    }
    Ok(ProjectOutput {
        files,
        source_maps,
        diagnostics,
    })
}

/// Checks one resolved module's bodies (with its `use`-imported symbols in
/// scope) and emits it to `target`, pushing any type/emit diagnostics onto
/// `diagnostics`. Returns the emitted source and its [`SourceMap`].
fn check_and_emit_module(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    path: &Path,
    target: Target,
    diagnostics: &mut Vec<ProjectDiagnostic>,
) -> (String, SourceMap) {
    let me = &loaded[path];
    let mut table = types::collect(&me.module);
    for item in &me.module.items {
        if let ast::ItemKind::Use(u) = &item.kind {
            resolve_use(root, loaded, item, u, &mut table, diagnostics, path);
        }
    }
    let checked = types::check_with_table(&me.module, &table);
    for d in &checked.diagnostics {
        diagnostics.push(ProjectDiagnostic {
            path: path.to_path_buf(),
            diagnostic: Diagnostic {
                stage: Stage::Type,
                message: d.message.clone(),
                span: (d.start != 0 || d.end != 0).then_some(ByteSpan {
                    start: d.start,
                    end: d.end,
                }),
            },
        });
    }
    let elaborated = types::elaborate(&me.module);
    let (output, emit_messages, source_map) = emit(target, &elaborated, &me.source);
    for message in emit_messages {
        diagnostics.push(ProjectDiagnostic {
            path: path.to_path_buf(),
            diagnostic: Diagnostic {
                stage: Stage::Emit,
                message,
                span: None,
            },
        });
    }
    (output, source_map)
}

/// Resolves one `use` declaration against the loaded module graph, importing
/// the named `pub` item into `table` or pushing a diagnostic.
///
/// `item` is the `use` item (its span anchors the diagnostic); `from` is the
/// importing module's path.
fn resolve_use(
    root: &Path,
    loaded: &HashMap<PathBuf, LoadedModule>,
    item: &ast::Item,
    u: &ast::UseDecl,
    table: &mut types::SymbolTable,
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
            diagnostic: Diagnostic {
                stage: Stage::Type,
                message,
                span: (item.span.start != 0 || item.span.end != 0).then_some(ByteSpan {
                    start: item.span.start,
                    end: item.span.end,
                }),
            },
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
    table.import_from(&source.table, last, binding);
}

/// Maps a `use` path to the source file of the module it names, relative to the
/// project `root`. The final segment is the imported item, not part of the
/// module path; the preceding segments are the module path (`a::b` →
/// `<root>/a/b.neoc`). Returns `None` for a single-segment path (no module
/// qualifier, so nothing to resolve to a sibling file).
fn module_file(root: &Path, path: &[ast::Ident]) -> Option<PathBuf> {
    let (_, prefix) = path.split_last()?;
    let (file, dirs) = prefix.split_last()?;
    let mut p = root.to_path_buf();
    for dir in dirs {
        p.push(dir);
    }
    p.push(format!("{file}.neoc"));
    Some(p)
}

/// Canonicalises a path, falling back to the path itself when it does not yet
/// exist on disk, so unresolved imports still get a stable map key.
fn canonical_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Dispatches emission to the backend for `target`, returning its source, any
/// emit diagnostic messages, and a [`SourceMap`] back to neoc.
///
/// `source` is the neoc text the module was parsed from; the Lua backend uses
/// it to map emitted lines to neoc lines. The JS and Python backends have no
/// source map yet (#56), so they return an empty [`SourceMap`].
fn emit(target: Target, module: &ast::Module, source: &str) -> (String, Vec<String>, SourceMap) {
    match target {
        Target::Lua => {
            let r = codegen_lua::emit_with_source(module, source);
            (
                r.lua,
                r.diagnostics.into_iter().map(|d| d.message).collect(),
                SourceMap { lua: r.source_map },
            )
        }
        Target::Js => {
            let r = codegen_js::emit(module);
            (
                r.js,
                r.diagnostics.into_iter().map(|d| d.message).collect(),
                SourceMap::default(),
            )
        }
        Target::Python => {
            let r = codegen_py::emit(module);
            (
                r.py,
                r.diagnostics.into_iter().map(|d| d.message).collect(),
                SourceMap::default(),
            )
        }
    }
}
