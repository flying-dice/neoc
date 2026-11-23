//! The Docker Engine API — `vnd:bollard` (backed by the `bollard` crate).
//!
//! ```lua
//! local bollard = require("vnd:bollard")
//!
//! local docker = assert(bollard.connect())
//! for _, c in assert(docker:list_containers({ all = true })) do
//!     print(c.Names[1], c.State)
//! end
//!
//! local made = assert(docker:create_container({
//!     name = "web",
//!     Image = "nginx:1.27",
//!     Labels = { app = "web" },
//!     HostConfig = {
//!         PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } },
//!         RestartPolicy = { Name = "unless-stopped" },
//!     },
//! }))
//! assert(docker:start_container(made.Id))
//!
//! local logs = assert(docker:logs("web", { follow = true, tail = 100 }))
//! for piece in function() return logs:next() end do
//!     print(piece.stream, piece.data)
//! end
//! ```
//!
//! **Docker's own shapes.** What Docker returns comes back as it sent it: plain tables with Docker's field names
//! (`Id`, `Names`, `State`, `Config.Labels`); a JSON `null` is `nil`. What Docker takes as a body (the configuration
//! of `create_container`, `create_volume`, `create_network`) is a table with the same names, passed through. This
//! module's own options are lower-case (`all`, `force`, `filters`), and an option a call does not have is refused.
//! An empty table in a body is left out, because Luau cannot say whether it is a list or a map (`options`).
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, "bollard.<function>: <kind>: <message>")`; the kinds are
//! stable (`error`, `bollard.error_kind`; `bollard.error_message` is the message). `connect` is the kind of a host
//! with no Docker: there is no socket, or nothing answers on it. `inspect_container` is the one call for which "no
//! such container" is not an error: it returns `(nil, nil)`.
//!
//! **Connecting.** `connect()` makes a client and sends nothing: the first call does. With no `host` it uses
//! `DOCKER_HOST`, and without that the local socket: `/var/run/docker.sock`, then the per-user sockets of Docker
//! Desktop (`~/.docker/run/docker.sock`) and of a rootless daemon (`$XDG_RUNTIME_DIR/docker.sock`). A client's
//! first call asks the daemon for its API version first, so that an older daemon is spoken to in a version it has.
//!
//! **What reaching Docker means.** Whoever can use the Docker socket can run a container that mounts the host's
//! root: it is root on the host. Nothing here narrows that, as nothing narrows `std:fs` or `vnd:hyper`: a script
//! that is given this module is trusted with the machine.
//!
//! **Streams.** Logs, events and the progress of a pull are a `Stream` (`stream`): `next()` waits for the next
//! item, `(nil, nil)` is the end. A stream belongs to the script that made it and stops with it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ::bollard::container::LogOutput;
use ::bollard::exec::StartExecResults;
use ::bollard::models::{
    ContainerCreateBody, ExecConfig, NetworkCreateRequest, VolumeCreateRequest,
};
use ::bollard::query_parameters as query;
use ::bollard::{API_DEFAULT_VERSION, Docker};
use futures::StreamExt;
use mlua::{IntoLua, Lua, Table, Value};
use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::OnceCell;

use crate::lua::error_text;
use crate::lua::facade::args::{Checked, Op};
use crate::lua::facade::{Bytes, DeclType, Module};
use crate::lua::lifecycle::Life;
use crate::lua::sandbox::{Fallible, from_err, ok};

mod args;
mod error;
mod options;
mod stream;

#[cfg(test)]
mod tests;

use args::{
    Body, Command, Connect, ContainerName, Events, ExecOptions, Filtered, Force, ImageRef, Kill,
    ListContainers, ListImages, Logs, NetworkName, Pull, PullTarget, RemoveContainer, RemoveImage,
    Stop, VolumeName,
};
use error::{BollardError, Kind};
use stream::{Item, Stream};

/// How long a call waits for the daemon's answer when `connect` is given no `timeout`: bollard's own default.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// The most output `exec` reads into memory (both streams together): `vnd:hyper`'s default body cap.
const MAX_EXEC_OUTPUT: usize = 64 * 1024 * 1024;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:bollard` surface through the facade. `connect`, `error_kind` and `error_message` are plain
/// functions; the `Client` and `Stream` types keep their own `impl UserData` and are recorded with
/// [`crate::lua::facade::Module::userdata`].
#[allow(clippy::too_many_lines)] // one record per method, nothing else
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:bollard")?;

    m.declared(connect::lua(), lua.create_function(connect)?)?;

    /// The stable kind inside an error string returned by a `vnd:bollard` function, or `nil` when the string is not one of theirs (or is `nil`, so `bollard.error_kind(err)` takes the `err` of any call). Such a string is `bollard.<function>: <kind>: <message>`. Kinds: `invalid_input` (an argument or option was refused, by this module or by Docker), `connect` (no Docker daemon could be reached: no socket, or nothing answers), `timed_out` (the daemon did not answer within the client's `timeout`), `not_found` (no such container, image, volume or network), `conflict` (the object's state does not allow it: a name in use, a running container, a volume in use), `api` (any other error status from Docker), `bad_response` (Docker's answer could not be read), `too_large` (a result longer than this module reads into memory), `other`. The message is Docker's own where Docker answered; it is for a log and may change.
    #[lua(takes("err: string?"))]
    m.func("error_kind", error_text::reader(error::kind_of))?;
    /// The message inside an error string returned by a `vnd:bollard` function: everything after the kind in `bollard.<function>: <kind>: <message>`, or `nil` when the string is not one of theirs (or is `nil`, so `bollard.error_message(err)` takes the `err` of any call), the same rule as `error_kind`. It is Docker's own sentence where Docker answered (`No such container: web`). It is for showing or logging as it is: branch on `error_kind`, not on these words, which differ between platforms and versions. Read it here instead of taking the error string apart.
    #[lua(takes("err: string?"))]
    m.func("error_message", error_text::reader(error::message_of))?;

    m.userdata("Client", |c| {
        Client::lua(c);
    });

    m.userdata("Stream", |s| {
        Stream::lua(s);
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

/// A client for one daemon. Cheap to clone: every clone is the same client.
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    docker: Docker,
    /// Set once the client and the daemon agreed on an API version (see [`Client::api`]).
    negotiated: OnceCell<()>,
}

impl Client {
    /// The bollard client, after its first use asked the daemon which API version it speaks, so that the two use
    /// the newest both have: a daemon refuses a request that names a version newer than its own. (bollard 0.20
    /// happens to send no version at all, which a daemon answers in its own; the question is asked so that this
    /// module does not depend on that.) A failed attempt (no daemon yet) is made again by the next call.
    async fn api(&self) -> Result<Docker, BollardError> {
        self.inner
            .negotiated
            .get_or_try_init(|| async {
                // a clone shares the version the negotiation sets
                self.inner.docker.clone().negotiate_version().await?;
                Ok::<(), BollardError>(())
            })
            .await?;
        Ok(self.inner.docker.clone())
    }
}

/// A client for a Docker daemon. Nothing is sent until the client's first call. `host` is `unix:///path/docker.sock`, `tcp://host:2375`, `http://host:2375` or `https://host:2376`; without it `DOCKER_HOST` is used, and without that the local socket (`/var/run/docker.sock`, then Docker Desktop's and a rootless daemon's per-user socket). `timeout` is how long a call waits for the daemon's answer, in seconds (default 120). `tls_ca`, `tls_cert` and `tls_key` (all three, paths of PEM files) connect over TLS with a client certificate. Reaching Docker is root on its host: a script given this module is trusted with the machine. Errors are `bollard.<function>: <kind>: <message>` (see `error_kind`); a host with no Docker socket is `connect`.
#[neoc_macros::lua_fn]
fn connect(_: &Lua, opts: Checked<Option<Connect>>) -> mlua::Result<Fallible<Client>> {
    let opened = opts
        .take::<BollardError>()
        .and_then(|opts| open(opts.unwrap_or_default()));
    match opened {
        Ok(docker) => ok(Client {
            inner: Arc::new(ClientInner {
                docker,
                negotiated: OnceCell::new(),
            }),
        }),
        Err(e) => from_err("bollard.connect", e),
    }
}

fn open(opts: Connect) -> Result<Docker, BollardError> {
    let timeout = opts.timeout.map_or(Ok(DEFAULT_TIMEOUT), |secs| {
        Duration::try_from_secs_f64(secs.0)
            .map_err(|_| BollardError::invalid_input("option \"timeout\" is too long"))
    })?;
    let host = match opts.host {
        Some(host) => host,
        None => default_host()?,
    };
    // a bare path is the socket at that path
    let host = if host.starts_with('/') {
        format!("unix://{host}")
    } else {
        host
    };
    let tls = [opts.tls_ca, opts.tls_cert, opts.tls_key];
    let docker = match tls {
        [None, None, None] => Docker::connect_with_host(&host)?,
        [Some(ca), Some(cert), Some(key)] => Docker::connect_with_ssl(
            &host,
            Path::new(&key),
            Path::new(&cert),
            Path::new(&ca),
            DEFAULT_TIMEOUT.as_secs(),
            API_DEFAULT_VERSION,
        )?,
        _ => {
            return Err(BollardError::invalid_input(
                "TLS needs all three of `tls_ca`, `tls_cert` and `tls_key`",
            ));
        }
    };
    Ok(docker.with_timeout(timeout))
}

/// `DOCKER_HOST`, or the first local socket that exists.
fn default_host() -> Result<String, BollardError> {
    if let Some(host) = std::env::var("DOCKER_HOST")
        .ok()
        .filter(|host| !host.is_empty())
    {
        return Ok(host);
    }
    let sockets = local_sockets();
    match sockets.iter().find(|socket| socket.exists()) {
        Some(socket) => Ok(format!("unix://{}", socket.display())),
        None => {
            let tried: Vec<String> = sockets
                .iter()
                .map(|socket| socket.display().to_string())
                .collect();
            Err(BollardError::new(
                Kind::Connect,
                format!(
                    "there is no Docker socket ({}): is Docker installed and running? Set DOCKER_HOST, or pass `host`, for a daemon elsewhere",
                    tried.join(", ")
                ),
            ))
        }
    }
}

/// Where a local daemon listens, most common first: the system socket, Docker Desktop's per-user socket (macOS and
/// Linux, when its system-wide link is off) and a rootless daemon's.
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

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

/// Docker's answer, which a script gets as a table.
struct Answer(Json);

impl IntoLua for Answer {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        stream::to_lua(lua, &self.0)
    }
}

