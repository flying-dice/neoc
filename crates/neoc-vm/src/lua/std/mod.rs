//! `std:*` modules — mirror the Rust standard library.
//!
//! Anything you'd reach for in `std::` belongs here, plus the tokio
//! exception (`std:workers`) which is the worker-pool substrate with no
//! direct `std::` analog. Modules expose a `pub fn module(lua: &Lua) ->
//! Result<Table>` and are wired into the `require` registry by
//! `crate::lua::modules::install`.

pub mod collections;
pub mod env;
pub mod fs;
pub mod io;
pub mod net;
pub mod osstr;
pub mod path;
pub mod task;
pub mod thread;
pub mod time;
pub mod workers;
