//! The application ring of the hexagon: ports and use cases.
//!
//! Owns the PORTS (what the core needs from the outside world: app runtime, data store, tool registry,
//! fetcher, agent connector, device registry ...) and the USE CASES that orchestrate domain objects
//! through those ports. Adapter crates implement the ports; driving adapters (`http`, `mcp`) call the
//! use cases.
//!
//! Dependency rule: this crate may depend on `agentos-domain` and on tiny, runtime-free libraries.
//! It must never depend on an adapter crate, nor on tokio, axum, mlua, rusqlite, ... (a test runtime
//! is a dev-dependency only).

pub mod error;
pub mod ports;
pub mod services;
#[cfg(any(test, feature = "testing"))]
#[allow(clippy::unwrap_used, clippy::missing_panics_doc)]
// in-memory fakes: a poisoned lock should just panic
pub mod testing;

pub use error::{AppError, PortError};
pub use services::AppService;