impl DeclType for Answer {
    fn decl_type() -> String {
        Value::decl_type()
    }
}

/// What a command run by `exec` left.
#[neoc_macros::lua_record(into)]
struct Exec {
    exit_code: Option<i64>,
    stdout: Bytes,
    stderr: Bytes,
}

/// Docker's typed answer as the JSON Docker sent.
fn table<T: Serialize>(answer: T) -> Result<Answer, BollardError> {
    serde_json::to_value(answer)
        .map(Answer)
        .map_err(|e| BollardError::new(Kind::BadResponse, e.to_string()))
}

#[neoc_macros::lua_methods("vnd:bollard", "Client")]
impl Client {
    /// Ask the daemon whether it is there: `true`, or the error (`connect` when no daemon answers).
    async fn ping(&self) -> Result<bool, BollardError> {
        self.api().await?.ping().await?;
        Ok(true)
    }

    /// The daemon's version: Docker's `/version` table (`Version`, `ApiVersion`, `Os`, `Arch`, ...).
    async fn version(&self) -> Result<Answer, BollardError> {
        table(self.api().await?.version().await?)
    }

    /// The daemon's system information: Docker's `/info` table (`Containers`, `Images`, `ServerVersion`, `OperatingSystem`, ...).
    async fn info(&self) -> Result<Answer, BollardError> {
        table(self.api().await?.info().await?)
    }

