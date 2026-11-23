//! neoc's Lua runtime on mlua-ir: each module is written once in plain Rust, and its types, docs and errors are
//! read from that Rust (`mlua_ir::schema()`), so the declarations cannot drift from the code.
//!
//! - [`make_engine`] gives a ready VM; [`modules`] is the `require` registry; [`lifecycle`] is what a script starts
//!   and who may run its code; [`host`] makes every module call a traced, lifecycle-aware one.
//! - [`declarations`] writes the `.d.luau` / `.d.ts` files from the modules this crate links.
//!
//! neoc-vm is the reference this crate is ported from, module by module.

pub mod declarations;
pub mod engine;
pub mod host;
pub mod lifecycle;
pub mod modules;
mod span;

pub use engine::{load_source, make_bare_engine, make_engine, make_engine_in, prepare_script};
pub use lifecycle::{Life, VM_THREAD_STACK};

/// The `mlua` this crate is built on, re-exported so an embedder names the one binding the engine and the hooks in
/// [`lifecycle`] speak.
pub use mlua;
/// mlua-ir, re-exported: the schema (`mlua_ir::schema`) is the API's description.
pub use mlua_ir;
