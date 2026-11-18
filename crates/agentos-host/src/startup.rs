//! Things the binary does around [`crate::build`]: local MCP discovery and installing the configured apps.

use agentos_application::AppService;
use anyhow::Context;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Write `<data>/mcp.json` (mode 0600) so local MCP clients can find `/mcp` and its bearer token, in
/// the shape MCP clients take as a server entry: `{ "url": ..., "headers": { "Authorization": ... } }`.
///
/// The token is for **agents**, not browsers: a paired session cookie is not an MCP credential and this
/// token is not a session.
pub fn write_mcp_json(data_dir: &Path, url: &str, token: &str) -> anyhow::Result<PathBuf> {
    let path = data_dir.join("mcp.json");
    let body = serde_json::to_string_pretty(
        &json!({ "url": url, "headers": { "Authorization": format!("Bearer {token}") } }),
    )?;
    write_private(&path, body.as_bytes()).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Create (or replace) a file readable and writable by its owner only, with no window where it is wider.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // a pre-existing file keeps its old mode through `open`
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)
}

/// Create the data directory (and `apps/` below it) readable by this user only.
pub fn prepare_data_dir(data_dir: &Path) -> anyhow::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(data_dir.join("apps")).with_context(|| format!("creating {}", data_dir.display()))
}

/// Install every configured registry entry (an app-ref URL). A failure is logged and skipped: one broken
/// URL must not keep the host from starting.
pub async fn install_registries(service: &AppService, urls: &[String]) {
    for url in urls {
        match service.install_from_url(url).await {
            Ok(app) => tracing::info!(%app, %url, "installed app"),
            Err(e) => tracing::warn!(%url, error = %e, "could not install app from registry"),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn mcp_json_is_private_and_holds_the_url_and_bearer_token() {
        let dir = std::env::temp_dir().join(format!("agentos-mcpjson-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), "old and world-readable").unwrap();
        std::fs::set_permissions(dir.join("mcp.json"), std::fs::Permissions::from_mode(0o644)).unwrap();

        let path = write_mcp_json(&dir, "http://127.0.0.1:7400/mcp", "s3cret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "an existing file is tightened too");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["url"], "http://127.0.0.1:7400/mcp");
        assert_eq!(v["headers"]["Authorization"], "Bearer s3cret");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_data_dir_is_private_to_this_user() {
        let dir = std::env::temp_dir().join(format!("agentos-datadir-{}", std::process::id()));
        prepare_data_dir(&dir).unwrap();
        assert!(dir.join("apps").is_dir());
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(dir);
    }
}
