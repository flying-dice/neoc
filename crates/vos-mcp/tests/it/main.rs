#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The integration tests of `vos-mcp`, one test binary, not one per file: each test binary is a link
//! of its own.

mod agents;
mod mcp;
