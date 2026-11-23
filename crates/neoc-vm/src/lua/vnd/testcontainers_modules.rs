//! Disposable backing services — `lib.testcontainers_modules`.
//!
//! Spin up MySQL or Postgres in a Docker container, get a connection URL,
//! stop it when done. Scripts (and our integration tests) use this to avoid
//! depending on external infrastructure. `generic` covers any image — pass
//! the image name + tag, exposed ports, env, cmd, and a wait strategy.
//!
//! ```lua
//! -- Typed shortcuts
//! local container = lib.testcontainers_modules.mysql()
//! local db = lib.sqlx_mysql.open(container:url())
//! container:stop()
//!
//! -- Any image
//! local redis = lib.testcontainers_modules.generic({
//!     image = "redis",
//!     tag   = "7-alpine",
//!     ports = { 6379 },
//!     wait  = { type = "log", message = "Ready to accept connections" },
//! })
//! local host = redis:host()       -- "127.0.0.1"
//! local port = redis:port(6379)   -- mapped host port
//! redis:stop()
//! ```
//!
//! Requires Docker on the host. `start` is async — it pulls the image on
//! first use and waits for the container's readiness probe to succeed before
//! returning. `stop` is also async; if you forget to call it, the container
//! stops when its handle is garbage-collected (testcontainers does the
//! cleanup via Docker on drop).

use std::collections::HashMap;
use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataMethods};
use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{
    ContainerAsync, ContainerRequest, GenericImage, ImageExt,
};
use tokio::sync::Mutex;
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::lifecycle::{Role, async_function, async_method};
use crate::lua::sandbox::{Fallible, from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:testcontainers_modules` surface through the facade.
///
/// `mysql`, `postgres`, and `generic` are set verbatim with
/// [`crate::lua::facade::Module::set`] rather than `try_async_func` because
/// each hand-written async function carries its own
/// `#[instrument(name = "testcontainers_modules.<fn>", target = "lua::lib::testcontainers_modules")]`
/// attribute and bakes the `testcontainers_modules.<fn>` error prefix into its
/// `Fallible` return — routing through the facade would add a
/// second span and double the prefix. Their signatures are read by `neoc_macros`
/// from the implementation. The `MysqlContainer`,
/// `PostgresContainer`, and `GenericContainer` handle types keep their own
/// `impl UserData` (the sync accessors and async `stop` unchanged) and are
/// recorded via [`crate::lua::facade::Module::userdata`]. The `mysql` /
/// `postgres` shortcuts share one generic `TypedContainer<I>` Rust type at
/// runtime; the recording covers the two image-specific surfaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:testcontainers_modules")?;

    m.declared(
        start_mysql::lua(),
        async_function(lua, Role::Wait, |lua, _: ()| start_mysql(lua))?,
    )?;

    m.declared(
        start_postgres::lua(),
        async_function(lua, Role::Wait, |lua, _: ()| start_postgres(lua))?,
    )?;

    m.declared(
        start_generic::lua(),
        async_function(lua, Role::Wait, |lua, opts: Table| start_generic(lua, opts))?,
    )?;

    m.userdata(Mysql::TYPE_NAME, TypedContainer::<Mysql>::lua);

    m.userdata(Postgres::TYPE_NAME, TypedContainer::<Postgres>::lua);

    m.userdata("GenericContainer", |c| {
        GenericContainer::lua(c);
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Typed container (Mysql / Postgres)
// ---------------------------------------------------------------------------

/// Image-specific facts the shared start flow needs.
trait TypedImage:
    testcontainers_modules::testcontainers::Image + Default + Send + Sync + 'static
{
    /// The name of the container's type in the module (`MysqlContainer`).
    const TYPE_NAME: &'static str;
    const CONTAINER_PORT: u16;
    const OP_NAME: &'static str;
    /// What the declarations say of `url` and of `port`.
    const URL_DOC: &'static str;
    const PORT_DOC: &'static str;
    fn url(host: &str, port: u16) -> String;
}

impl TypedImage for Mysql {
    const TYPE_NAME: &'static str = "MysqlContainer";
    const CONTAINER_PORT: u16 = 3306;
    const OP_NAME: &'static str = "testcontainers_modules.mysql";
    const URL_DOC: &'static str = "Return a MySQL connection URL pre-formatted for `sqlx_mysql.open` (user `root`, no password, database `test`).";
    const PORT_DOC: &'static str = "Return the mapped host port for MySQL's 3306.";

    /// `Mysql::default()` exposes db `test` to user `root` with no password.
    fn url(host: &str, port: u16) -> String {
        format!("mysql://root@{host}:{port}/test")
    }
}

impl TypedImage for Postgres {
    const TYPE_NAME: &'static str = "PostgresContainer";
    const CONTAINER_PORT: u16 = 5432;
    const OP_NAME: &'static str = "testcontainers_modules.postgres";
    const URL_DOC: &'static str = "Return a Postgres connection URL pre-formatted for `sqlx_postgres.open` (user `postgres`, password `postgres`, database `postgres`).";
    const PORT_DOC: &'static str = "Return the mapped host port for Postgres's 5432.";

    /// `Postgres::default()` exposes db `postgres` to user `postgres`/`postgres`.
    fn url(host: &str, port: u16) -> String {
        format!("postgres://postgres:postgres@{host}:{port}/postgres")
    }
}

#[derive(Clone)]
pub struct TypedContainer<I: testcontainers_modules::testcontainers::Image> {
    inner: Arc<Mutex<Option<ContainerAsync<I>>>>,
    url: String,
    host: String,
    port: u16,
}

impl<I: TypedImage> crate::lua::facade::DeclType for TypedContainer<I> {
    fn decl_type() -> String {
        I::TYPE_NAME.to_owned()
    }
}

async fn start_typed<I: TypedImage>(_: Lua) -> mlua::Result<Fallible<TypedContainer<I>>> {
    let container = match I::default().start().await {
        Ok(c) => c,
        Err(e) => return from_err(I::OP_NAME, e),
    };
    let port = match container.get_host_port_ipv4(I::CONTAINER_PORT).await {
        Ok(p) => p,
        Err(e) => return from_err(I::OP_NAME, e),
    };
    let host = "127.0.0.1".to_string();
    let url = I::url(&host, port);
    ok(TypedContainer::<I> {
        inner: Arc::new(Mutex::new(Some(container))),
        url,
        host,
        port,
    })
}

#[neoc_macros::lua_userdata]
impl<I> UserData for TypedContainer<I>
where
    I: TypedImage,
{
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        #[lua(doc = I::URL_DOC)]
        methods.add_method("url", |_, this, _: ()| Ok(this.url.clone()));
        /// Return the container host (always `"127.0.0.1"`).
        methods.add_method("host", |_, this, _: ()| Ok(this.host.clone()));
        #[lua(doc = I::PORT_DOC)]
        methods.add_method("port", |_, this, _: ()| Ok(this.port as i32));
        /// Stop the container; idempotent. The container also stops on garbage collection if not stopped explicitly.
        #[lua("()")]
        async_method(methods, "stop", Role::Wait, |_, this, _: ()| {
            let inner = this.inner.clone();
            async move { stop_container(inner).await }
        });
    }
}

/// Start a MySQL container with default settings, returning a handle once it is ready to accept connections.
#[neoc_macros::lua_fn(name = "mysql", takes())]
#[instrument(
    name = "testcontainers_modules.mysql",
    target = "lua::lib::testcontainers_modules",
    skip_all
)]
async fn start_mysql(lua: Lua) -> mlua::Result<Fallible<TypedContainer<Mysql>>> {
    start_typed::<Mysql>(lua).await
}

