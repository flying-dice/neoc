#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The integration tests of `vos-host`, one test binary, not one per file: each test binary is a link
//! of its own.

mod agents;
mod app_call_limits;
mod app_frames;
mod apps;
mod cli;
mod common;
mod e2e;
mod files;
mod pairing;
mod rpc_params;
mod stack;
mod telemetry;
mod typed_luau;
