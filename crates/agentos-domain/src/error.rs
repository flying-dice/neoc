#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("invalid app id `{0}` (lowercase a-z, 0-9, '-', 3..=40 chars)")]
    InvalidAppId(String),
    #[error("invalid tool name `{0}`")]
    InvalidToolName(String),
    #[error("checksum mismatch for `{path}`: expected {expected}, got {actual}")]
    ChecksumMismatch { path: String, expected: String, actual: String },
    #[error("app ref has no file `{0}`")]
    MissingFile(String),
    #[error("permission denied: {agent} may not {action}")]
    PermissionDenied { agent: String, action: String },
    #[error("migrations must be numbered 1..N without gaps")]
    MigrationOrder,
    #[error("invalid app.manifest: {0}")]
    InvalidManifest(String),
}
