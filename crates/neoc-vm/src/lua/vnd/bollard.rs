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

use std::future::Future;
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
use mlua::{IntoLua, Lua, Table, UserData, UserDataMethods, Value};
use serde::Serialize;
use serde_json::Value as Json;
use tokio::sync::OnceCell;
use tracing::Instrument;

use crate::lua::error_text;
use crate::lua::facade::{Bytes, DeclType, Module, Shape, decl, typed};
use crate::lua::lifecycle::{Life, Role, async_method};
use crate::lua::sandbox::{Fallible, from_err, ok};

mod error;
mod options;
mod stream;

#[cfg(test)]
mod tests;

use error::{BollardError, Kind};
use options::Opts;
use stream::{Item, Stream};

/// How long a call waits for the daemon's answer when `connect` is given no `timeout`: bollard's own default.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// The most output `exec` reads into memory (both streams together): `vnd:hyper`'s default body cap.
const MAX_EXEC_OUTPUT: usize = 64 * 1024 * 1024;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// The option tables of the calls: each is read by [`read_opts`], which refuses what is not a key of it.
mod shapes {
    use mlua::Value;

    use crate::lua::facade::{List, Map};

    /// Docker's filters: a list of values, or one, under each name.
    type Filters = Option<Map<String, Value>>;

