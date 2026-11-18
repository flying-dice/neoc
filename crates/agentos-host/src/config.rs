//! Host settings: built-in defaults, overridden by an `agentos.toml`, overridden by environment variables.
//!
//! | `agentos.toml`                | environment                          | default                             |
//! |-------------------------------|--------------------------------------|-------------------------------------|
//! | `data_dir`                    | `AGENTOS_DATA`                       | `~/.agentos`                        |
//! | `bind`                        | `AGENTOS_BIND`                       | `0.0.0.0`                           |
//! | `port`                        | `AGENTOS_PORT`                       | `7400`                              |
//! | `client_dir`                  | `AGENTOS_CLIENT_DIR`                 | `client/dist`, else `apps/desktop/dist` |
//! | `registries`                  | `AGENTOS_REGISTRIES` (comma list)    | `dist/apps/registry.json` when built |
//! | `install`                     | `AGENTOS_INSTALL` (comma list)       | none: app-ref URLs installed at start |
//! | `allowed_hosts`               | `AGENTOS_ALLOWED_HOSTS` (comma list) | none                                |
//! | `dev_client_port`             | `AGENTOS_DEV_CLIENT_PORT`            | none                                |
//! | `dev_reload`                  | `AGENTOS_DEV_RELOAD`                 | off: rebuild an app's VM when its `file://` bundle changes |
//! | `secure_cookies`              | `AGENTOS_SECURE_COOKIES`             | true when TLS is configured         |
//! | `[tls] cert`, `[tls] key`     | `AGENTOS_TLS_CERT`, `AGENTOS_TLS_KEY`| TLS off                             |
//! | `[[agents]]`                  | (file only)                          | Claude, if its ACP adapter is installed |
//!
//! `[[agents]]` seed the agent roster: an entry is added at start when no agent has its `id`, and left alone
//! after that (edit agents through `agents.*` on `/rpc`). Each is an `Agent`: `id`, `name`, `role`, `avatar`,
//! `[agents.runtime] command/args/env`, `[agents.grants] all/apps` (deny by default) and `approval`.
//!
//! The file is `--config <path>`, else `AGENTOS_CONFIG`, else `./agentos.toml`, else `<data_dir>/agentos.toml`.
//! Relative paths in the file are relative to the file; relative paths in the environment, to the working
//! directory.

