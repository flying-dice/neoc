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

/// Lower a module to Lua 5.1 source, with no source map.
///
/// Use [`emit_with_source`] when a runtime needs to translate emitted-Lua line
/// numbers back to neoc positions; the emitted Lua text is identical either way.
#[must_use]
pub fn emit(module: &ast::Module) -> EmitResult {
    emit_inner(module, None)
}

/// Lower a module to Lua 5.1 source and build a [`LineMap`] from `source`.
///
/// `source` must be the exact `.neoc` text the `module` was parsed from; the
/// node byte spans index into it to derive line numbers. The emitted Lua text
/// is byte-for-byte identical to [`emit`]'s — only the [`EmitResult::source_map`]
/// differs.
#[must_use]
pub fn emit_with_source(module: &ast::Module, source: &str) -> EmitResult {
    emit_inner(module, Some(source))
}

fn emit_inner(module: &ast::Module, source: Option<&str>) -> EmitResult {
    let mut e = Emitter {
        enums: EnumIndex::build(module),
        tuple_structs: TupleStructIndex::build(module),
        line_index: source.map(LineIndex::build),
        forward_decls: forward_declared_fns(module),
        ..Emitter::default()
    };
    e.emit_module(module);
    let diagnostics = std::mem::take(&mut e.diagnostics);
    e.finish_with_map(diagnostics)
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
    /// The enclosing-loop stack. Each frame records whether the loop body was
    /// restructured for `continue` (the repeat/break idiom) and, if so, the name
    /// of its outer-break flag — see [`expr`].
    loops: Vec<LoopFrame>,
    /// Counter feeding unique outer-break flag names across `continue` loops.
    break_flag_seq: usize,
    /// Counter feeding unique temporaries for the `?` operator (§4.10).
    try_seq: usize,
    /// Native imports from module-bound `ext` blocks, sorted by specifier.
    imports: Vec<(String, Vec<String>)>,
    /// Private free functions referenced before their definition. They are
    /// declared `local` up front and assigned in place, so a caller emitted
    /// earlier closes over the (later-assigned) local rather than a nil global.
    /// A plain `local function` is only in scope after its line.
    forward_decls: std::collections::HashSet<String>,
}

/// A loop being emitted. When `break_flag` is set the loop uses the
/// repeat/break idiom (because its body contains `continue`); `continue` then
/// lowers to a `break` of the inner `repeat` and a neoc `break` sets the flag
/// and breaks out so the outer loop also exits.
pub(crate) struct LoopFrame {
    pub break_flag: Option<String>,
}

