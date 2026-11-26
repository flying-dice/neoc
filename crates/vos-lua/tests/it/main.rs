#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The integration tests of `vos-lua`, one test binary, not one per file: each test binary is a link
//! of its own.

mod agents;
mod api;
mod baseline;
mod calculator;
mod call_invariants;
mod chaos;
mod common;
mod files;
mod handlers;
mod hardening;
mod host_files;
mod luau;
mod modules;
mod panics;
mod peers;
mod poison;
mod router;
mod runtime;
mod streams;
mod surface;
mod terminal;
mod todos;
mod turns;
mod typescript;
mod vm;