    #[neoc_macros::lua_record(shape)]
    pub(super) struct ListContainers {
        all: Option<bool>,
        limit: Option<i64>,
        size: Option<bool>,
        filters: Filters,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Stop {
        timeout: Option<i64>,
        signal: Option<String>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Kill {
        signal: Option<String>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct RemoveContainer {
        force: Option<bool>,
        volumes: Option<bool>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Logs {
        follow: Option<bool>,
        stdout: Option<bool>,
        stderr: Option<bool>,
        tail: Option<i64>,
        since: Option<i64>,
        until: Option<i64>,
        timestamps: Option<bool>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Exec {
        env: Option<List<String>>,
        user: Option<String>,
        working_dir: Option<String>,
        privileged: Option<bool>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct ListImages {
        all: Option<bool>,
        digests: Option<bool>,
        filters: Filters,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Pull {
        platform: Option<String>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct RemoveImage {
        force: Option<bool>,
        noprune: Option<bool>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Filtered {
        filters: Filters,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Force {
        force: Option<bool>,
    }

    #[neoc_macros::lua_record(shape)]
    pub(super) struct Events {
        since: Option<i64>,
        until: Option<i64>,
        filters: Filters,
    }
}
const BODY: &str = "{ [string]: any }";

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
#[neoc_macros::lua_fn(types(
    opts = "{ host: string?, timeout: number?, tls_ca: string?, tls_cert: string?, tls_key: string? }?"
))]
fn connect(_: &Lua, opts: Option<Value>) -> mlua::Result<Fallible<Client>> {
    let op = "bollard.connect";
    let opts = match opts {
        Some(opts) => match options::json(&opts, op)? {
            Ok(json) => Some(json),
            Err(e) => return from_err(op, e),
        },
        None => None,
    };
    match open(opts) {
        Ok(docker) => ok(Client {
            inner: Arc::new(ClientInner {
                docker,
                negotiated: OnceCell::new(),
            }),
        }),
        Err(e) => from_err(op, e),
    }
}

fn open(opts: Option<Json>) -> Result<Docker, BollardError> {
    let opts = Opts::read(opts, &["host", "timeout", "tls_ca", "tls_cert", "tls_key"])?;
    let timeout = opts
        .seconds("timeout")?
        .map_or(Ok(DEFAULT_TIMEOUT), |secs| {
            Duration::try_from_secs_f64(secs)
                .map_err(|_| BollardError::invalid_input("option `timeout` is too long"))
        })?;
    let host = match opts.string("host")? {
        Some(host) => host,
        None => default_host()?,
    };
    // a bare path is the socket at that path
    let host = if host.starts_with('/') {
        format!("unix://{host}")
    } else {
        host
    };
    let tls = [
        opts.string("tls_ca")?,
        opts.string("tls_cert")?,
        opts.string("tls_key")?,
    ];
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

/// What a finished call hands the script: its value, or `nil` and why not.
fn reply<R>(op: &str, result: Result<R, BollardError>) -> mlua::Result<Fallible<R>> {
    match result {
        Ok(value) => ok(value),
        Err(e) => from_err(op, e),
    }
}

/// The arguments of a call as plain data: an `Err` outside is raised (a buffer was passed), an `Err` inside is the
/// call's `(nil, err)`.
type Prepared<P> = mlua::Result<Result<P, BollardError>>;

/// Register the method `op` (`bollard.<method>`). `prepare` reads the arguments in the synchronous part of the
/// call (a script table can run script code when it is read, see `vnd:hyper`'s client); `run` is the request, made
/// after the client settled its API version, and what it gives is what the method is declared to return.
fn call<M, A, P, Fut, R>(
    methods: &mut M,
    op: &'static str,
    prepare: impl Fn(&'static str, A) -> Prepared<P> + Send + 'static,
    run: impl Fn(Docker, P) -> Fut + Send + Sync + 'static,
) where
    M: UserDataMethods<Client>,
    A: mlua::FromLuaMulti,
    P: Send + 'static,
    Fut: Future<Output = Result<R, BollardError>> + Send + 'static,
    R: IntoLua + DeclType + Send + 'static,
{
    typed::answers::<Fallible<R>>();
    let run = Arc::new(run);
    async_method(
        methods,
        method_name(op),
        Role::Wait,
        move |_, this, args: A| {
            let client = this.clone();
            let prepared = prepare(op, args);
            let run = Arc::clone(&run);
            async move {
                let result = match prepared? {
                    Ok(prepared) => match client.api().await {
                        Ok(docker) => run(docker, prepared).await,
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                };
                reply(op, result)
            }
            .instrument(tracing::info_span!(target: "lua::lib::bollard", "bollard.call", op))
        },
    );
}

/// Register the method `op` that returns a [`Stream`]. The method itself is synchronous: `source` builds the
/// stream's items from the client, and the stream's task makes the request.
fn stream_call<M, A, P, S>(
    methods: &mut M,
    op: &'static str,
    prepare: impl Fn(&'static str, A) -> Prepared<P> + Send + 'static,
    source: impl Fn(&Docker, P) -> S + Send + Sync + 'static,
) where
    M: UserDataMethods<Client>,
    A: mlua::FromLuaMulti,
    P: Send + 'static,
    S: futures::Stream<Item = Result<Item, BollardError>> + Send + 'static,
{
    typed::answers::<Fallible<Stream>>();
    let source = Arc::new(source);
    methods.add_method(method_name(op), move |lua, this, args: A| {
        let prepared = match prepare(op, args)? {
            Ok(prepared) => prepared,
            Err(e) => return from_err(op, e),
        };
        let client = this.clone();
        let source = Arc::clone(&source);
        let started = Stream::start(&Life::of(lua), op, async move {
            let docker = client.api().await?;
            Ok(source(&docker, prepared))
        });
        match started {
            Ok(stream) => ok(stream),
            Err(e) => from_err(op, e),
        }
    });
}

fn method_name(op: &'static str) -> &'static str {
    op.strip_prefix("bollard.").unwrap_or(op)
}

/// A call with no arguments.
fn nothing(_: &'static str, (): ()) -> Prepared<()> {
    Ok(Ok(()))
}

/// A call on one named container, volume or network.
fn named(what: &'static str) -> impl Fn(&'static str, String) -> Prepared<String> {
    move |_, name| Ok(options::name(&name, what))
}

/// A call on one named thing, with the options in `allowed`.
fn named_with(
    what: &'static str,
    allowed: &'static [&'static str],
) -> impl Fn(&'static str, (String, Option<Value>)) -> Prepared<(String, Opts)> {
    move |op, (name, opts)| {
        let opts = match read_opts(op, opts, allowed)? {
            Ok(opts) => opts,
            Err(e) => return Ok(Err(e)),
        };
        Ok(options::name(&name, what).map(|name| (name, opts)))
    }
}

/// A call that takes only the options in `allowed`.
fn only_opts(
    allowed: &'static [&'static str],
) -> impl Fn(&'static str, Option<Value>) -> Prepared<Opts> {
    move |op, opts| read_opts(op, opts, allowed)
}

fn read_opts(op: &str, opts: Option<Value>, allowed: &[&str]) -> Prepared<Opts> {
    let opts = match opts {
        Some(opts) => match options::json(&opts, op)? {
            Ok(json) => Some(json),
            Err(e) => return Ok(Err(e)),
        },
        None => None,
    };
    Ok(Opts::read(opts, allowed))
}

/// A call that takes one of Docker's bodies.
fn body_of(op: &'static str, config: Value) -> Prepared<Json> {
    options::json(&config, op)
}

const CONTAINER: &str = "the container";
const VOLUME: &str = "the volume";
const NETWORK: &str = "the network";

/// A signal as Docker takes it: a name (`SIGHUP`, `HUP`) or a number.
fn signal(opts: &Opts) -> Result<Option<String>, BollardError> {
    match opts.string("signal")? {
        Some(signal) if signal.is_empty() || !signal.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            Err(BollardError::invalid_input(
                "option `signal` must be a signal's name or number (`SIGTERM`, `HUP`, `9`)",
            ))
        }
        signal => Ok(signal),
    }
}

#[neoc_macros::lua_userdata("vnd:bollard", "Client")]
impl UserData for Client {
    #[allow(clippy::too_many_lines)] // one registration per method
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Ask the daemon whether it is there: `true`, or the error (`connect` when no daemon answers).
        #[lua(name = "ping", takes())]
        call(methods, "bollard.ping", nothing, |docker, ()| async move {
            docker.ping().await?;
            Ok(true)
        });
        /// The daemon's version: Docker's `/version` table (`Version`, `ApiVersion`, `Os`, `Arch`, ...).
        #[lua(name = "version", takes())]
        call(
            methods,
            "bollard.version",
            nothing,
            |docker, ()| async move { table(docker.version().await?) },
        );
        /// The daemon's system information: Docker's `/info` table (`Containers`, `Images`, `ServerVersion`, `OperatingSystem`, ...).
        #[lua(name = "info", takes())]
        call(methods, "bollard.info", nothing, |docker, ()| async move {
            table(docker.info().await?)
        });

        /// The containers, as Docker's list of summaries (`Id`, `Names`, `Image`, `State`, `Status`, `Labels`, `Ports`, ...). Only running ones unless `all`; `limit` is the most recently created ones; `size` adds `SizeRw` and `SizeRootFs`; `filters` are Docker's (`{ label = { "com.docker.compose.project=blog" }, status = "running" }`).
        #[lua(name = "list_containers", takes(opts = decl::<Option<shapes::ListContainers>>()))]
        call(
            methods,
            "bollard.list_containers",
            only_opts(shapes::ListContainers::KEYS),
            |docker, opts| async move {
                let options = query::ListContainersOptions {
                    all: opts.bool("all")?.unwrap_or(false),
                    limit: opts.int("limit")?,
                    size: opts.bool("size")?.unwrap_or(false),
                    filters: opts.filters()?,
                };
                table(docker.list_containers(Some(options)).await?)
            },
        );
        /// Docker's inspect table of a container, by name or id (`Id`, `State`, `Config.Labels`, `HostConfig`, `NetworkSettings`, ...), or `(nil, nil)` when there is no such container.
        #[lua(name = "inspect_container", takes("name: string"))]
        call(
            methods,
            "bollard.inspect_container",
            named(CONTAINER),
            |docker, name| async move {
                let inspected = docker
                    .inspect_container(&name, None::<query::InspectContainerOptions>)
                    .await
                    .map_err(BollardError::from);
                match inspected {
                    Ok(container) => table(container).map(Some),
                    Err(e) if e.kind() == Kind::NotFound => Ok(None),
                    Err(e) => Err(e),
                }
            },
        );
        /// Create a container (not started) from Docker's create body, with Docker's field names: `{ name = "web", Image = "nginx", Env = { "A=1" }, Labels = { ... }, HostConfig = { Binds = { ... }, PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } }, RestartPolicy = { Name = "unless-stopped" } } }`. The lower-case `name` (and `platform`) are the call's own. Returns `{ Id, Warnings }`. The image must be on the host already (`pull_image`): a missing one is `not_found`; a name in use is `conflict`.
        #[lua(name = "create_container", takes(config = BODY))]
        call(
            methods,
            "bollard.create_container",
            body_of,
            |docker, config| async move {
                let Json::Object(mut config) = config else {
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
                let body: ContainerCreateBody =
                    options::body(Json::Object(config), "the configuration")?;
                let options = query::CreateContainerOptions { name, platform };
                table(docker.create_container(Some(options), body).await?)
            },
        );
        /// Start a container. Starting a running one succeeds.
        #[lua(name = "start_container", takes("name: string"))]
        call(
            methods,
            "bollard.start_container",
            named(CONTAINER),
            |docker, name| async move {
                docker
                    .start_container(&name, None::<query::StartContainerOptions>)
                    .await?;
                Ok(true)
            },
        );
        /// Stop a container: Docker sends `signal` (default the image's stop signal, usually `SIGTERM`), waits `timeout` seconds (default 10) and then kills it. Stopping a stopped one succeeds.
        #[lua(
            name = "stop_container",
            takes("name: string", opts = decl::<Option<shapes::Stop>>())
        )]
        call(
            methods,
            "bollard.stop_container",
            named_with(CONTAINER, shapes::Stop::KEYS),
            |docker, (name, opts)| async move {
                let options = query::StopContainerOptions {
                    signal: signal(&opts)?,
                    t: opts.int("timeout")?,
                };
                docker.stop_container(&name, Some(options)).await?;
                Ok(true)
            },
        );
        /// Stop a container as `stop_container` does and start it again.
        #[lua(
            name = "restart_container",
            takes("name: string", opts = decl::<Option<shapes::Stop>>())
        )]
        call(
            methods,
            "bollard.restart_container",
            named_with(CONTAINER, shapes::Stop::KEYS),
            |docker, (name, opts)| async move {
                let options = query::RestartContainerOptions {
                    signal: signal(&opts)?,
                    t: opts.int("timeout")?,
                };
                docker.restart_container(&name, Some(options)).await?;
                Ok(true)
            },
        );
        /// Send a signal to a running container (default `SIGKILL`; `SIGHUP` to make a server reload). A container that is not running is `conflict`.
        #[lua(
            name = "kill_container",
            takes("name: string", opts = decl::<Option<shapes::Kill>>())
        )]
        call(
            methods,
            "bollard.kill_container",
            named_with(CONTAINER, shapes::Kill::KEYS),
            |docker, (name, opts)| async move {
                let options = query::KillContainerOptions {
                    signal: signal(&opts)?.unwrap_or_else(|| "SIGKILL".to_owned()),
                };
                docker.kill_container(&name, Some(options)).await?;
                Ok(true)
            },
        );
        /// Suspend every process of a running container.
        #[lua(name = "pause_container", takes("name: string"))]
        call(
            methods,
            "bollard.pause_container",
            named(CONTAINER),
            |docker, name| async move {
                docker.pause_container(&name).await?;
                Ok(true)
            },
        );
        /// Resume a paused container.
        #[lua(name = "unpause_container", takes("name: string"))]
        call(
            methods,
            "bollard.unpause_container",
            named(CONTAINER),
            |docker, name| async move {
                docker.unpause_container(&name).await?;
                Ok(true)
            },
        );
        /// Remove a container. A running one is `conflict` unless `force` (which kills it first); `volumes` removes its anonymous volumes with it.
        #[lua(
            name = "remove_container",
            takes("name: string", opts = decl::<Option<shapes::RemoveContainer>>())
        )]
        call(
            methods,
            "bollard.remove_container",
            named_with(CONTAINER, shapes::RemoveContainer::KEYS),
            |docker, (name, opts)| async move {
                let options = query::RemoveContainerOptions {
                    force: opts.bool("force")?.unwrap_or(false),
                    v: opts.bool("volumes")?.unwrap_or(false),
                    link: false,
                };
                docker.remove_container(&name, Some(options)).await?;
                Ok(true)
            },
        );
        /// A container's output as a `Stream` of `{ stream = "stdout" | "stderr", data = string }` pieces (a container with a TTY sends everything as `"console"`). `tail` is how many lines from the end to start with (default all), `since` and `until` are Unix times, `timestamps` prefixes each line, `stdout = false` or `stderr = false` leaves one out. The stream ends after what there is, or with `follow` stays open for new output until the container stops. A container that does not exist is the error of the first `next()`.
        #[lua(
            name = "logs",
            takes(
                "name: string",
                opts = decl::<Option<shapes::Logs>>()
            )
        )]
        stream_call(
            methods,
            "bollard.logs",
            |op, (name, opts): (String, Option<Value>)| {
                let opts = match read_opts(op, opts, shapes::Logs::KEYS)? {
                    Ok(opts) => opts,
                    Err(e) => return Ok(Err(e)),
                };
                Ok(logs_options(&opts)
                    .and_then(|options| Ok((options::name(&name, CONTAINER)?, options))))
            },
            |docker, (name, options)| docker.logs(&name, Some(options)).map(output_item),
        );
        /// One reading of a container's resource use: Docker's stats table (`cpu_stats`, `memory_stats`, `networks`, `blkio_stats`, ...).
        #[lua(name = "stats", takes("name: string"))]
        call(
            methods,
            "bollard.stats",
            named(CONTAINER),
            |docker, name| async move {
                let options = query::StatsOptions {
                    stream: false,
                    one_shot: true,
                };
                let reading = docker.stats(&name, Some(options)).next().await;
                match reading {
                    Some(reading) => table(reading?),
                    None => Err(BollardError::new(Kind::BadResponse, "Docker sent no stats")),
                }
            },
        );
        /// Run a command in a running container and wait for it: `docker:exec("web", { "nginx", "-t" })`. Returns its exit code and what it wrote (at most 64 MiB together, `too_large` past that); a command that fails is a result with a non-zero `exit_code`, not an error. `env` is a list of `NAME=value`.
        #[lua(
            name = "exec",
            takes(
                "name: string",
                "cmd: { string }",
                opts = decl::<Option<shapes::Exec>>()
            )
        )]
        call(
            methods,
            "bollard.exec",
            |op, (name, cmd, opts): (String, Value, Option<Value>)| {
                let opts = match read_opts(op, opts, shapes::Exec::KEYS)? {
                    Ok(opts) => opts,
                    Err(e) => return Ok(Err(e)),
                };
                let cmd = match options::json(&cmd, op)? {
                    Ok(cmd) => cmd,
                    Err(e) => return Ok(Err(e)),
                };
                Ok(exec_config(cmd, &opts)
                    .and_then(|config| Ok((options::name(&name, CONTAINER)?, config))))
            },
            |docker, (name, config)| exec(docker, name, config),
        );

        /// The images on the host, as Docker's list of summaries (`Id`, `RepoTags`, `Size`, `Created`, `Containers`, ...). `all` includes intermediate layers.
        #[lua(name = "list_images", takes(opts = decl::<Option<shapes::ListImages>>()))]
        call(
            methods,
            "bollard.list_images",
            only_opts(shapes::ListImages::KEYS),
            |docker, opts| async move {
                let options = query::ListImagesOptions {
                    all: opts.bool("all")?.unwrap_or(false),
                    digests: opts.bool("digests")?.unwrap_or(false),
                    filters: opts.filters()?,
                    ..Default::default()
                };
                table(docker.list_images(Some(options)).await?)
            },
        );
        /// Pull an image from its registry: a `Stream` of Docker's progress tables (`status`, `id`, `progressDetail = { current, total }`), which ends when the pull is over. `nginx` pulls `nginx:latest`, never every tag. The pull runs as the stream is read and stops if the stream is closed; a pull that fails (no such image, no network) is the error of a `next()`.
        #[lua(
            name = "pull_image",
            takes("image: string", opts = decl::<Option<shapes::Pull>>())
        )]
        stream_call(
            methods,
            "bollard.pull_image",
            |op, (image, opts): (String, Option<Value>)| {
                let opts = match read_opts(op, opts, shapes::Pull::KEYS)? {
                    Ok(opts) => opts,
                    Err(e) => return Ok(Err(e)),
                };
                Ok(options::pull_target(&image).and_then(|(from_image, tag)| {
                    Ok(query::CreateImageOptions {
                        from_image: Some(from_image),
                        tag,
                        platform: opts.string("platform")?.unwrap_or_default(),
                        ..Default::default()
                    })
                }))
            },
            |docker, options| {
                // a pull that fails does so inside a 200 response, as a progress line: bollard makes it an error
                docker
                    .create_image(Some(options), None, None)
                    .map(|progress| json_item(progress?))
            },
        );
        /// Remove an image, by reference or id: Docker's list of what was untagged and deleted. An image a container uses is `conflict` unless `force`; `noprune` keeps its untagged parents.
        #[lua(
            name = "remove_image",
            takes("image: string", opts = decl::<Option<shapes::RemoveImage>>())
        )]
        call(
            methods,
            "bollard.remove_image",
            |op, (image, opts): (String, Option<Value>)| {
                let opts = match read_opts(op, opts, shapes::RemoveImage::KEYS)? {
                    Ok(opts) => opts,
                    Err(e) => return Ok(Err(e)),
                };
                Ok(options::image(&image).map(|image| (image, opts)))
            },
            |docker, (image, opts)| async move {
                let options = query::RemoveImageOptions {
                    force: opts.bool("force")?.unwrap_or(false),
                    noprune: opts.bool("noprune")?.unwrap_or(false),
                    ..Default::default()
                };
                table(docker.remove_image(&image, Some(options), None).await?)
            },
        );

        /// The volumes: Docker's `{ Volumes = { ... }, Warnings }` (each volume `Name`, `Driver`, `Mountpoint`, `Labels`, ...).
        #[lua(name = "list_volumes", takes(opts = decl::<Option<shapes::Filtered>>()))]
        call(
            methods,
            "bollard.list_volumes",
            only_opts(shapes::Filtered::KEYS),
            |docker, opts| async move {
                let options = query::ListVolumesOptions {
                    filters: opts.filters()?,
                };
                table(docker.list_volumes(Some(options)).await?)
            },
        );
        /// Create a volume from Docker's create body (`{ Name = "data", Labels = { ... } }`; without a `Name` Docker makes one up). Returns the volume. Creating one that exists returns it.
        #[lua(name = "create_volume", takes(config = BODY))]
        call(
            methods,
            "bollard.create_volume",
            body_of,
            |docker, config| async move {
                let body: VolumeCreateRequest = options::body(config, "the configuration")?;
                table(docker.create_volume(body).await?)
            },
        );
        /// Remove a volume. One a container uses is `conflict`.
        #[lua(
            name = "remove_volume",
            takes("name: string", opts = decl::<Option<shapes::Force>>())
        )]
        call(
            methods,
            "bollard.remove_volume",
            named_with(VOLUME, shapes::Force::KEYS),
            |docker, (name, opts)| async move {
                let options = query::RemoveVolumeOptions {
                    force: opts.bool("force")?.unwrap_or(false),
                };
                docker.remove_volume(&name, Some(options)).await?;
                Ok(true)
            },
        );

        /// The networks, as Docker's list (`Id`, `Name`, `Driver`, `Scope`, `Labels`, ...).
        #[lua(name = "list_networks", takes(opts = decl::<Option<shapes::Filtered>>()))]
        call(
            methods,
            "bollard.list_networks",
            only_opts(shapes::Filtered::KEYS),
            |docker, opts| async move {
                let options = query::ListNetworksOptions {
                    filters: opts.filters()?,
                };
                table(docker.list_networks(Some(options)).await?)
            },
        );
        /// Create a network from Docker's create body (`{ Name = "backend", Driver = "bridge", Labels = { ... } }`). Returns `{ Id, Warning }`. A name in use is `conflict`.
        #[lua(name = "create_network", takes(config = BODY))]
        call(
            methods,
            "bollard.create_network",
            body_of,
            |docker, config| async move {
                let body: NetworkCreateRequest = options::body(config, "the configuration")?;
                table(docker.create_network(body).await?)
            },
        );
        /// Remove a network, by name or id. One with containers attached is an error (`api`: Docker answers 403).
        #[lua(name = "remove_network", takes("name: string"))]
        call(
            methods,
            "bollard.remove_network",
            named(NETWORK),
            |docker, name| async move {
                docker.remove_network(&name).await?;
                Ok(true)
            },
        );

        /// What happens on the daemon from now on, as a `Stream` of Docker's event tables (`Type = "container"`, `Action = "start"`, `Actor = { ID, Attributes }`, `time`, ...). `since` (a Unix time) replays earlier events first, `until` ends the stream at that time, `filters` are Docker's (`{ type = "container", event = { "start", "die" } }`). Without `until` the stream stays open until it is closed.
        #[lua(name = "events", takes(opts = decl::<Option<shapes::Events>>()))]
        stream_call(
            methods,
            "bollard.events",
            |op, opts: Option<Value>| {
                let opts = match read_opts(op, opts, shapes::Events::KEYS)? {
                    Ok(opts) => opts,
                    Err(e) => return Ok(Err(e)),
                };
                Ok((|| {
                    Ok(query::EventsOptions {
                        since: opts.int("since")?.map(|time| time.to_string()),
                        until: opts.int("until")?.map(|time| time.to_string()),
                        filters: opts.filters()?,
                    })
                })())
            },
            |docker, options| docker.events(Some(options)).map(|event| json_item(event?)),
        );
    }
}

