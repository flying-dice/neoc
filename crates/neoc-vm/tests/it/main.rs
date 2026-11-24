#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The integration tests of `neoc-vm`, one test binary, not one per file: each test binary is a link
//! of its own.

mod engine;
mod host;
mod modules;
mod scheduler;
mod scripts;
mod strict_luau;
