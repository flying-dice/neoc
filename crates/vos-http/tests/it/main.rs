#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The integration tests of `vos-http`, one test binary, not one per file: each test binary is a link
//! of its own.

mod http;
mod hub;
mod limits;
mod pairing_client;
