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
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
    #[error(
        "app `{0}` has no `ui` in its manifest: vos runs apps that are a page and a backend (a Vite project with an \
         `openrpc.json`, built with `bun run build:apps`). An app that drew its windows with `app.window` is no longer \
         supported: rebuild it as a page"
    )]
    NoUi(String),
    #[error("this app needs host API {needs}, this host provides {has}")]
    UnsupportedHost { needs: String, has: String },
    #[error("invalid agent: {0}")]
    InvalidAgent(String),
    #[error("the app ref now serves app `{served}`, it was installed as `{installed}`")]
    AppIdChanged { installed: String, served: String },
    #[error("the app ref changed since it was previewed (sha256 {actual}, previewed {expected}); preview it again")]
    RefChanged { expected: String, actual: String },
    #[error("package file url `{url}` is refused: {reason}")]
    UnsafeFileUrl { url: String, reason: String },
    #[error("app package is too large: {0}")]
    PackageTooLarge(String),
}