use agentos_domain::{Agent, AgentId, AgentRuntime, ApprovalPolicy, Grants};
use anyhow::{bail, Context};
use serde::Deserialize;
use std::{
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

pub const DEFAULT_PORT: u16 = 7400;
/// Tried in order when `client_dir` is not set: the managed client, then the pre-rename desktop build.
const DEFAULT_CLIENT_DIRS: [&str; 2] = ["client/dist", "apps/desktop/dist"];
/// The registry `bun run build:apps` writes: used when no `registries` are configured and it exists.
const DEFAULT_REGISTRY: &str = "dist/apps/registry.json";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSettings {
    data_dir: Option<PathBuf>,
    bind: Option<IpAddr>,
    port: Option<u16>,
    client_dir: Option<PathBuf>,
    registries: Option<Vec<String>>,
    install: Option<Vec<String>>,
    allowed_hosts: Option<Vec<String>>,
    dev_client_port: Option<u16>,
    dev_reload: Option<bool>,
    secure_cookies: Option<bool>,
    tls: Option<FileTls>,
    agents: Option<Vec<Agent>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTls {
    cert: PathBuf,
    key: PathBuf,
}

/// Certificate chain and private key (PEM files) for serving HTTPS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Where the built client comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientSource {
    /// Set by the operator: must exist.
    Explicit(PathBuf),
    /// Not set: the first of [`DEFAULT_CLIENT_DIRS`] that has been built; none is fine (API only).
    Default,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub bind: IpAddr,
    pub port: u16,
    pub client: ClientSource,
    /// What Discover offers: registry files (`{"apps": [...]}`) or app refs, by URL. Empty = the built
    /// registry of this checkout, if there is one (see [`Settings::catalog`]).
    pub registries: Vec<String>,
    /// App-ref URLs the host makes sure are installed at every start.
    pub install: Vec<String>,
    /// Extra `Host` names accepted (a reverse proxy, a DNS name), on any port.
    pub allowed_hosts: Vec<String>,
    /// A dev client (Vite) allowed to talk to the host from this port.
    pub dev_client_port: Option<u16>,
    /// Dev: watch the `file://` app bundles and rebuild an app's VM (keeping its database) when one changes.
    pub dev_reload: bool,
    pub secure_cookies: bool,
    pub tls: Option<TlsFiles>,
    /// Agents added to the roster at start when absent.
    pub agents: Vec<Agent>,
}

impl Settings {
    /// Load from `config_arg` (`--config`) / the environment / the usual file locations.
    pub fn load(
        config_arg: Option<&Path>,
        env: &dyn Fn(&str) -> Option<String>,
        cwd: &Path,
        home: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let env_path = |key: &str| env(key).filter(|v| !v.is_empty()).map(|v| expand(&v, cwd, home));
        let env_data_dir = env_path("AGENTOS_DATA");
        let default_data_dir = home.map_or_else(|| PathBuf::from(".agentos"), |h| h.join(".agentos"));

        let (file, file_dir) = Self::read_file(
            config_arg.map(Path::to_path_buf).or_else(|| env_path("AGENTOS_CONFIG")),
            env_data_dir.as_ref().unwrap_or(&default_data_dir),
            cwd,
        )?;
        let in_file = |p: PathBuf| expand_in(&p, &file_dir, home);

        let data_dir = env_data_dir.or(file.data_dir.map(in_file)).unwrap_or(default_data_dir);
        let bind = match env("AGENTOS_BIND").filter(|v| !v.is_empty()) {
            Some(v) => v.parse().with_context(|| format!("AGENTOS_BIND `{v}` is not an IP address"))?,
            None => file.bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        };
        let port = match env("AGENTOS_PORT").filter(|v| !v.is_empty()) {
            Some(v) => v.parse().with_context(|| format!("AGENTOS_PORT `{v}` is not a port"))?,
            None => file.port.unwrap_or(DEFAULT_PORT),
        };
        let client = match env_path("AGENTOS_CLIENT_DIR") {
            Some(dir) => ClientSource::Explicit(dir),
            None => match file.client_dir {
                Some(dir) => ClientSource::Explicit(expand_in(&dir, &file_dir, home)),
                None => ClientSource::Default,
            },
        };
        let list = |key: &str| env(key).filter(|v| !v.is_empty()).map(|v| split_list(&v));
        let dev_client_port = match env("AGENTOS_DEV_CLIENT_PORT").filter(|v| !v.is_empty()) {
            Some(v) => {
                Some(v.parse().with_context(|| format!("AGENTOS_DEV_CLIENT_PORT `{v}` is not a port"))?)
            }
            None => file.dev_client_port,
        };

        let tls = match (env_path("AGENTOS_TLS_CERT"), env_path("AGENTOS_TLS_KEY")) {
            (Some(cert), Some(key)) => Some(TlsFiles { cert, key }),
            (None, None) => file.tls.map(|t| TlsFiles {
                cert: expand_in(&t.cert, &file_dir, home),
                key: expand_in(&t.key, &file_dir, home),
            }),
            _ => bail!("AGENTOS_TLS_CERT and AGENTOS_TLS_KEY must be set together"),
        };
        let flag = |v: String| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
        let dev_reload = match env("AGENTOS_DEV_RELOAD").filter(|v| !v.is_empty()) {
            Some(v) => flag(v),
            None => file.dev_reload.unwrap_or(false),
        };
        let secure_cookies = match env("AGENTOS_SECURE_COOKIES").filter(|v| !v.is_empty()) {
            Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
            None => file.secure_cookies.unwrap_or(tls.is_some()),
        };

        Ok(Self {
            data_dir,
            bind,
            port,
            client,
            registries: list("AGENTOS_REGISTRIES").or(file.registries).unwrap_or_default(),
            install: list("AGENTOS_INSTALL").or(file.install).unwrap_or_default(),
            allowed_hosts: list("AGENTOS_ALLOWED_HOSTS").or(file.allowed_hosts).unwrap_or_default(),
            dev_client_port,
            dev_reload,
            secure_cookies,
            tls,
            agents: file.agents.unwrap_or_else(|| default_agents(cwd)),
        })
    }

    /// The config file, if any, and the directory its relative paths resolve against.
    fn read_file(
        explicit: Option<PathBuf>,
        data_dir: &Path,
        cwd: &Path,
    ) -> anyhow::Result<(FileSettings, PathBuf)> {
        let candidates: Vec<(PathBuf, bool)> = match explicit {
            Some(p) => vec![(cwd.join(p), true)],
            None => vec![(cwd.join("agentos.toml"), false), (data_dir.join("agentos.toml"), false)],
        };
        for (path, required) in candidates {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let parsed: FileSettings = toml::from_str(&text)
                        .with_context(|| format!("{}: invalid config", path.display()))?;
                    let dir = path.parent().map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
                    return Ok((parsed, dir));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => {}
                Err(e) => return Err(e).with_context(|| format!("reading config {}", path.display())),
            }
        }
        Ok((FileSettings::default(), cwd.to_path_buf()))
    }

    /// `http` or `https`.
    #[must_use]
    pub fn scheme(&self) -> &'static str {
        if self.tls.is_some() {
            "https"
        } else {
            "http"
        }
    }

    /// The registries Discover browses: the configured ones, else the registry `bun run build:apps` wrote
    /// (`dist/apps/registry.json`, next to the working directory or in the source checkout this binary was built from), as a `file://` URL.
    #[must_use]
    pub fn catalog(&self, cwd: &Path) -> Vec<String> {
        if !self.registries.is_empty() {
            return self.registries.clone();
        }
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        [cwd, source_root.as_path()]
            .iter()
            .map(|root| root.join(DEFAULT_REGISTRY))
            .find(|f| f.is_file())
            .and_then(|f| f.canonicalize().ok())
            .map(|f| format!("file://{}", f.display()))
            .into_iter()
            .collect()
    }

    /// The directory of the built client to serve, if there is one.
    pub fn resolve_client_dir(&self, cwd: &Path) -> anyhow::Result<Option<PathBuf>> {
        let built = |dir: &Path| dir.join("index.html").is_file();
        match &self.client {
            ClientSource::Explicit(dir) => {
                if built(dir) {
                    Ok(Some(dir.clone()))
                } else {
                    bail!(
                        "client_dir {} has no index.html (build the client first: `bun run build`)",
                        dir.display()
                    )
                }
            }
            ClientSource::Default => {
                // next to the working directory first, then the source checkout this binary was built from
                let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let found = [cwd, source_root.as_path()]
                    .iter()
                    .flat_map(|root| DEFAULT_CLIENT_DIRS.iter().map(move |d| root.join(d)))
                    .find(|d| built(d));
                Ok(found.map(|d| d.canonicalize().unwrap_or(d)))
            }
        }
    }
}

