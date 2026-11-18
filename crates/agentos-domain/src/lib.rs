//! The innermost ring of the hexagon: the pure domain model.
//!
//! App ids, app refs and manifests (with checksum verification), permissions, agent identity, tools,
//! notifications, the view tree and the protocol messages. No I/O, no runtime, no framework: the only
//! external crates are `serde`, `serde_json`, `sha2` and `thiserror` (plus `ts-rs` and `schemars`
//! behind the derive-only `codegen` feature). `scripts/check-deps.ts` and `deny.toml` enforce that.

pub mod agent;
pub mod app;
pub mod error;
pub mod notification;
pub mod protocol;
pub mod tool;
pub mod ui;

pub use agent::{AgentId, AgentIdentity, Permission};
pub use app::{AppId, AppRef, FileRef, Manifest, MigrationSpec};
pub use error::DomainError;
pub use notification::Notification;
pub use tool::{QualifiedToolName, ToolSpec};
