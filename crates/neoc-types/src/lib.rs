//! Static type checking for neoc (`lang/STATIC.md`).
//!
//! Consumes a [`neoc_ast::Module`] and produces [`Diagnostic`]s for the
//! static-semantics violations the spec defines: unresolved names, expression
//! and call type mismatches, malformed struct literals, field access on
//! non-structs, non-exhaustive or redundant `match` arms, misuse of the `?`
//! operator, and incomplete or extraneous trait-impl methods.
//!
//! The entry point is [`check`]. It runs in two stages: declaration collection
//! builds a symbol table over the module's two namespaces (`STATIC.md` §2),
//! then a walking checker infers and validates every function body. All
//! diagnostic messages speak neoc — `number`, `string`, `Result<T, E>`,
//! user-defined `struct`s — and never leak host-language type names.
//!
//! ```
//! let result = neoc_parser::parse("fn main() -> number { 1 + 1 }");
//! let check = neoc_types::check(&result.module);
//! assert!(check.diagnostics.is_empty());
//! ```

mod attrs;
mod collect;
mod elaborate;
mod env;
mod infer;
mod resolve;
mod ty;

pub use elaborate::{elaborate, elaborate_with_table};
pub use env::{
    BindingId, EnumInfo, MethodInfo, StructFields, StructInfo, SymbolTable, TraitInfo, TraitMethod,
    TypeDef, VariantFields, VariantInfo,
};
pub use infer::ExprType;
pub use resolve::{Binding, BindingKind, Resolution, Resolved};
pub use ty::Ty;

use infer::Checker;

/// A single static-semantics problem found while checking a module.
///
/// The message is a complete, neoc-flavoured sentence suitable for direct
/// display. `start..end` is the half-open byte range in the source the
/// diagnostic refers to; it defaults to `0..0` when the reporting site has no
/// node span to attach (a fallback the compiler maps to the start of the file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// The outcome of checking a module: every diagnostic the checker produced.
///
/// An empty `diagnostics` vector means the module is well-typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckResult {
    /// Returns `true` when the module passed static checking with no problems.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Type-checks a neoc module and returns the diagnostics it produced.
///
/// This is the crate's primary entry point. It collects the module's
/// declarations into a symbol table, then walks every function, `const`,
/// `impl`, and trait default body, validating each against the rules in
/// `lang/STATIC.md`. A returned [`CheckResult`] with an empty `diagnostics`
/// vector means the module is well-typed.
///
/// # Arguments
///
/// * `module` — the parsed source file to check. The caller owns it; the
///   checker borrows it for the duration of the call and retains nothing.
#[must_use]
pub fn check(module: &neoc_ast::Module) -> CheckResult {
    let table = collect::collect(module);
    check_with_table(module, &table)
}

/// Type-checks a module's bodies against a caller-supplied [`SymbolTable`].
///
/// Like [`check`], but the table is provided rather than freshly collected from
/// the module. A multi-file driver uses this to check a module after augmenting
/// its table with the `pub` symbols brought in by `use` from sibling modules,
/// so cross-module references resolve. The table must already hold the module's
/// own declarations (collect it with [`collect`] first, then add imports).
///
/// # Arguments
///
/// * `module` — the parsed source file whose bodies are checked.
/// * `table` — the symbol table to resolve names against. Ownership stays with
///   the caller; the checker borrows it for the duration of the call.
#[must_use]
pub fn check_with_table(module: &neoc_ast::Module, table: &SymbolTable) -> CheckResult {
    let mut checker = Checker::new(table);
    checker.check_module(module);
    CheckResult {
        diagnostics: checker.into_diagnostics(),
    }
}

/// Collects a module's declarations into a [`SymbolTable`] without running the
/// expression checker.
///
/// A later emitter can use this to query what names resolve to: whether a path
/// names an `enum` variant, what fields a `struct` declares, what type a `fn`
/// has. [`check`] uses it internally; it is exposed here so downstream tooling
/// can reuse the resolved declarations rather than re-deriving them.
#[must_use]
pub fn collect(module: &neoc_ast::Module) -> SymbolTable {
    collect::collect(module)
}

/// The inferred type of every expression in `module`, each with the byte range
/// it occupies in the source.
///
/// Runs the same checker as [`check`] with expression-type recording enabled,
/// so tooling can map a cursor position to a type (hover, type inlay hints).
/// Diagnostics are discarded; use [`check`] when only the diagnostics are
/// wanted. The returned spans may overlap — an outer expression's range
/// contains its sub-expressions' — so a position query should prefer the
/// narrowest containing span.
#[must_use]
pub fn expression_types(module: &neoc_ast::Module) -> Vec<ExprType> {
    let table = collect::collect(module);
    let mut checker = Checker::new(&table);
    checker.record_types();
    checker.check_module(module);
    checker.into_expr_types()
}

/// The scope-aware name resolution of `module`: every local binding it
/// introduces and what each single-segment identifier use resolves to (a local
/// binding or a top-level item).
///
/// Runs the same checker as [`check`] with resolution recording enabled, so
/// tooling can resolve a cursor to a *binding* and find that binding's
/// occurrences — references, rename, and goto that respect scope and shadowing,
/// rather than matching by spelling. Diagnostics are discarded.
#[must_use]
pub fn resolution(module: &neoc_ast::Module) -> Resolution {
    let table = collect::collect(module);
    let mut checker = Checker::new(&table);
    checker.record_resolution();
    checker.check_module(module);
    checker.into_resolution()
}
