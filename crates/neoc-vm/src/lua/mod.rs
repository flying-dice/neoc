//! Embedded Lua sandbox.
//!
//! This module is intentionally framework-free: it spawns Lua engines,
//! registers the namespaced module surface (`std:*` / `lib:*` / `vnd:*`),
//! and exposes opaque resource handles. The submodules mirror the
//! namespaces 1:1 (`crate::lua::std::collections` → `std:collections`,
//! `crate::lua::vnd::hyper` → `vnd:hyper`, etc.); `modules.rs` builds the
//! `require` registry that ties them together.
pub(crate) mod command_options;
pub mod facade;
pub mod lib;
pub mod lifecycle;
pub(crate) mod limits;
pub mod modules;
pub mod panics;
pub mod sandbox;
pub mod script_env;
pub(crate) mod serde_guard;
pub mod std;
pub mod vnd;
