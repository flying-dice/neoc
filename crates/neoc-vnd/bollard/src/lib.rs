//! The Docker Engine API: `vnd:bollard` (the `bollard` crate).
//!
//! ```lua
//! local bollard = require("vnd:bollard")
//!
//! local docker = assert(bollard.connect())
//! for _, c in assert(docker:list_containers({ all = true })) do
//!     print(c.Names and c.Names[1], c.State)
//! end
//!
//! local made = assert(docker:create_container({
//!     Image = "nginx:1.27",
//!     Labels = { app = "web" },
//!     HostConfig = {
//!         PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } },
//!         RestartPolicy = { Name = "unless-stopped" },
//!     },
//! }, { name = "web" }))
//! assert(docker:start_container(made.Id))
//!
//! for piece in docker:logs("web", { follow = true, tail = 100 }):items() do
//!     print(piece.stream, piece.data)
//! end
//! ```
//!
//! **Docker's own shapes.** What Docker returns and what it takes as a body (the configuration of
//! `create_container`, `create_volume`, `create_network`) are bollard's own models, with Docker's field names
//! (`Id`, `Names`, `State`, `HostConfig.PortBindings`): tables typed field by field, read from the models
//! (`mlua_ir::Foreign`), so a field Docker does not have is refused and the declarations know every one. A JSON
//! `null` is `nil`. This module's own options are lower-case (`all`, `force`, `filters`), and an option a call
//! does not have is refused.
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, err)`, where `err` is a
//! [`DockerError`](error::DockerError): its `kind` is stable, and what Docker said is its `reason`. `connect` is the
//! kind of a host with no Docker: there is no socket, or nothing answers on it. An argument of the wrong type, a
//! name that is not one, an option a call does not have are raised, as everywhere.
//!
//! **Connecting.** `connect()` makes a client and sends nothing: the first call does. With no `host` it uses
//! `DOCKER_HOST`, and without that the local socket: `/var/run/docker.sock`, then the per-user sockets of Docker
//! Desktop (`~/.docker/run/docker.sock`) and of a rootless daemon (`$XDG_RUNTIME_DIR/docker.sock`); on Windows,
//! Docker's named pipe. A client's first call asks the daemon for its API version first, so that an older daemon is
//! spoken to in a version it has.
//!
//! **What reaching Docker means.** Whoever can use the Docker socket can run a container that mounts the host's
//! root: it is root on the host. Nothing here narrows that, as nothing narrows `std:fs` or `vnd:hyper`: a script
//! that is given this module is trusted with the machine.
//!
//! **Streams.** Logs, events and the progress of a pull are streams (`stream`): `next()` waits for the next item,
//! `(nil, nil)` is the end, `items()` iterates. A stream belongs to the script that made it and stops with it.
//!
//! The API's rules:
//! - An error is a typed [`DockerError`](error::DockerError) (`err.kind`, `err.reason`, `err.status`).
//! - Docker's bodies and answers are typed with bollard's models, so a body with a field Docker does not have, or one
//!   of the wrong type, is refused as a wrong argument with its path. An empty table in a body is an empty list or map,
//!   which is what the field is.
//! - `create_container` takes Docker's body and, after it, the call's own `{ name, platform }`.
//! - A name, an image reference, a signal, an option that is refused is raised as a wrong argument with its place.
//! - A filter's values are always a list (`status = { "running" }`).
//! - TLS is `tls = { ca, cert, key }`.
//! - `logs`, `events` and `pull_image` return the stream itself (they cannot fail before the first `next()`), and a
//!   stream has `items()`.
//! - `exec` with an empty command is refused as `invalid_input`, and what it returns is a typed table.
//! - A volume that is returned (`list_volumes`, `create_volume`) is this module's own record of Docker's volume, with
//!   Docker's field names and its date as text: bollard's model cannot be read field by field yet. It leaves out
//!   `Status` and `ClusterVolume`.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

pub mod args;
pub mod error;
mod stream;

#[cfg(test)]
mod tests;

#[mlua_ir::module("vnd:bollard")]
pub mod bollard {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use bollard::container::LogOutput;
    use bollard::exec::StartExecResults;
    use bollard::models::{
        self, ContainerCreateBody, ContainerCreateResponse, ContainerInspectResponse, ContainerStatsResponse,
        ContainerSummary, CreateImageInfo, EventMessage, ExecConfig, ImageDeleteResponseItem, ImageSummary,
        Network, NetworkCreateRequest, NetworkCreateResponse, SystemInfo, SystemVersion, VolumeCreateRequest,
    };
    use bollard::query_parameters as query;
    use bollard::{API_DEFAULT_VERSION, Docker};
    use futures::StreamExt;
    use mlua::Lua;
    use mlua_ir::{Foreign, Object, Shape, Stream};
    use serde::{Deserialize, Serialize};
    use time::format_description::well_known::Rfc3339;
    use tokio::sync::OnceCell;

    use super::args::{
        ConnectOptions, ContainerName, CreateContainer, Events, ExecOptions, Filtered, ImageRef, Kill,
        ListContainers, ListImages, Logs, NetworkName, Pull, RemoveContainer, RemoveImage, RemoveVolume,
        Stop, VolumeName, clamped,
    };
    use super::error::DockerError;
    use super::stream;
    use neoc_vm_core::lifecycle::Life;
    use neoc_vnd_hyper::Body as RawBytes;

    /// How long a call waits for the daemon's answer when `connect` is given no `timeout`: bollard's own default.
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

    /// The most output `exec` reads into memory (both streams together): `vnd:hyper`'s default body cap.
    const MAX_EXEC_OUTPUT: usize = 64 * 1024 * 1024;

    // -----------------------------------------------------------------------------------------------------------
    // Connecting
    // -----------------------------------------------------------------------------------------------------------

    /// A client for one daemon. Cheap to clone: every clone is the same client.
    #[derive(Clone, Object)]
    pub struct Client {
        inner: Arc<ClientInner>,
    }

    struct ClientInner {
        docker: Docker,
        /// Set once the client and the daemon agreed on an API version (see [`Client::api`]).
        negotiated: OnceCell<()>,
    }

    impl Client {
        /// The bollard client, after its first use asked the daemon which API version it speaks, so that the two
        /// use the newest both have: a daemon refuses a request that names a version newer than its own. (bollard
        /// 0.20 happens to send no version at all, which a daemon answers in its own; the question is asked so
        /// that this module does not depend on that.) A failed attempt (no daemon yet) is made again by the next
        /// call.
        async fn api(&self) -> Result<Docker, DockerError> {
            self.inner
                .negotiated
                .get_or_try_init(|| async {
                    // a clone shares the version the negotiation sets
                    self.inner.docker.clone().negotiate_version().await?;
                    Ok::<(), DockerError>(())
                })
                .await?;
            Ok(self.inner.docker.clone())
        }

        /// A stream of what `source` yields. The method that returns it does not wait: the stream's task makes
        /// the request, after the client settled its API version.
        fn stream<T, S>(
            &self,
            lua: &Lua,
            source: impl FnOnce(&Docker) -> S + Send + 'static,
        ) -> Stream<T, DockerError>
        where
            T: Send + 'static,
            S: futures::Stream<Item = Result<T, DockerError>> + Send + 'static,
        {
            let client = self.clone();
            stream::reading(&Life::of(lua), async move {
                let docker = client.api().await?;
                Ok(source(&docker))
            })
        }
    }

    /// A client for a Docker daemon. Nothing is sent until the client's first call, so a host nothing answers on
    /// is the `connect` error of that call; a host with no Docker socket at all is the `connect` error of this
    /// one. Reaching Docker is root on its host: a script given this module is trusted with the machine.
    ///
    /// ```luau
    /// local docker = assert(bollard.connect())
    /// local remote = assert(bollard.connect({ host = "tcp://10.0.0.5:2375", timeout = 10 }))
    /// local none, err = bollard.connect({ host = "unix:///nowhere/docker.sock" })
    /// assert(none == nil and err ~= nil and err.kind == "connect")
    /// ```
    pub fn connect(opts: ConnectOptions) -> Result<Client, DockerError> {
        let docker = open(opts)?;
        Ok(Client { inner: Arc::new(ClientInner { docker, negotiated: OnceCell::new() }) })
    }

    fn open(opts: ConnectOptions) -> Result<Docker, DockerError> {
        let timeout = match opts.timeout {
            None => DEFAULT_TIMEOUT,
            Some(secs) if secs.is_finite() && secs > 0.0 => Duration::try_from_secs_f64(secs)
                .map_err(|_| DockerError::invalid_input("option \"timeout\" is too long"))?,
            Some(_) => {
                return Err(DockerError::invalid_input("option \"timeout\" must be more than zero seconds"));
            }
        };
        let host = match opts.host {
            Some(host) => host,
            None => default_host()?,
        };
        // a bare path is the socket at that path
        let host = if host.starts_with('/') { format!("unix://{host}") } else { host };
        let docker = match opts.tls {
            None => Docker::connect_with_host(&host)?,
            Some(tls) => Docker::connect_with_ssl(
                &host,
                Path::new(&tls.key),
                Path::new(&tls.cert),
                Path::new(&tls.ca),
                DEFAULT_TIMEOUT.as_secs(),
                API_DEFAULT_VERSION,
            )?,
        };
        Ok(docker.with_timeout(timeout))
    }

    /// `DOCKER_HOST`, or where a local daemon listens.
    fn default_host() -> Result<String, DockerError> {
        if let Some(host) = std::env::var("DOCKER_HOST").ok().filter(|host| !host.is_empty()) {
            return Ok(host);
        }
        local_host()
    }

    /// Docker's named pipe: a pipe is not a file to look for, so a daemon that is not there is the `connect` error
    /// of the client's first call.
    #[cfg(windows)]
    #[allow(clippy::unnecessary_wraps)] // the other platforms' can fail
    fn local_host() -> Result<String, DockerError> {
        Ok("npipe:////./pipe/docker_engine".to_owned())
    }

    /// The first local socket that exists.
    #[cfg(not(windows))]
    fn local_host() -> Result<String, DockerError> {
        let sockets = local_sockets();
        match sockets.iter().find(|socket| socket.exists()) {
            Some(socket) => Ok(format!("unix://{}", socket.display())),
            None => {
                let tried: Vec<String> = sockets.iter().map(|socket| socket.display().to_string()).collect();
                Err(DockerError::Connect {
                    reason: format!(
                        "there is no Docker socket ({}): is Docker installed and running? Set DOCKER_HOST, or pass `host`, for a daemon elsewhere",
                        tried.join(", ")
                    ),
                })
            }
        }
    }

    /// Where a local daemon listens, most common first: the system socket, Docker Desktop's per-user socket (macOS
    /// and Linux, when its system-wide link is off) and a rootless daemon's.
    #[cfg_attr(windows, allow(dead_code))]
    fn local_sockets() -> Vec<PathBuf> {
        let mut sockets = vec![PathBuf::from("/var/run/docker.sock")];
        if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
            sockets.push(Path::new(&home).join(".docker/run/docker.sock"));
        }
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
            sockets.push(Path::new(&runtime).join("docker.sock"));
        }
        sockets
    }

    // -----------------------------------------------------------------------------------------------------------
    // Calls
    // -----------------------------------------------------------------------------------------------------------

    /// Which of a container's outputs a piece came from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Output {
        /// Its standard output.
        Stdout,
        /// Its standard error.
        Stderr,
        /// What was written to its standard input.
        Stdin,
        /// Everything a container with a TTY writes: the terminal has one output.
        Console,
    }

    /// A piece of a container's output.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct LogPiece {
        /// Which output it came from.
        pub stream: Output,
        /// What was written, byte for byte.
        pub data: RawBytes,
    }

    fn log_piece(piece: Result<LogOutput, bollard::errors::Error>) -> Result<LogPiece, DockerError> {
        let (stream, data) = match piece? {
            LogOutput::StdOut { message } => (Output::Stdout, message),
            LogOutput::StdErr { message } => (Output::Stderr, message),
            LogOutput::StdIn { message } => (Output::Stdin, message),
            LogOutput::Console { message } => (Output::Console, message),
        };
        Ok(LogPiece { stream, data: RawBytes(data.to_vec()) })
    }

    /// What a command run by `exec` left.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Exec {
        /// How the command exited; none when Docker does not know (the container stopped under it).
        pub exit_code: Option<i64>,
        /// What it wrote to its standard output, byte for byte.
        pub stdout: RawBytes,
        /// What it wrote to its standard error, byte for byte.
        pub stderr: RawBytes,
    }

    /// Where a volume is known: on its host only, or across a swarm.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum VolumeScope {
        /// On the host it was made on.
        Local,
        /// On every node of the swarm.
        Global,
    }

    /// How much a volume holds and how many containers use it. Docker only counts them for its disk usage report.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "PascalCase")]
    pub struct VolumeUsage {
        /// The bytes the volume holds; `-1` when Docker did not count them.
        pub size: i64,
        /// The containers that use the volume; `-1` when Docker did not count them.
        pub ref_count: i64,
    }

    /// A volume, with Docker's field names.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "PascalCase")]
    pub struct Volume {
        /// The volume's name.
        pub name: String,
        /// The driver that keeps it (`local`).
        pub driver: String,
        /// Where it is on the host.
        pub mountpoint: String,
        /// When it was made, as RFC 3339 text (`2024-05-01T10:00:00Z`).
        pub created_at: Option<String>,
        /// Its labels.
        pub labels: HashMap<String, String>,
        /// Where it is known.
        pub scope: Option<VolumeScope>,
        /// The options its driver was given.
        pub options: HashMap<String, String>,
        /// Its size and use, when Docker counted them.
        pub usage_data: Option<VolumeUsage>,
    }

    impl TryFrom<models::Volume> for Volume {
        type Error = DockerError;

        fn try_from(volume: models::Volume) -> Result<Self, DockerError> {
            let created_at =
                volume.created_at.map(|date| date.format(&Rfc3339)).transpose().map_err(|e| {
                    DockerError::BadResponse { reason: format!("the volume's CreatedAt is no date: {e}") }
                })?;
            Ok(Self {
                name: volume.name,
                driver: volume.driver,
                mountpoint: volume.mountpoint,
                created_at,
                labels: volume.labels,
                scope: match volume.scope {
                    Some(models::VolumeScopeEnum::LOCAL) => Some(VolumeScope::Local),
                    Some(models::VolumeScopeEnum::GLOBAL) => Some(VolumeScope::Global),
                    Some(models::VolumeScopeEnum::EMPTY) | None => None,
                },
                options: volume.options,
                usage_data: volume
                    .usage_data
                    .map(|usage| VolumeUsage { size: usage.size, ref_count: usage.ref_count }),
            })
        }
    }

    /// What `list_volumes` returns: Docker's `{ Volumes, Warnings }`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "PascalCase")]
    pub struct VolumeList {
        /// The volumes.
        pub volumes: Vec<Volume>,
        /// What Docker warns of while listing them.
        pub warnings: Vec<String>,
    }

    #[mlua_ir::object]
    impl Client {
        /// Ask the daemon whether it is there: `true`, or the error (`connect` when no daemon answers).
        pub async fn ping(&self) -> Result<(), DockerError> {
            self.api().await?.ping().await?;
            Ok(())
        }

        /// The daemon's version: Docker's `/version` (`Version`, `ApiVersion`, `Os`, `Arch`, ...).
        pub async fn version(&self) -> Result<Foreign<SystemVersion>, DockerError> {
            Ok(Foreign(self.api().await?.version().await?))
        }

        /// The daemon's system information: Docker's `/info` (`Containers`, `Images`, `ServerVersion`,
        /// `OperatingSystem`, ...).
        pub async fn info(&self) -> Result<Foreign<SystemInfo>, DockerError> {
            Ok(Foreign(self.api().await?.info().await?))
        }

        /// The containers, as Docker's summaries (`Id`, `Names`, `Image`, `State`, `Status`, `Labels`, `Ports`,
        /// ...). Only running ones unless `all`.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local blog = assert(docker:list_containers({
        ///     all = true,
        ///     filters = { label = { "com.docker.compose.project=blog" } },
        /// }))
        /// for _, container in blog do
        ///     print(container.Id, container.State)
        /// end
        /// ```
        pub async fn list_containers(
            &self,
            opts: ListContainers,
        ) -> Result<Foreign<Vec<ContainerSummary>>, DockerError> {
            let options = query::ListContainersOptions {
                all: opts.all,
                limit: opts.limit.map(clamped),
                size: opts.size,
                filters: opts.filters,
            };
            Ok(Foreign(self.api().await?.list_containers(Some(options)).await?))
        }

        /// Docker's inspection of a container, by name or id (`Id`, `State`, `Config.Labels`, `HostConfig`,
        /// `NetworkSettings`, ...), or `(nil, nil)` when there is no such container.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local web, err = docker:inspect_container("web")
        /// if web ~= nil then
        ///     print(web.State and web.State.Status)
        /// elseif err == nil then
        ///     print("there is no web")
        /// end
        /// ```
        pub async fn inspect_container(
            &self,
            name: ContainerName,
        ) -> Result<Option<Foreign<ContainerInspectResponse>>, DockerError> {
            let inspected = self
                .api()
                .await?
                .inspect_container(&name.0, None::<query::InspectContainerOptions>)
                .await
                .map_err(DockerError::from);
            match inspected {
                Ok(container) => Ok(Some(Foreign(container))),
                Err(DockerError::NotFound { .. }) => Ok(None),
                Err(e) => Err(e),
            }
        }

        /// Create a container (not started) from Docker's create body, with Docker's field names. Returns
        /// `{ Id, Warnings }`. The image must be on the host already (`pull_image`): a missing one is `not_found`;
        /// a name in use is `conflict`.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local made = assert(docker:create_container({
        ///     Image = "nginx:1.27",
        ///     Env = { "A=1" },
        ///     HostConfig = {
        ///         Binds = { "/srv/www:/usr/share/nginx/html:ro" },
        ///         PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } },
        ///         RestartPolicy = { Name = "unless-stopped" },
        ///     },
        /// }, { name = "web" }))
        /// assert(docker:start_container(made.Id))
        /// ```
        pub async fn create_container(
            &self,
            body: Foreign<ContainerCreateBody>,
            opts: CreateContainer,
        ) -> Result<Foreign<ContainerCreateResponse>, DockerError> {
            let options = query::CreateContainerOptions {
                name: opts.name.map(|name| name.0),
                platform: opts.platform.unwrap_or_default(),
            };
            Ok(Foreign(self.api().await?.create_container(Some(options), body.0).await?))
        }

        /// Start a container. Starting a running one succeeds.
        pub async fn start_container(&self, name: ContainerName) -> Result<(), DockerError> {
            self.api().await?.start_container(&name.0, None::<query::StartContainerOptions>).await?;
            Ok(())
        }

        /// Stop a container: Docker sends `signal`, waits `timeout` seconds and then kills it. Stopping a stopped
        /// one succeeds.
        pub async fn stop_container(&self, name: ContainerName, opts: Stop) -> Result<(), DockerError> {
            let options =
                query::StopContainerOptions { signal: opts.signal.map(|signal| signal.0), t: opts.timeout };
            self.api().await?.stop_container(&name.0, Some(options)).await?;
            Ok(())
        }

        /// Stop a container as `stop_container` does and start it again.
        pub async fn restart_container(&self, name: ContainerName, opts: Stop) -> Result<(), DockerError> {
            let options = query::RestartContainerOptions {
                signal: opts.signal.map(|signal| signal.0),
                t: opts.timeout,
            };
            self.api().await?.restart_container(&name.0, Some(options)).await?;
            Ok(())
        }

        /// Send a signal to a running container (`SIGKILL` unless `signal`; `SIGHUP` to make a server reload). A
        /// container that is not running is `conflict`.
        pub async fn kill_container(&self, name: ContainerName, opts: Kill) -> Result<(), DockerError> {
            let options = query::KillContainerOptions {
                signal: opts.signal.map_or_else(|| "SIGKILL".to_owned(), |signal| signal.0),
            };
            self.api().await?.kill_container(&name.0, Some(options)).await?;
            Ok(())
        }

        /// Suspend every process of a running container.
        pub async fn pause_container(&self, name: ContainerName) -> Result<(), DockerError> {
            self.api().await?.pause_container(&name.0).await?;
            Ok(())
        }

        /// Resume a paused container.
        pub async fn unpause_container(&self, name: ContainerName) -> Result<(), DockerError> {
            self.api().await?.unpause_container(&name.0).await?;
            Ok(())
        }

        /// Remove a container. A running one is `conflict` unless `force`, which kills it first.
        pub async fn remove_container(
            &self,
            name: ContainerName,
            opts: RemoveContainer,
        ) -> Result<(), DockerError> {
            let options = query::RemoveContainerOptions { force: opts.force, v: opts.volumes, link: false };
            self.api().await?.remove_container(&name.0, Some(options)).await?;
            Ok(())
        }

        /// A container's output, as a stream of pieces (a container with a TTY sends everything as `"console"`).
        /// The stream ends after what there is, or with `follow` stays open for new output until the container
        /// stops. A container that does not exist is the error of the first `next()`.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local logs = docker:logs("web", { tail = 100 })
        /// while true do
        ///     local piece, err = logs:next()
        ///     if piece == nil then
        ///         assert(err == nil, tostring(err))
        ///         break
        ///     end
        ///     print(piece.stream, piece.data)
        /// end
        /// ```
        pub fn logs(&self, lua: &Lua, name: ContainerName, opts: Logs) -> Stream<LogPiece, DockerError> {
            let options = query::LogsOptions {
                follow: opts.follow,
                stdout: opts.stdout,
                stderr: opts.stderr,
                since: opts.since.map_or(0, clamped),
                until: opts.until.map_or(0, clamped),
                timestamps: opts.timestamps,
                tail: opts.tail.map_or_else(|| "all".to_owned(), |lines| lines.to_string()),
            };
            self.stream(lua, move |docker| docker.logs(&name.0, Some(options)).map(log_piece))
        }

        /// One reading of a container's resource use: Docker's stats (`cpu_stats`, `memory_stats`, `networks`,
        /// `blkio_stats`, ...).
        pub async fn stats(
            &self,
            name: ContainerName,
        ) -> Result<Foreign<ContainerStatsResponse>, DockerError> {
            let options = query::StatsOptions { stream: false, one_shot: true };
            let reading = self.api().await?.stats(&name.0, Some(options)).next().await;
            match reading {
                Some(reading) => Ok(Foreign(reading?)),
                None => Err(DockerError::BadResponse { reason: "Docker sent no stats".to_owned() }),
            }
        }

        /// Run a command in a running container and wait for it: the program, then its arguments. Returns its exit
        /// code and what it wrote (at most 64 MiB together, `too_large` past that); a command that fails is a
        /// result with a non-zero `exit_code`, not an error.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local ran = assert(docker:exec("web", { "nginx", "-t" }))
        /// assert(ran.exit_code == 0, ran.stderr)
        /// ```
        pub async fn exec(
            &self,
            name: ContainerName,
            cmd: Vec<String>,
            opts: ExecOptions,
        ) -> Result<Exec, DockerError> {
            if cmd.is_empty() {
                return Err(DockerError::invalid_input(
                    "the command must be a list of strings: the program, then its arguments",
                ));
            }
            let config = ExecConfig {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd),
                env: opts.env,
                user: opts.user,
                working_dir: opts.working_dir,
                privileged: opts.privileged,
                ..Default::default()
            };
            exec(self.api().await?, name.0, config).await
        }

        /// The images on the host, as Docker's summaries (`Id`, `RepoTags`, `Size`, `Created`, `Containers`, ...).
        pub async fn list_images(&self, opts: ListImages) -> Result<Foreign<Vec<ImageSummary>>, DockerError> {
            let options = query::ListImagesOptions {
                all: opts.all,
                digests: opts.digests,
                filters: opts.filters,
                ..Default::default()
            };
            Ok(Foreign(self.api().await?.list_images(Some(options)).await?))
        }

        /// Pull an image from its registry: a stream of Docker's progress (`status`, `id`,
        /// `progressDetail = { current, total }`), which ends when the pull is over. `nginx` pulls `nginx:latest`,
        /// never every tag. The pull runs as the stream is read and stops if the stream is closed; a pull that
        /// fails (no such image, no network) is the error of a `next()`.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// for progress in docker:pull_image("nginx:1.27"):items() do
        ///     print(progress.status)
        /// end
        /// ```
        pub fn pull_image(
            &self,
            lua: &Lua,
            image: ImageRef,
            opts: Pull,
        ) -> Stream<Foreign<CreateImageInfo>, DockerError> {
            let (repository, tag) = image.pull_target();
            let options = query::CreateImageOptions {
                from_image: Some(repository),
                tag,
                platform: opts.platform.unwrap_or_default(),
                ..Default::default()
            };
            // a pull that fails does so inside a 200 response, as a progress line: bollard makes it an error
            self.stream(lua, move |docker| {
                docker.create_image(Some(options), None, None).map(|progress| Ok(Foreign(progress?)))
            })
        }

        /// Remove an image, by reference or id: Docker's list of what was untagged and deleted. An image a
        /// container uses is `conflict` unless `force`.
        pub async fn remove_image(
            &self,
            image: ImageRef,
            opts: RemoveImage,
        ) -> Result<Foreign<Vec<ImageDeleteResponseItem>>, DockerError> {
            let options =
                query::RemoveImageOptions { force: opts.force, noprune: opts.noprune, ..Default::default() };
            Ok(Foreign(self.api().await?.remove_image(&image.0, Some(options), None).await?))
        }

        /// The volumes: `{ Volumes, Warnings }`, each volume with Docker's field names (`Name`, `Driver`,
        /// `Mountpoint`, `Labels`, ...).
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// for _, volume in assert(docker:list_volumes({ filters = { label = { "app=web" } } })).Volumes do
        ///     print(volume.Name, volume.Mountpoint)
        /// end
        /// ```
        pub async fn list_volumes(&self, opts: Filtered) -> Result<VolumeList, DockerError> {
            let options = query::ListVolumesOptions { filters: opts.filters };
            let listed = self.api().await?.list_volumes(Some(options)).await?;
            let volumes = listed.volumes.unwrap_or_default().into_iter().map(Volume::try_from);
            Ok(VolumeList {
                volumes: volumes.collect::<Result<_, _>>()?,
                warnings: listed.warnings.unwrap_or_default(),
            })
        }

        /// Create a volume from Docker's create body (`{ Name = "data", Labels = { ... } }`; without a `Name`
        /// Docker makes one up). Returns the volume. Creating one that exists returns it.
        pub async fn create_volume(&self, body: Foreign<VolumeCreateRequest>) -> Result<Volume, DockerError> {
            self.api().await?.create_volume(body.0).await?.try_into()
        }

        /// Remove a volume. One a container uses is `conflict`.
        pub async fn remove_volume(&self, name: VolumeName, opts: RemoveVolume) -> Result<(), DockerError> {
            let options = query::RemoveVolumeOptions { force: opts.force };
            self.api().await?.remove_volume(&name.0, Some(options)).await?;
            Ok(())
        }

        /// The networks, as Docker's list (`Id`, `Name`, `Driver`, `Scope`, `Labels`, ...).
        pub async fn list_networks(&self, opts: Filtered) -> Result<Foreign<Vec<Network>>, DockerError> {
            let options = query::ListNetworksOptions { filters: opts.filters };
            Ok(Foreign(self.api().await?.list_networks(Some(options)).await?))
        }

        /// Create a network from Docker's create body (`{ Name = "backend", Driver = "bridge", Labels = { ... } }`).
        /// Returns `{ Id, Warning }`. A name in use is `conflict`.
        pub async fn create_network(
            &self,
            body: Foreign<NetworkCreateRequest>,
        ) -> Result<Foreign<NetworkCreateResponse>, DockerError> {
            Ok(Foreign(self.api().await?.create_network(body.0).await?))
        }

        /// Remove a network, by name or id. One with containers attached is an error (`api`: Docker answers 403).
        pub async fn remove_network(&self, name: NetworkName) -> Result<(), DockerError> {
            self.api().await?.remove_network(&name.0).await?;
            Ok(())
        }

        /// What happens on the daemon from now on, as a stream of Docker's events (`Type = "container"`,
        /// `Action = "start"`, `Actor = { ID, Attributes }`, `time`, ...). Without `until` the stream stays open
        /// until it is closed.
        ///
        /// ```luau
        /// local docker = assert(bollard.connect())
        /// local events = docker:events({ filters = { type = { "container" }, event = { "start", "die" } } })
        /// local event = assert(events:next())
        /// print(event.Action, event.Actor and event.Actor.ID)
        /// events:close()
        /// ```
        pub fn events(&self, lua: &Lua, opts: Events) -> Stream<Foreign<EventMessage>, DockerError> {
            let options = query::EventsOptions {
                since: opts.since.map(|time| time.to_string()),
                until: opts.until.map(|time| time.to_string()),
                filters: opts.filters,
            };
            self.stream(lua, move |docker| docker.events(Some(options)).map(|event| Ok(Foreign(event?))))
        }
    }

    /// Run a command in a container, read what it writes to the end, then ask how it exited.
    async fn exec(docker: Docker, name: String, config: ExecConfig) -> Result<Exec, DockerError> {
        let created = docker.create_exec(&name, config).await?;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        if let StartExecResults::Attached { mut output, .. } = docker.start_exec(&created.id, None).await? {
            while let Some(piece) = output.next().await {
                let (into, data) = match piece? {
                    LogOutput::StdErr { message } => (&mut stderr, message),
                    LogOutput::StdOut { message } | LogOutput::Console { message } => (&mut stdout, message),
                    LogOutput::StdIn { .. } => continue,
                };
                into.extend_from_slice(&data);
                if stdout.len() + stderr.len() > MAX_EXEC_OUTPUT {
                    return Err(DockerError::TooLarge { limit: MAX_EXEC_OUTPUT as u64 });
                }
            }
        }
        let exit_code = docker.inspect_exec(&created.id).await?.exit_code;
        Ok(Exec { exit_code, stdout: RawBytes(stdout), stderr: RawBytes(stderr) })
    }
}
