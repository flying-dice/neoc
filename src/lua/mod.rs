//! Embedded Luau sandbox.
//!
//! This module is intentionally framework-free: it spawns Luau engines,
//! registers the namespaced module surface (`std:*` / `lib:*` / `vnd:*`),
//! and exposes opaque resource handles. The submodules mirror the
//! namespaces 1:1 (`crate::lua::std::collections` → `std:collections`,
//! `crate::lua::vnd::hyper` → `vnd:hyper`, etc.); `modules.rs` builds the
//! `require` registry that ties them together.
pub mod lib;
pub mod modules;
pub mod sandbox;
pub mod std;
pub mod tracing_layer;
pub mod vnd;

pub use tracing_layer::{CURRENT_RUN, LuauLayer, RunBuffers, scope};
