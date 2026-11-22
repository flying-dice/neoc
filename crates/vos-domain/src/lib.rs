//! The innermost ring of the hexagon: the pure domain model.
//!
//! App ids, app refs and manifests (with checksum verification), tool grants, agent identity, tools,
//! notifications, the view tree and the protocol messages. No I/O, no runtime, no framework: the only
//! external crates are `serde`, `serde_json`, `sha2` and `thiserror` (plus `ts-rs` and `schemars`
//! behind the derive-only `codegen` feature). `scripts/check-deps.ts` and `deny.toml` enforce that.

pub mod actor;
pub mod agent;
pub mod api;
pub mod app;
pub mod app_ui;
pub mod contact;
pub mod decl;
pub mod device;
pub mod error;
pub mod files;
pub mod limits;
pub mod merge;
pub mod notification;
pub mod protocol;
pub mod revision;
pub mod time;
pub mod tool;
pub mod tool_schema;
pub mod ui;

pub use actor::{Actor, ActorKind};
pub use agent::{AgentId, AgentIdentity, ToolGrant};
pub use app::{
    resolve_package_url, resolve_url, source_label, AppId, AppRef, FileRef, Manifest, MigrationSpec,
    Registry, Requires, GLOBALS_NOT_FOR_APPS, HOST_API, NOT_FOR_APPS,
};
pub use app_ui::{AppUi, Csp, Permissions};
pub use contact::{hash_token, hex_of, same_hash, Agent, AgentRuntime, ApprovalPolicy, Grants};
pub use decl::{AppDecl, KitRequirement, WindowDecl};
pub use device::{Device, DeviceId};
pub use error::DomainError;
pub use files::{FileError, FileErrorKind, FileInfo, FileKind, FilePath, FileResult, Location};
pub use limits::PackageBudget;
pub use merge::{merge3, Merge};
pub use notification::Notification;
pub use revision::{Revision, RevisionReason, StoredRevision, REVISION_RETENTION_MS};
pub use tool::{QualifiedToolName, ToolSpec};