/// Claude through its ACP adapter, when the adapter is installed next to the working directory or in the source
/// checkout this binary was built from. It starts with no grants: tools are opt-in.
fn default_agents(cwd: &Path) -> Vec<Agent> {
    const ADAPTER: &str = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(index) = [cwd, source_root.as_path()].iter().map(|r| r.join(ADAPTER)).find(|p| p.is_file())
    else {
        return vec![];
    };
    vec![Agent {
        id: AgentId("claude".to_owned()),
        name: "Claude".to_owned(),
        avatar: None,
        role: "Claude, through the ACP adapter".to_owned(),
        runtime: AgentRuntime {
            command: "node".to_owned(),
            args: vec![index.canonicalize().unwrap_or(index).to_string_lossy().into_owned()],
            env: std::collections::BTreeMap::new(),
        },
        grants: Grants::default(),
        approval: ApprovalPolicy::default(),
    }]
}

fn split_list(v: &str) -> Vec<String> {
    v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// `~` and `~/x` -> the home directory; relative paths -> under `base`.
fn expand_in(path: &Path, base: &Path, home: Option<&Path>) -> PathBuf {
    if let (Ok(rest), Some(home)) = (path.strip_prefix("~"), home) {
        return home.join(rest);
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn expand(value: &str, base: &Path, home: Option<&Path>) -> PathBuf {
    expand_in(Path::new(value), base, home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(file: Option<&str>, env: &[(&str, &str)]) -> anyhow::Result<Settings> {
        let dir =
            std::env::temp_dir().join(format!("agentos-config-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_arg = file.map(|text| {
            std::fs::write(dir.join("agentos.toml"), text).unwrap();
            dir.join("agentos.toml")
        });
        let env: HashMap<String, String> = env.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        let r = Settings::load(
            config_arg.as_deref(),
            &|k| env.get(k).cloned(),
            &dir,
            Some(Path::new("/home/me")),
        );
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn defaults_listen_on_the_lan_at_7400_with_data_in_the_home_directory() {
        let s = load(None, &[]).unwrap();
        assert_eq!(s.port, 7400);
        assert_eq!(s.bind, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(s.data_dir, PathBuf::from("/home/me/.agentos"));
        assert_eq!(s.client, ClientSource::Default);
        assert!(s.registries.is_empty() && s.install.is_empty() && s.tls.is_none() && !s.secure_cookies);
        assert_eq!(s.scheme(), "http");
    }

    #[test]
    fn the_file_overrides_defaults_and_the_environment_overrides_the_file() {
        let toml = r#"
            data_dir = "~/agent-data"
            bind = "127.0.0.1"
            port = 8000
            registries = ["file:///apps/notes/app-ref.json"]
            install = ["file:///apps/notes/app-ref.json"]
            allowed_hosts = ["agentos.example.org"]
            [tls]
            cert = "certs/host.pem"
            key = "certs/host.key"
        "#;
        let s = load(Some(toml), &[]).unwrap();
        assert_eq!(s.data_dir, PathBuf::from("/home/me/agent-data"));
        assert_eq!((s.bind, s.port), ("127.0.0.1".parse().unwrap(), 8000));
        assert_eq!(s.registries, ["file:///apps/notes/app-ref.json"]);
        assert_eq!(s.install, ["file:///apps/notes/app-ref.json"]);
        assert!(s.tls.as_ref().unwrap().cert.ends_with("certs/host.pem"));
        assert!(s.secure_cookies, "TLS implies secure cookies");
        assert_eq!(s.scheme(), "https");

        let s = load(
            Some(toml),
            &[
                ("AGENTOS_PORT", "9000"),
                ("AGENTOS_REGISTRIES", "a, b ,,c"),
                ("AGENTOS_INSTALL", "d"),
                ("AGENTOS_BIND", "::1"),
            ],
        )
        .unwrap();
        assert_eq!((s.port, s.bind), (9000, "::1".parse().unwrap()));
        assert_eq!(s.registries, ["a", "b", "c"]);
        assert_eq!(s.install, ["d"]);
    }

    #[test]
    fn the_catalog_is_the_configured_registries_else_the_built_registry() {
        let root = std::env::temp_dir().join(format!("agentos-catalog-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dist/apps")).unwrap();
        let s = load(None, &[]).unwrap();
        std::fs::write(root.join("dist/apps/registry.json"), r#"{"apps":[]}"#).unwrap();
        let found = s.catalog(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("file://") && found[0].ends_with("dist/apps/registry.json"));
        let configured = Settings { registries: vec!["https://apps.example.org/registry.json".into()], ..s };
        assert_eq!(configured.catalog(&root), ["https://apps.example.org/registry.json"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mistakes_are_reported_not_ignored() {
        assert!(load(Some("prot = 1"), &[]).is_err(), "unknown key");
        assert!(load(None, &[("AGENTOS_PORT", "seventy")]).is_err());
        assert!(load(None, &[("AGENTOS_BIND", "everywhere")]).is_err());
        assert!(load(None, &[("AGENTOS_TLS_CERT", "/c.pem")]).is_err(), "cert without key");
        assert!(load(Some("[tls]\ncert = \"c\""), &[]).is_err(), "tls table needs both");
    }

    #[test]
    fn an_explicit_client_dir_must_be_built_but_the_default_may_be_missing() {
        let dir = std::env::temp_dir().join(format!("agentos-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let explicit = Settings { client: ClientSource::Explicit(dir.clone()), ..load(None, &[]).unwrap() };
        assert!(explicit.resolve_client_dir(&dir).is_err(), "no index.html yet");
        std::fs::write(dir.join("index.html"), "x").unwrap();
        assert_eq!(explicit.resolve_client_dir(&dir).unwrap(), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_default_client_falls_back_to_the_old_desktop_build() {
        let root = std::env::temp_dir().join(format!("agentos-fallback-{}", std::process::id()));
        std::fs::create_dir_all(root.join("apps/desktop/dist")).unwrap();
        std::fs::write(root.join("apps/desktop/dist/index.html"), "old").unwrap();
        let s = load(None, &[]).unwrap();
        let found = s.resolve_client_dir(&root).unwrap().unwrap();
        assert!(found.ends_with("apps/desktop/dist"), "{found:?}");
        std::fs::create_dir_all(root.join("client/dist")).unwrap();
        std::fs::write(root.join("client/dist/index.html"), "new").unwrap();
        assert!(
            s.resolve_client_dir(&root).unwrap().unwrap().ends_with("client/dist"),
            "client/ wins when built"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