/// Start a Postgres container with default settings, returning a handle once it is ready.
#[neoc_macros::lua_fn(name = "postgres", takes())]
#[instrument(
    name = "testcontainers_modules.postgres",
    target = "lua::lib::testcontainers_modules",
    skip_all
)]
async fn start_postgres(lua: Lua) -> mlua::Result<Fallible<TypedContainer<Postgres>>> {
    start_typed::<Postgres>(lua).await
}

async fn stop_container<I>(inner: Arc<Mutex<Option<ContainerAsync<I>>>>) -> mlua::Result<()>
where
    I: testcontainers_modules::testcontainers::Image,
{
    let mut guard = inner.lock().await;
    if let Some(c) = guard.take() {
        c.stop().await.map_err(mlua::Error::external)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Generic
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GenericContainer {
    inner: Arc<Mutex<Option<ContainerAsync<GenericImage>>>>,
    host: String,
    /// Container port → mapped host port. Populated at start time for every
    /// port listed in `opts.ports`, so `:port(p)` is a sync lookup.
    ports: HashMap<u16, u16>,
}

struct GenericOpts {
    image: String,
    tag: String,
    ports: Vec<u16>,
    wait: Option<WaitFor>,
    env: Vec<(String, String)>,
    cmd: Option<Vec<String>>,
}

fn parse_generic_opts(opts: &Table) -> mlua::Result<GenericOpts> {
    let image: String = opts.get("image")?;
    let tag: String = opts
        .get::<Option<String>>("tag")?
        .unwrap_or_else(|| "latest".to_string());

    let ports = match opts.get::<Option<Table>>("ports")? {
        Some(t) => t
            .sequence_values::<u16>()
            .collect::<mlua::Result<Vec<_>>>()?,
        None => Vec::new(),
    };

    let wait = match opts.get::<Option<Table>>("wait")? {
        Some(t) => Some(parse_wait(&t).map_err(mlua::Error::external)?),
        None => None,
    };

    let env = match opts.get::<Option<Table>>("env")? {
        Some(t) => t
            .pairs::<String, String>()
            .collect::<mlua::Result<Vec<_>>>()?,
        None => Vec::new(),
    };

    let cmd = match opts.get::<Option<Table>>("cmd")? {
        Some(t) => Some(
            t.sequence_values::<String>()
                .collect::<mlua::Result<Vec<_>>>()?,
        ),
        None => None,
    };

    Ok(GenericOpts {
        image,
        tag,
        ports,
        wait,
        env,
        cmd,
    })
}

fn build_request(parsed: GenericOpts) -> ContainerRequest<GenericImage> {
    let GenericOpts {
        image,
        tag,
        ports,
        wait,
        env,
        cmd,
    } = parsed;

    let mut img = GenericImage::new(&image, &tag);
    for p in &ports {
        img = img.with_exposed_port((*p).tcp());
    }
    if let Some(w) = wait {
        img = img.with_wait_for(w);
    }

    let mut req: ContainerRequest<GenericImage> = img.into();
    for (k, v) in env {
        req = req.with_env_var(k, v);
    }
    if let Some(parts) = cmd {
        req = req.with_cmd(parts);
    }
    req
}

async fn map_ports(
    container: &ContainerAsync<GenericImage>,
    exposed_ports: &[u16],
) -> Result<HashMap<u16, u16>, testcontainers_modules::testcontainers::TestcontainersError> {
    let mut ports = HashMap::with_capacity(exposed_ports.len());
    for p in exposed_ports {
        let host_port = container.get_host_port_ipv4(*p).await?;
        ports.insert(*p, host_port);
    }
    Ok(ports)
}

// The options are read here, before the future exists (see `vnd:hyper`'s client): a script table can run script code
// when it is read, and read inside the future each level of a recursion would stack another polled future.
/// Start a generic Docker container from any image given `{ image, tag?, ports?, env?, cmd?, wait? }`, returning a handle once its readiness probe passes.
#[neoc_macros::lua_fn(name = "generic", takes("opts: LuaValue"))]
fn start_generic(
    lua: Lua,
    opts: Table,
) -> impl std::future::Future<Output = mlua::Result<Fallible<GenericContainer>>> + Send + 'static {
    start_generic_with(lua, parse_generic_opts(&opts))
}

#[instrument(
    name = "testcontainers_modules.generic",
    target = "lua::lib::testcontainers_modules",
    skip_all
)]
async fn start_generic_with(
    _: Lua,
    parsed: mlua::Result<GenericOpts>,
) -> mlua::Result<Fallible<GenericContainer>> {
    let op = "testcontainers_modules.generic";

    let parsed = match parsed {
        Ok(p) => p,
        Err(e) => return from_err(op, e),
    };
    let exposed_ports = parsed.ports.clone();
    let req = build_request(parsed);

    let container = match req.start().await {
        Ok(c) => c,
        Err(e) => return from_err(op, e),
    };

    let ports = match map_ports(&container, &exposed_ports).await {
        Ok(p) => p,
        Err(e) => return from_err(op, e),
    };

    ok(GenericContainer {
        inner: Arc::new(Mutex::new(Some(container))),
        host: "127.0.0.1".to_string(),
        ports,
    })
}

