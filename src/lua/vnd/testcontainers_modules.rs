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

use mlua::{Lua, Table, UserData, UserDataMethods, Value};
use testcontainers_modules::mysql::Mysql;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{
    ContainerAsync, ContainerRequest, GenericImage, ImageExt,
};
use tokio::sync::Mutex;
use tracing::instrument;

use crate::lua::sandbox::{from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "mysql",
        lua.create_async_function(|lua, _: ()| start_mysql(lua))?,
    )?;
    t.set(
        "postgres",
        lua.create_async_function(|lua, _: ()| start_postgres(lua))?,
    )?;
    t.set(
        "generic",
        lua.create_async_function(|lua, opts: Table| start_generic(lua, opts))?,
    )?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// MySQL
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MysqlContainer {
    inner: Arc<Mutex<Option<ContainerAsync<Mysql>>>>,
    url: String,
    host: String,
    port: u16,
}

// TODO: clean-code - 0.85 - DRY: MysqlContainer/PostgresContainer + start_mysql/start_postgres are near-identical twins (struct shape, start flow, UserData impl) — collapse via a typed-image trait or shared generic.
#[instrument(
    name = "testcontainers_modules.mysql",
    target = "luau::lib::testcontainers_modules",
    skip_all
)]
async fn start_mysql(lua: Lua) -> mlua::Result<(Value, Value)> {
    let container = match Mysql::default().start().await {
        Ok(c) => c,
        Err(e) => return from_err(&lua, "testcontainers_modules.mysql", e),
    };
    let port = match container.get_host_port_ipv4(3306).await {
        Ok(p) => p,
        Err(e) => return from_err(&lua, "testcontainers_modules.mysql", e),
    };
    let host = "127.0.0.1".to_string();
    // `Mysql::default()` exposes db `test` to user `root` with no password.
    let url = format!("mysql://root@{host}:{port}/test");
    ok(
        &lua,
        MysqlContainer {
            inner: Arc::new(Mutex::new(Some(container))),
            url,
            host,
            port,
        },
    )
}

impl UserData for MysqlContainer {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_container_methods(methods, |c| c.url.clone(), |c| c.host.clone(), |c| c.port);
        methods.add_async_method("stop", |_, this, _: ()| {
            let inner = this.inner.clone();
            async move { stop_container(inner).await }
        });
    }
}

// ---------------------------------------------------------------------------
// Postgres
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PostgresContainer {
    inner: Arc<Mutex<Option<ContainerAsync<Postgres>>>>,
    url: String,
    host: String,
    port: u16,
}

#[instrument(
    name = "testcontainers_modules.postgres",
    target = "luau::lib::testcontainers_modules",
    skip_all
)]
async fn start_postgres(lua: Lua) -> mlua::Result<(Value, Value)> {
    let container = match Postgres::default().start().await {
        Ok(c) => c,
        Err(e) => return from_err(&lua, "testcontainers_modules.postgres", e),
    };
    let port = match container.get_host_port_ipv4(5432).await {
        Ok(p) => p,
        Err(e) => return from_err(&lua, "testcontainers_modules.postgres", e),
    };
    let host = "127.0.0.1".to_string();
    // `Postgres::default()` exposes db `postgres` to user `postgres`/`postgres`.
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    ok(
        &lua,
        PostgresContainer {
            inner: Arc::new(Mutex::new(Some(container))),
            url,
            host,
            port,
        },
    )
}

impl UserData for PostgresContainer {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_container_methods(methods, |c| c.url.clone(), |c| c.host.clone(), |c| c.port);
        methods.add_async_method("stop", |_, this, _: ()| {
            let inner = this.inner.clone();
            async move { stop_container(inner).await }
        });
    }
}

// ---------------------------------------------------------------------------
// Shared method registration / lifecycle
// ---------------------------------------------------------------------------

fn add_container_methods<T, M>(
    methods: &mut M,
    url: fn(&T) -> String,
    host: fn(&T) -> String,
    port: fn(&T) -> u16,
) where
    T: 'static,
    M: UserDataMethods<T>,
{
    methods.add_method("url", move |_, this, _: ()| Ok(url(this)));
    methods.add_method("host", move |_, this, _: ()| Ok(host(this)));
    methods.add_method("port", move |_, this, _: ()| Ok(port(this) as i32));
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

// TODO: clean-code - 0.8 - SRP: start_generic does 6 jobs in 86 lines — opts parsing, image build, wait, env, cmd, port mapping. Split into parse_opts / build_request / map_ports.
#[instrument(
    name = "testcontainers_modules.generic",
    target = "luau::lib::testcontainers_modules",
    skip_all
)]
async fn start_generic(lua: Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let image: String = match opts.get("image") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    };
    let tag: String = opts
        .get::<Option<String>>("tag")
        .ok()
        .flatten()
        .unwrap_or_else(|| "latest".to_string());

    let mut img = GenericImage::new(&image, &tag);

    // TODO: clean-code - 0.75 - DRY: `match opts.get::<Option<Table>>(...) { Ok => .., Err(e) => return from_err(...) }` is repeated 4+ times with the same literal error tag — extract a helper or use `?` with map_err.
    let exposed_ports: Vec<u16> = match opts.get::<Option<Table>>("ports") {
        Ok(Some(t)) => match t.sequence_values::<u16>().collect::<mlua::Result<Vec<_>>>() {
            Ok(v) => v,
            Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
        },
        Ok(None) => Vec::new(),
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    };
    for p in &exposed_ports {
        img = img.with_exposed_port((*p).tcp());
    }

    let wait_opts = match opts.get::<Option<Table>>("wait") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    };
    if let Some(w) = wait_opts {
        let strategy = match parse_wait(&w) {
            Ok(s) => s,
            Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
        };
        img = img.with_wait_for(strategy);
    }

    // Now move into ContainerRequest so we can apply env / cmd via ImageExt.
    let mut req: ContainerRequest<GenericImage> = img.into();

    if let Some(env) = match opts.get::<Option<Table>>("env") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    } {
        for pair in env.pairs::<String, String>() {
            let (k, v) = match pair {
                Ok(p) => p,
                Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
            };
            req = req.with_env_var(k, v);
        }
    }

    if let Some(cmd) = match opts.get::<Option<Table>>("cmd") {
        Ok(v) => v,
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    } {
        let parts: Vec<String> = match cmd.sequence_values::<String>().collect() {
            Ok(v) => v,
            Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
        };
        req = req.with_cmd(parts);
    }

    let container = match req.start().await {
        Ok(c) => c,
        Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
    };

    let mut ports = HashMap::with_capacity(exposed_ports.len());
    for p in &exposed_ports {
        let host_port = match container.get_host_port_ipv4(*p).await {
            Ok(hp) => hp,
            Err(e) => return from_err(&lua, "testcontainers_modules.generic", e),
        };
        ports.insert(*p, host_port);
    }

    ok(
        &lua,
        GenericContainer {
            inner: Arc::new(Mutex::new(Some(container))),
            host: "127.0.0.1".to_string(),
            ports,
        },
    )
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

impl UserData for GenericContainer {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("host", |_, this, _: ()| Ok(this.host.clone()));
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
        methods.add_async_method("stop", |_, this, _: ()| {
            let inner = this.inner.clone();
            async move { stop_container(inner).await }
        });
    }
}
