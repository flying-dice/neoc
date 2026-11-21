use vos_domain::DomainError;

/// What adapters return. Adapters map their own errors (rusqlite, mlua, reqwest ...) onto this.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("not found: {0}")]
    NotFound(String),
    /// The app (or caller) did something wrong: bad SQL, a Lua error.
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// What use cases return to driving adapters (http, mcp).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error("app `{0}` is already installed")]
    AlreadyInstalled(String),
    #[error("app `{0}` is not installed")]
    NotInstalled(String),
    /// The app is installed but disabled or failed, so it cannot serve calls.
    #[error("app `{app}` is {state}")]
    NotRunning { app: String, state: String },
}