fn parse_wait(t: &Table) -> Result<WaitFor, String> {
    let kind: String = t.get("type").map_err(|e| e.to_string())?;
    Ok(match kind.as_str() {
        "log" | "stdout" => {
            let msg: String = t.get("message").map_err(|e| e.to_string())?;
            WaitFor::message_on_stdout(msg)
        }
        "stderr" => {
            let msg: String = t.get("message").map_err(|e| e.to_string())?;
            WaitFor::message_on_stderr(msg)
        }
        "seconds" => {
            let n: u64 = t.get("seconds").map_err(|e| e.to_string())?;
            WaitFor::seconds(n)
        }
        "healthcheck" => WaitFor::healthcheck(),
        other => return Err(format!("unknown wait type \"{other}\"")),
    })
}

#[neoc_macros::lua_userdata("vnd:testcontainers_modules", "GenericContainer")]
impl UserData for GenericContainer {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the container host (always `"127.0.0.1"`).
        methods.add_method("host", |_, this, _: ()| Ok(this.host.clone()));
        /// Return the mapped host port for the given container port; only ports listed at start time are available, requesting an unmapped port errors.
        #[lua(types(container_port = "number"))]
        methods.add_method("port", |_, this, container_port: u16| {
            this.ports
                .get(&container_port)
                .copied()
                .map(|p| p as i32)
                .ok_or_else(|| {
                    mlua::Error::external(format!(
                        "testcontainers_modules.generic: port {container_port} was not exposed at start"
                    ))
                })
        });
        /// Stop the container; idempotent.
        #[lua("()")]
        async_method(methods, "stop", Role::Wait, |_, this, _: ()| {
            let inner = this.inner.clone();
            async move { stop_container(inner).await }
        });
    }
}