const RESULT_PRELUDE: &str = "\
local Ok   = function(v) return { _enum = \"Result\", tag = \"Ok\", v } end
local Err  = function(e) return { _enum = \"Result\", tag = \"Err\", e } end
local Some = function(v) return { _enum = \"Option\", tag = \"Some\", v } end
local None = { _enum = \"Option\", tag = \"None\" }";

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

        if self.uses_result {
            let prelude_lines = RESULT_PRELUDE.split('\n').count() + 1;
            let mut out = vec![RESULT_PRELUDE.to_string(), String::new()];
            out.append(&mut self.lines);
            self.lines = out;
            self.prepend_origins(tracking, prelude_lines, preamble_origin);
        }
        if !self.imports.is_empty() {
            let mut out = Vec::new();
            for (n, (spec, names)) in self.imports.iter().enumerate() {
                let handle = format!("_mod_{n}");
                out.push(format!("local {handle} = require(\"{spec}\")"));
                for name in names {
                    out.push(format!("local {name} = {handle}.{name}"));
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

    fn emit_module(&mut self, module: &ast::Module) {
        self.collect_method_enums(module);
        self.imports = ast::module_imports(module, ast::Target::Lua);
        // Hoist the locals of forward-referenced private functions so a caller
        // emitted earlier captures the local, not a nil global.
        if !self.forward_decls.is_empty() {
            let mut names: Vec<String> = self.forward_decls.iter().cloned().collect();
            names.sort_unstable();
            self.line(format!("local {}", names.join(", ")));
        }
        for item in &module.items {
            self.emit_item(item);
        }
    }

    /// Record every enum that needs a method table: one with a `#[derive(...)]`
    /// or any `impl` block targeting it. Variant construction for these enums is
    /// `setmetatable`d so `:method` dispatch resolves (§3.2).
    fn collect_method_enums(&mut self, module: &ast::Module) {
        let enum_names: std::collections::HashSet<&str> = module
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
            ast::ItemKind::Fn(f) => self.emit_fn(None, f, item.vis),
            ast::ItemKind::Struct(s) => {
                self.emit_struct(s, item.vis);
                self.emit_derives(&item.attrs, neoc_macros::DeriveTarget::Struct(s));
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
                }
            }
            // No inline Lua output: aliases erase; ext / extern externals are
            // provided by the runtime (hosted blocks' imports, when present,
            // emit in the preamble).
            ast::ItemKind::TypeAlias(_)
            | ast::ItemKind::Ext(_)
            | ast::ItemKind::Extern { .. }
            | ast::ItemKind::Trait(_) => {}
        }
    }

    /// Emit the methods generated by each `#[derive(...)]` on a type, right
    /// after its constructor so they land on the type's table.
    fn emit_derives(&mut self, attrs: &[ast::Attribute], target: neoc_macros::DeriveTarget) {
        for name in derive_names(attrs) {
            match neoc_macros::derive(&name, target) {
                Ok(lines) => {
                    for l in lines {
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
        let binding = u.alias.as_deref().unwrap_or(last);
        let module = prefix.join(".");
        self.line(format!("local {binding} = require(\"{module}\").{last}"));
    }

    fn emit_const(&mut self, c: &ast::ConstDecl, vis: ast::Visibility) {
        let value = self.emit_expr(&c.value);
        let kw = if vis == ast::Visibility::Public {
            ""
        } else {
            "local "
        };
        self.line(format!("{kw}{} = {value}", c.name));
    }

    fn emit_struct(&mut self, s: &ast::StructDecl, vis: ast::Visibility) {
        let local = if vis == ast::Visibility::Public {
            ""
        } else {
            "local "
        };
        match &s.body {
            ast::StructBody::Unit => {
                self.line(format!(
                    "{local}{} = {{ _struct = \"{}\" }}",
                    s.name, s.name
                ));
            }
            ast::StructBody::Tuple(_) => {
                self.line(format!("{local}{} = {{}}", s.name));
                self.line(format!("{}.__index = {}", s.name, s.name));
                self.line(format!("function {}.new(value)", s.name));
                self.indented(|e| {
                    e.line(format!(
                        "return setmetatable({{ _struct = \"{}\", value }}, {})",
                        s.name, s.name
                    ));
                });
                self.line("end");
            }
            ast::StructBody::Named(_) => {
                self.line(format!("{local}{} = {{}}", s.name));
                self.line(format!("{}.__index = {}", s.name, s.name));
                self.line(format!("function {}.new(fields)", s.name));
                self.indented(|e| {
                    e.line(format!("fields._struct = \"{}\"", s.name));
                    e.line(format!("return setmetatable(fields, {})", s.name));
                });
                self.line("end");
            }
        }
    }

    /// Emit the method table for an enum that has `impl`/derived methods. The
    /// table doubles as the shared metatable: `E.__index = E` lets variant
    /// values resolve `:method` once they are `setmetatable`d at construction
    /// (§3.2).
    fn emit_enum_table(&mut self, name: &str, vis: ast::Visibility) {
        let local = if vis == ast::Visibility::Public {
            ""
        } else {
            "local "
        };
        self.line(format!("{local}{name} = {{}}"));
        self.line(format!("{name}.__index = {name}"));
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
    /// associated function inside an impl.
    fn emit_fn(&mut self, target: Option<&str>, f: &ast::FnDecl, vis: ast::Visibility) {
        let mut params = Vec::new();
        for p in &f.sig.params {
            match p {
                ast::Param::Receiver => params.push("self".to_string()),
                ast::Param::Named { name, .. } => params.push(name.clone()),
            }
        }
        let param_list = params.join(", ");
        let header = match target {
            Some(t) => format!("function {t}.{}({param_list})", f.sig.name),
            None if vis == ast::Visibility::Public => {
                format!("function {}({param_list})", f.sig.name)
            }
            // A forward-referenced private fn: assign the pre-declared `local`
            // (hoisted in `emit_module`) instead of declaring it here, where a
            // `local function` would be invisible to earlier callers.
            None if self.forward_decls.contains(&f.sig.name) => {
                format!("{} = function({param_list})", f.sig.name)
            }
            None => format!("local function {}({param_list})", f.sig.name),
        };
        self.line(header);
        self.indented(|e| e.emit_block_as_body(&f.body));
        self.line("end");
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

/// The private free functions referenced by an item that precedes their
/// definition. Those must be declared `local` up front (see [`Emitter`]'s
/// `forward_decls`); a `local function` is only visible from its own line on, so
/// an earlier caller would otherwise close over a nil global. Pub functions emit
/// as globals (visible everywhere) and methods as table fields, so neither
/// needs this.
fn forward_declared_fns(module: &ast::Module) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut def_index: HashMap<&str, usize> = HashMap::new();
    for (i, item) in module.items.iter().enumerate() {
        if let ast::ItemKind::Fn(f) = &item.kind
            && item.vis == ast::Visibility::Private
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
