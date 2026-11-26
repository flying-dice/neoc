//! The innermost ring of the hexagon: the pure domain model.
//!
//! App ids, app refs and manifests (with checksum verification), tool grants, agent identity, tools,
//! notifications and the protocol messages. No I/O, no runtime, no framework: the only
//! external crates are `serde`, `serde_json`, `sha2` and `thiserror` (plus `ts-rs`
//! behind the derive-only `codegen` feature). `scripts/check-deps.ts` and `deny.toml` enforce that.

pub mod actor;
pub mod agent;
pub mod api;
pub mod app;
pub mod app_ui;
pub mod contact;
pub mod device;
pub mod error;
pub mod files;
pub mod limits;
pub mod notification;
pub mod protocol;
pub mod time;
pub mod tool;
pub mod version;

pub use actor::{Actor, ActorKind};
pub use agent::{AgentId, AgentIdentity, ToolGrant};
pub use app::{
    resolve_package_url, resolve_url, source_label, AppId, AppRef, FileRef, KitRequirement, Manifest,
    MigrationSpec, Registry, Requires, GLOBALS_NOT_FOR_APPS, HOST_API, NOT_FOR_APPS,
};
pub use app_ui::{AppUi, Csp, Permissions, WindowDecl};
pub use contact::{hash_token, hex_of, same_hash, Agent, Grants};
pub use device::{Device, DeviceId};
pub use error::DomainError;
pub use files::{FileError, FileErrorKind, FileInfo, FileKind, FilePath, FileResult, HostPath, Location};
pub use limits::PackageBudget;
pub use notification::Notification;
pub use tool::{QualifiedToolName, ToolSpec};
pub use version::{KitVersion, KIT_VERSION, PROTO_VERSION};