    /// The containers, as Docker's list of summaries (`Id`, `Names`, `Image`, `State`, `Status`, `Labels`, `Ports`, ...). Only running ones unless `all`; `limit` is the most recently created ones; `size` adds `SizeRw` and `SizeRootFs`; `filters` are Docker's (`{ label = { "com.docker.compose.project=blog" }, status = "running" }`).
    async fn list_containers(&self, opts: Option<ListContainers>) -> Result<Answer, BollardError> {
        let opts = opts.unwrap_or_default();
        let options = query::ListContainersOptions {
            all: opts.all.unwrap_or(false),
            limit: opts.limit,
            size: opts.size.unwrap_or(false),
            filters: opts.filters.map(|filters| filters.0),
        };
        table(self.api().await?.list_containers(Some(options)).await?)
    }

    /// Docker's inspect table of a container, by name or id (`Id`, `State`, `Config.Labels`, `HostConfig`, `NetworkSettings`, ...), or `(nil, nil)` when there is no such container.
    async fn inspect_container(&self, name: ContainerName) -> Result<Option<Answer>, BollardError> {
        let inspected = self
            .api()
            .await?
            .inspect_container(&name.0, None::<query::InspectContainerOptions>)
            .await
            .map_err(BollardError::from);
        match inspected {
            Ok(container) => table(container).map(Some),
            Err(e) if e.kind() == Kind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Create a container (not started) from Docker's create body, with Docker's field names: `{ name = "web", Image = "nginx", Env = { "A=1" }, Labels = { ... }, HostConfig = { Binds = { ... }, PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } }, RestartPolicy = { Name = "unless-stopped" } } }`. The lower-case `name` (and `platform`) are the call's own. Returns `{ Id, Warnings }`. The image must be on the host already (`pull_image`): a missing one is `not_found`; a name in use is `conflict`.
    async fn create_container(&self, config: Body) -> Result<Answer, BollardError> {
        let docker = self.api().await?;
        let Json::Object(mut config) = config.0 else {
            return Err(BollardError::invalid_input(
                "the configuration must be a table with Docker's field names",
            ));
        };
        // the two lower-case keys are the call's own; everything else is Docker's body
        let name = match config.remove("name") {
            None => None,
            Some(Json::String(name)) => Some(options::name(&name, "the container's name")?),
            Some(_) => {
                return Err(BollardError::invalid_input("`name` must be a string"));
            }
        };
        let platform = match config.remove("platform") {
            None => String::new(),
            Some(Json::String(platform)) => platform,
            Some(_) => {
                return Err(BollardError::invalid_input("`platform` must be a string"));
            }
        };
        let body: ContainerCreateBody = options::body(Json::Object(config), "the configuration")?;
        let options = query::CreateContainerOptions { name, platform };
        table(docker.create_container(Some(options), body).await?)
    }

    /// Start a container. Starting a running one succeeds.
    async fn start_container(&self, name: ContainerName) -> Result<bool, BollardError> {
        self.api()
            .await?
            .start_container(&name.0, None::<query::StartContainerOptions>)
            .await?;
        Ok(true)
    }

    /// Stop a container: Docker sends `signal` (default the image's stop signal, usually `SIGTERM`), waits `timeout` seconds (default 10) and then kills it. Stopping a stopped one succeeds.
    async fn stop_container(
        &self,
        name: ContainerName,
        opts: Option<Stop>,
    ) -> Result<bool, BollardError> {
        let Stop { timeout, signal } = opts.unwrap_or_default();
        let options = query::StopContainerOptions {
            signal: signal.map(|signal| signal.0),
            t: timeout,
        };
        self.api()
            .await?
            .stop_container(&name.0, Some(options))
            .await?;
        Ok(true)
    }

    /// Stop a container as `stop_container` does and start it again.
    async fn restart_container(
        &self,
        name: ContainerName,
        opts: Option<Stop>,
    ) -> Result<bool, BollardError> {
        let Stop { timeout, signal } = opts.unwrap_or_default();
        let options = query::RestartContainerOptions {
            signal: signal.map(|signal| signal.0),
            t: timeout,
        };
        self.api()
            .await?
            .restart_container(&name.0, Some(options))
            .await?;
        Ok(true)
    }

    /// Send a signal to a running container (default `SIGKILL`; `SIGHUP` to make a server reload). A container that is not running is `conflict`.
    async fn kill_container(
        &self,
        name: ContainerName,
        opts: Option<Kill>,
    ) -> Result<bool, BollardError> {
        let Kill { signal } = opts.unwrap_or_default();
        let options = query::KillContainerOptions {
            signal: signal.map_or_else(|| "SIGKILL".to_owned(), |signal| signal.0),
        };
        self.api()
            .await?
            .kill_container(&name.0, Some(options))
            .await?;
        Ok(true)
    }

    /// Suspend every process of a running container.
    async fn pause_container(&self, name: ContainerName) -> Result<bool, BollardError> {
        self.api().await?.pause_container(&name.0).await?;
        Ok(true)
    }

    /// Resume a paused container.
    async fn unpause_container(&self, name: ContainerName) -> Result<bool, BollardError> {
        self.api().await?.unpause_container(&name.0).await?;
        Ok(true)
    }

    /// Remove a container. A running one is `conflict` unless `force` (which kills it first); `volumes` removes its anonymous volumes with it.
    async fn remove_container(
        &self,
        name: ContainerName,
        opts: Option<RemoveContainer>,
    ) -> Result<bool, BollardError> {
        let RemoveContainer { force, volumes } = opts.unwrap_or_default();
        let options = query::RemoveContainerOptions {
            force: force.unwrap_or(false),
            v: volumes.unwrap_or(false),
            link: false,
        };
        self.api()
            .await?
            .remove_container(&name.0, Some(options))
            .await?;
        Ok(true)
    }

    /// A container's output as a `Stream` of `{ stream = "stdout" | "stderr", data = string }` pieces (a container with a TTY sends everything as `"console"`). `tail` is how many lines from the end to start with (default all), `since` and `until` are Unix times, `timestamps` prefixes each line, `stdout = false` or `stderr = false` leaves one out. The stream ends after what there is, or with `follow` stays open for new output until the container stops. A container that does not exist is the error of the first `next()`.
    fn logs(
        &self,
        lua: &Lua,
        op: Op,
        name: ContainerName,
        opts: Option<Logs>,
    ) -> Result<Stream, BollardError> {
        let options = logs_options(opts.unwrap_or_default())?;
        self.stream(lua, op, move |docker| {
            docker.logs(&name.0, Some(options)).map(output_item)
        })
    }

    /// One reading of a container's resource use: Docker's stats table (`cpu_stats`, `memory_stats`, `networks`, `blkio_stats`, ...).
    async fn stats(&self, name: ContainerName) -> Result<Answer, BollardError> {
        let options = query::StatsOptions {
            stream: false,
            one_shot: true,
        };
        let reading = self.api().await?.stats(&name.0, Some(options)).next().await;
        match reading {
            Some(reading) => table(reading?),
            None => Err(BollardError::new(Kind::BadResponse, "Docker sent no stats")),
        }
    }

    /// Run a command in a running container and wait for it: `docker:exec("web", { "nginx", "-t" })`. Returns its exit code and what it wrote (at most 64 MiB together, `too_large` past that); a command that fails is a result with a non-zero `exit_code`, not an error. `env` is a list of `NAME=value`.
    async fn exec(
        &self,
        name: ContainerName,
        cmd: Command,
        opts: Option<ExecOptions>,
    ) -> Result<Exec, BollardError> {
        let ExecOptions {
            env,
            user,
            working_dir,
            privileged,
        } = opts.unwrap_or_default();
        let config = ExecConfig {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(cmd.0),
            env: env.map(|env| env.0),
            user,
            working_dir,
            privileged,
            ..Default::default()
        };
        exec(self.api().await?, name.0, config).await
    }

    /// The images on the host, as Docker's list of summaries (`Id`, `RepoTags`, `Size`, `Created`, `Containers`, ...). `all` includes intermediate layers.
    async fn list_images(&self, opts: Option<ListImages>) -> Result<Answer, BollardError> {
        let opts = opts.unwrap_or_default();
        let options = query::ListImagesOptions {
            all: opts.all.unwrap_or(false),
            digests: opts.digests.unwrap_or(false),
            filters: opts.filters.map(|filters| filters.0),
            ..Default::default()
        };
        table(self.api().await?.list_images(Some(options)).await?)
    }

    /// Pull an image from its registry: a `Stream` of Docker's progress tables (`status`, `id`, `progressDetail = { current, total }`), which ends when the pull is over. `nginx` pulls `nginx:latest`, never every tag. The pull runs as the stream is read and stops if the stream is closed; a pull that fails (no such image, no network) is the error of a `next()`.
    fn pull_image(
        &self,
        lua: &Lua,
        op: Op,
        image: PullTarget,
        opts: Option<Pull>,
    ) -> Result<Stream, BollardError> {
        let options = query::CreateImageOptions {
            from_image: Some(image.repository),
            tag: image.tag,
            platform: opts.unwrap_or_default().platform.unwrap_or_default(),
            ..Default::default()
        };
        // a pull that fails does so inside a 200 response, as a progress line: bollard makes it an error
        self.stream(lua, op, move |docker| {
            docker
                .create_image(Some(options), None, None)
                .map(|progress| json_item(progress?))
        })
    }

    /// Remove an image, by reference or id: Docker's list of what was untagged and deleted. An image a container uses is `conflict` unless `force`; `noprune` keeps its untagged parents.
    async fn remove_image(
        &self,
        image: ImageRef,
        opts: Option<RemoveImage>,
    ) -> Result<Answer, BollardError> {
        let RemoveImage { force, noprune } = opts.unwrap_or_default();
        let options = query::RemoveImageOptions {
            force: force.unwrap_or(false),
            noprune: noprune.unwrap_or(false),
            ..Default::default()
        };
        table(
            self.api()
                .await?
                .remove_image(&image.0, Some(options), None)
                .await?,
        )
    }

    /// The volumes: Docker's `{ Volumes = { ... }, Warnings }` (each volume `Name`, `Driver`, `Mountpoint`, `Labels`, ...).
    async fn list_volumes(&self, opts: Option<Filtered>) -> Result<Answer, BollardError> {
        let options = query::ListVolumesOptions {
            filters: opts.unwrap_or_default().filters.map(|filters| filters.0),
        };
        table(self.api().await?.list_volumes(Some(options)).await?)
    }

    /// Create a volume from Docker's create body (`{ Name = "data", Labels = { ... } }`; without a `Name` Docker makes one up). Returns the volume. Creating one that exists returns it.
    async fn create_volume(&self, config: Body) -> Result<Answer, BollardError> {
        let docker = self.api().await?;
        let body: VolumeCreateRequest = options::body(config.0, "the configuration")?;
        table(docker.create_volume(body).await?)
    }

    /// Remove a volume. One a container uses is `conflict`.
    async fn remove_volume(
        &self,
        name: VolumeName,
        opts: Option<Force>,
    ) -> Result<bool, BollardError> {
        let options = query::RemoveVolumeOptions {
            force: opts.unwrap_or_default().force.unwrap_or(false),
        };
        self.api()
            .await?
            .remove_volume(&name.0, Some(options))
            .await?;
        Ok(true)
    }

    /// The networks, as Docker's list (`Id`, `Name`, `Driver`, `Scope`, `Labels`, ...).
    async fn list_networks(&self, opts: Option<Filtered>) -> Result<Answer, BollardError> {
        let options = query::ListNetworksOptions {
            filters: opts.unwrap_or_default().filters.map(|filters| filters.0),
        };
        table(self.api().await?.list_networks(Some(options)).await?)
    }

    /// Create a network from Docker's create body (`{ Name = "backend", Driver = "bridge", Labels = { ... } }`). Returns `{ Id, Warning }`. A name in use is `conflict`.
    async fn create_network(&self, config: Body) -> Result<Answer, BollardError> {
        let docker = self.api().await?;
        let body: NetworkCreateRequest = options::body(config.0, "the configuration")?;
        table(docker.create_network(body).await?)
    }

    /// Remove a network, by name or id. One with containers attached is an error (`api`: Docker answers 403).
    async fn remove_network(&self, name: NetworkName) -> Result<bool, BollardError> {
        self.api().await?.remove_network(&name.0).await?;
        Ok(true)
    }

    /// What happens on the daemon from now on, as a `Stream` of Docker's event tables (`Type = "container"`, `Action = "start"`, `Actor = { ID, Attributes }`, `time`, ...). `since` (a Unix time) replays earlier events first, `until` ends the stream at that time, `filters` are Docker's (`{ type = "container", event = { "start", "die" } }`). Without `until` the stream stays open until it is closed.
    fn events(&self, lua: &Lua, op: Op, opts: Option<Events>) -> Result<Stream, BollardError> {
        let Events {
            since,
            until,
            filters,
        } = opts.unwrap_or_default();
        let options = query::EventsOptions {
            since: since.map(|time| time.to_string()),
            until: until.map(|time| time.to_string()),
            filters: filters.map(|filters| filters.0),
        };
        self.stream(lua, op, move |docker| {
            docker.events(Some(options)).map(|event| json_item(event?))
        })
    }
}

impl Client {
    /// A [`Stream`] of what `source` yields. The method that returns it is synchronous: the stream's task makes
    /// the request, after the client settled its API version.
    fn stream<S>(
        &self,
        lua: &Lua,
        op: Op,
        source: impl FnOnce(&Docker) -> S + Send + 'static,
    ) -> Result<Stream, BollardError>
    where
        S: futures::Stream<Item = Result<Item, BollardError>> + Send + 'static,
    {
        let client = self.clone();
        Stream::start(&Life::of(lua), op.0, async move {
            let docker = client.api().await?;
            Ok(source(&docker))
        })
    }
}

fn logs_options(opts: Logs) -> Result<query::LogsOptions, BollardError> {
    Ok(query::LogsOptions {
        follow: opts.follow.unwrap_or(false),
        stdout: opts.stdout.unwrap_or(true),
        stderr: opts.stderr.unwrap_or(true),
        since: opts.since.unwrap_or(0),
        until: opts.until.unwrap_or(0),
        timestamps: opts.timestamps.unwrap_or(false),
        tail: match opts.tail {
            Some(lines) if lines < 0 => {
                return Err(BollardError::invalid_input(
                    "option \"tail\" must be a number of lines, zero or more",
                ));
            }
            Some(lines) => lines.to_string(),
            None => "all".to_owned(),
        },
    })
}

fn output_item(piece: Result<LogOutput, ::bollard::errors::Error>) -> Result<Item, BollardError> {
    let (stream, data) = match piece? {
        LogOutput::StdOut { message } => ("stdout", message),
        LogOutput::StdErr { message } => ("stderr", message),
        LogOutput::StdIn { message } => ("stdin", message),
        LogOutput::Console { message } => ("console", message),
    };
    Ok(Item::Output { stream, data })
}

fn json_item<T: Serialize>(message: T) -> Result<Item, BollardError> {
    serde_json::to_value(message)
        .map(Item::Json)
        .map_err(|e| BollardError::new(Kind::BadResponse, e.to_string()))
}

/// Run a command in a container, read what it writes to the end, then ask how it exited.
async fn exec(docker: Docker, name: String, config: ExecConfig) -> Result<Exec, BollardError> {
    let created = docker.create_exec(&name, config).await?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    if let StartExecResults::Attached { mut output, .. } =
        docker.start_exec(&created.id, None).await?
    {
        while let Some(piece) = output.next().await {
            let (into, data) = match piece? {
                LogOutput::StdErr { message } => (&mut stderr, message),
                LogOutput::StdOut { message } | LogOutput::Console { message } => {
                    (&mut stdout, message)
                }
                LogOutput::StdIn { .. } => continue,
            };
            into.extend_from_slice(&data);
            if stdout.len() + stderr.len() > MAX_EXEC_OUTPUT {
                return Err(BollardError::new(
                    Kind::TooLarge,
                    format!("the command wrote more than {MAX_EXEC_OUTPUT} bytes"),
                ));
            }
        }
    }
    let exit_code = docker.inspect_exec(&created.id).await?.exit_code;
    Ok(Exec {
        exit_code,
        stdout: Bytes(stdout),
        stderr: Bytes(stderr),
    })
}