fn logs_options(opts: &Opts) -> Result<query::LogsOptions, BollardError> {
    Ok(query::LogsOptions {
        follow: opts.bool("follow")?.unwrap_or(false),
        stdout: opts.bool("stdout")?.unwrap_or(true),
        stderr: opts.bool("stderr")?.unwrap_or(true),
        since: opts.int("since")?.unwrap_or(0),
        until: opts.int("until")?.unwrap_or(0),
        timestamps: opts.bool("timestamps")?.unwrap_or(false),
        tail: match opts.int("tail")? {
            Some(lines) if lines < 0 => {
                return Err(BollardError::invalid_input(
                    "option `tail` must be a number of lines, zero or more",
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

fn exec_config(cmd: Json, opts: &Opts) -> Result<ExecConfig, BollardError> {
    let cmd: Vec<String> = serde_json::from_value(cmd)
        .ok()
        .filter(|cmd: &Vec<String>| !cmd.is_empty())
        .ok_or_else(|| {
            BollardError::invalid_input(
                "the command must be a list of strings: the program, then its arguments",
            )
        })?;
    Ok(ExecConfig {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        cmd: Some(cmd),
        env: opts.strings("env")?,
        user: opts.string("user")?,
        working_dir: opts.string("working_dir")?,
        privileged: opts.bool("privileged")?,
        ..Default::default()
    })
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
