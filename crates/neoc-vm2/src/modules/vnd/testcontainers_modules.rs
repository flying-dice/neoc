//! Disposable backing services: `vnd:testcontainers_modules` (the `testcontainers-modules` crate).
//!
//! Start MySQL or Postgres in a Docker container, get a connection URL, stop it when done. Scripts (and tests) use
//! this instead of depending on a server someone installed. `generic` runs any image: its name, tag, exposed
//! ports, environment, command and what to wait for.
//!
//! ```lua
//! local testcontainers_modules = require("vnd:testcontainers_modules")
//!
//! -- the typed shortcuts
//! local mysql = assert(testcontainers_modules.mysql())
//! local db = assert(require("vnd:sqlx_mysql").open(mysql:url()))
//! mysql:stop()
//!
//! -- any image
//! local redis = assert(testcontainers_modules.generic({
//!     image = "redis",
//!     tag = "7-alpine",
//!     ports = { 6379 },
//!     wait = { type = "stdout", message = "Ready to accept connections" },
//! }))
//! local host = redis:host() -- "127.0.0.1"
//! local port = redis:port(6379) -- the port of the host that reaches the container's 6379
//! redis:stop()
//! ```
//!
//! Needs Docker on the host. Starting waits without holding the VM: it pulls the image on first use and returns
//! once the container is ready (the image's own probe for MySQL and Postgres, `wait` for `generic`). A container
//! that is not stopped is stopped and removed when its handle is collected (the crate does it when the handle is
//! dropped).
//!
//! Ported from the old VM's `vnd:testcontainers_modules`. What changed on purpose:
//! - A failure is a typed [`ContainerError`](testcontainers_modules::ContainerError) (`docker`, `not_ready`,
//!   `not_exposed`, `other`); it was the text `testcontainers_modules.<function>: <message>`.
//! - What `generic` takes is a typed table: a missing `image`, a key it does not have, a `wait` whose `type` is not
//!   one of its kinds are refused as a wrong argument, with the place; they were a returned error, and an unknown
//!   key was ignored.
//! - `wait = { type = "log" }` is gone: it was another name for `stdout`.
//! - `stop` returns `true`, or `nil` and the error; a stop that failed was raised.
//! - `GenericContainer:port` of a port that was not exposed raises a typed `not_exposed` error with the `port`.

#[mlua_ir::module("vnd:testcontainers_modules")]
pub mod testcontainers_modules {
    use std::collections::HashMap;
    use std::sync::Arc;

    use mlua_ir::{LuaError, Object, Raise, Shape};
    use serde::{Deserialize, Serialize};
    use testcontainers_modules::mysql::Mysql;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use testcontainers_modules::testcontainers::{
        ContainerAsync, ContainerRequest, GenericImage, Image, ImageExt, TestcontainersError,
    };
    use tokio::sync::Mutex;

    /// Where a container's ports are published.
    const HOST: &str = "127.0.0.1";

    /// Why a container did not start, stop or answer.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ContainerError {
        /// Docker refused or could not be reached: no daemon, no such image, a name in use.
        #[error("{reason}")]
        Docker {
            /// What the Docker client said.
            reason: String,
        },
        /// The container started and what was waited for did not happen: the message never came, the container
        /// exited, the wait timed out.
        #[error("{reason}")]
        NotReady {
            /// What was waited for, and how it ended.
            reason: String,
        },
        /// The port is not one the container was started with (`ports`).
        #[error("port {port} was not exposed at start")]
        NotExposed {
            /// The container's port that was asked for.
            port: u16,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl From<TestcontainersError> for ContainerError {
        fn from(error: TestcontainersError) -> Self {
            let reason = error.to_string();
            match error {
                TestcontainersError::Client(_) => Self::Docker { reason },
                TestcontainersError::WaitContainer(_) => Self::NotReady { reason },
                TestcontainersError::PortNotExposed { port, .. } => Self::NotExposed { port: port.as_u16() },
                _ => Self::Other { reason },
            }
        }
    }

    /// A running container, until it is stopped. Clones are the same container.
    struct Running<I: Image>(Arc<Mutex<Option<ContainerAsync<I>>>>);

    impl<I: Image> Clone for Running<I> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }

    impl<I: Image> Running<I> {
        fn new(container: ContainerAsync<I>) -> Self {
            Self(Arc::new(Mutex::new(Some(container))))
        }

        /// Stop the container and let go of it (which removes it). Stopping a stopped one succeeds.
        async fn stop(&self) -> Result<(), ContainerError> {
            let Some(container) = self.0.lock().await.take() else { return Ok(()) };
            container.stop().await?;
            Ok(())
        }
    }

    /// Start `image` as it is by default; the container, and the host's port that reaches `port` of it.
    async fn start_default<I: Image + Default>(port: u16) -> Result<(Running<I>, u16), ContainerError> {
        let container = I::default().start().await?;
        let mapped = container.get_host_port_ipv4(port).await?;
        Ok((Running::new(container), mapped))
    }

    /// A MySQL server in a container.
    #[derive(Clone, Object)]
    pub struct MysqlContainer {
        running: Running<Mysql>,
        port: u16,
    }

    #[mlua_ir::object]
    impl MysqlContainer {
        /// The connection URL, as `vnd:sqlx_mysql`'s `open` takes it: user `root`, no password, database `test`.
        pub fn url(&self) -> String {
            format!("mysql://root@{HOST}:{}/test", self.port)
        }

        /// The host the container's ports are published on: `"127.0.0.1"`.
        pub fn host(&self) -> String {
            HOST.to_owned()
        }

        /// The host's port that reaches MySQL's 3306.
        pub fn port(&self) -> u16 {
            self.port
        }

        /// Stop the container and remove it. Stopping a stopped one succeeds. A container that is not stopped is
        /// stopped when its handle is collected.
        #[host("Wait")]
        pub async fn stop(&self) -> Result<(), ContainerError> {
            self.running.stop().await
        }
    }

    /// A Postgres server in a container.
    #[derive(Clone, Object)]
    pub struct PostgresContainer {
        running: Running<Postgres>,
        port: u16,
    }

    #[mlua_ir::object]
    impl PostgresContainer {
        /// The connection URL, as `vnd:sqlx_postgres`'s `open` takes it: user `postgres`, password `postgres`,
        /// database `postgres`.
        pub fn url(&self) -> String {
            format!("postgres://postgres:postgres@{HOST}:{}/postgres", self.port)
        }

        /// The host the container's ports are published on: `"127.0.0.1"`.
        pub fn host(&self) -> String {
            HOST.to_owned()
        }

        /// The host's port that reaches Postgres's 5432.
        pub fn port(&self) -> u16 {
            self.port
        }

        /// Stop the container and remove it. Stopping a stopped one succeeds. A container that is not stopped is
        /// stopped when its handle is collected.
        #[host("Wait")]
        pub async fn stop(&self) -> Result<(), ContainerError> {
            self.running.stop().await
        }
    }

    /// Start a MySQL container as the image is by default; it is returned once MySQL accepts connections.
    ///
    /// ```luau
    /// local mysql = assert(testcontainers_modules.mysql())
    /// local db = assert(require("vnd:sqlx_mysql").open(mysql:url()))
    /// db:close()
    /// assert(mysql:stop())
    /// ```
    #[host("Wait")]
    pub async fn mysql() -> Result<MysqlContainer, ContainerError> {
        let (running, port) = start_default::<Mysql>(3306).await?;
        Ok(MysqlContainer { running, port })
    }

    /// Start a Postgres container as the image is by default; it is returned once Postgres accepts connections.
    #[host("Wait")]
    pub async fn postgres() -> Result<PostgresContainer, ContainerError> {
        let (running, port) = start_default::<Postgres>(5432).await?;
        Ok(PostgresContainer { running, port })
    }

    /// What a container must do before `generic` returns it.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Wait {
        /// It wrote `message` to its standard output.
        Stdout {
            /// The text to wait for, anywhere in the output.
            message: String,
        },
        /// It wrote `message` to its standard error.
        Stderr {
            /// The text to wait for, anywhere in the output.
            message: String,
        },
        /// `seconds` seconds passed.
        Seconds {
            /// How long to wait.
            seconds: u32,
        },
        /// Its image's health check reports it healthy.
        Healthcheck,
    }

    impl From<Wait> for WaitFor {
        fn from(wait: Wait) -> Self {
            match wait {
                Wait::Stdout { message } => WaitFor::message_on_stdout(message),
                Wait::Stderr { message } => WaitFor::message_on_stderr(message),
                Wait::Seconds { seconds } => WaitFor::seconds(seconds.into()),
                Wait::Healthcheck => WaitFor::healthcheck(),
            }
        }
    }

    /// The container `generic` starts.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct ContainerSpec {
        /// The image's name: `redis`, `registry:5000/team/app`.
        pub image: String,
        /// The image's tag; `latest` when not given.
        pub tag: Option<String>,
        /// The container's TCP ports to publish on the host; `port` gives where each one is.
        pub ports: Option<Vec<u16>>,
        /// Environment variables for the container.
        pub env: Option<HashMap<String, String>>,
        /// The command to run instead of the image's own: the program, then its arguments.
        pub cmd: Option<Vec<String>>,
        /// What to wait for before the container is returned; nothing when not given.
        pub wait: Option<Wait>,
    }

    fn request(spec: ContainerSpec) -> ContainerRequest<GenericImage> {
        let mut image = GenericImage::new(spec.image, spec.tag.unwrap_or_else(|| "latest".to_owned()));
        for port in spec.ports.iter().flatten() {
            image = image.with_exposed_port(port.tcp());
        }
        if let Some(wait) = spec.wait {
            image = image.with_wait_for(wait.into());
        }
        let mut request: ContainerRequest<GenericImage> = image.into();
        for (name, value) in spec.env.into_iter().flatten() {
            request = request.with_env_var(name, value);
        }
        if let Some(cmd) = spec.cmd {
            request = request.with_cmd(cmd);
        }
        request
    }

    /// A container of any image.
    #[derive(Clone, Object)]
    pub struct GenericContainer {
        running: Running<GenericImage>,
        /// The container's port to the host's, for every port it was started with: `port` only looks one up.
        ports: Arc<HashMap<u16, u16>>,
    }

    #[mlua_ir::object]
    impl GenericContainer {
        /// The host the container's ports are published on: `"127.0.0.1"`.
        pub fn host(&self) -> String {
            HOST.to_owned()
        }

        /// The host's port that reaches the container's `container_port`. Only the ports the container was started
        /// with (`ports`) have one; a port that was not raises `not_exposed`.
        pub fn port(&self, container_port: u16) -> Result<u16, Raise<ContainerError>> {
            let port = self.ports.get(&container_port).copied();
            Ok(port.ok_or(ContainerError::NotExposed { port: container_port })?)
        }

        /// Stop the container and remove it. Stopping a stopped one succeeds. A container that is not stopped is
        /// stopped when its handle is collected.
        #[host("Wait")]
        pub async fn stop(&self) -> Result<(), ContainerError> {
            self.running.stop().await
        }
    }

    /// Start a container of any image; it is returned once what `wait` names has happened (at once without it).
    ///
    /// ```luau
    /// local nginx = assert(testcontainers_modules.generic({
    ///     image = "nginx",
    ///     tag = "1.27-alpine",
    ///     ports = { 80 },
    ///     wait = { type = "stderr", message = "start worker processes" },
    /// }))
    /// local address = nginx:host() .. ":" .. nginx:port(80)
    /// assert(nginx:stop())
    /// ```
    #[host("Wait")]
    pub async fn generic(image: ContainerSpec) -> Result<GenericContainer, ContainerError> {
        let exposed = image.ports.clone().unwrap_or_default();
        let container = request(image).start().await?;
        let mut ports = HashMap::with_capacity(exposed.len());
        for port in exposed {
            ports.insert(port, container.get_host_port_ipv4(port).await?);
        }
        Ok(GenericContainer { running: Running::new(container), ports: Arc::new(ports) })
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::testcontainers_modules::ContainerError;

    fn engine() -> Lua {
        crate::make_engine().expect("engine")
    }

    /// What `generic` cannot start is refused before Docker is asked anything: no daemon is needed to say so.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_generic_cannot_start_is_refused_with_its_place() {
        let refused: Vec<String> = engine()
            .load(
                r#"
                local testcontainers_modules = require("vnd:testcontainers_modules")
                local out = {}
                for _, image in {
                    {},
                    { image = 42 },
                    { image = "alpine", tga = "3.19" },
                    { image = "alpine", ports = { 70000 } },
                    { image = "alpine", ports = { http = 80 } },
                    { image = "alpine", wait = { type = "nonsense" } },
                    { image = "alpine", wait = { type = "seconds" } },
                    { image = "alpine", wait = { type = "log", message = "ready" } },
                    "alpine",
                } do
                    local ok, err = pcall(testcontainers_modules.generic, image)
                    assert(not ok, "accepted")
                    table.insert(out, tostring(err))
                end
                return out
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        let place = "vnd:testcontainers_modules.generic: argument #1 'image'";
        for reason in &refused {
            assert!(reason.contains(place), "{reason}");
        }
        assert!(refused[0].contains("image"), "{}", refused[0]);
        assert!(refused[2].contains("unknown field `tga`"), "{}", refused[2]);
        assert!(refused[3].contains("ports[1]"), "{}", refused[3]);
        assert!(refused[5].contains("`type` must be one of"), "{}", refused[5]);
        assert!(refused[7].contains("`type` must be one of"), "{}", refused[7]);
    }

    /// The error a script gets: its kinds, its fields and what `tostring` says.
    #[test]
    fn a_container_error_is_a_typed_table() {
        let lua = engine();
        let op = "vnd:testcontainers_modules.generic";
        let docker = ContainerError::Docker { reason: "no daemon".to_owned() };
        lua.globals().set("docker", mlua_ir::error_value(&docker, &lua, op).unwrap()).unwrap();
        let unexposed = ContainerError::NotExposed { port: 1234 };
        lua.globals().set("unexposed", mlua_ir::error_value(&unexposed, &lua, op).unwrap()).unwrap();
        let (kind, reason, text, port): (String, String, String, u16) = lua
            .load("return docker.kind, docker.reason, tostring(unexposed), unexposed.port")
            .eval()
            .unwrap();
        assert_eq!((kind.as_str(), reason.as_str(), port), ("docker", "no daemon", 1234));
        assert_eq!(text, "vnd:testcontainers_modules.generic: not_exposed: port 1234 was not exposed at start");
    }

    /// neoc's Luau suite for the module, against the Docker daemon of this machine:
    /// `cargo test -p neoc-vm2 testcontainers_modules -- --ignored`. It pulls `alpine:3.19` and `nginx:1.27-alpine`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a running Docker daemon"]
    async fn generic_containers_start_publish_their_ports_and_stop() {
        engine()
            .load(
                r#"
                local testcontainers_modules = require("vnd:testcontainers_modules")
                local hyper = require("vnd:hyper")

                -- a command of the script's, and an environment
                local alpine, err = testcontainers_modules.generic({
                    image = "alpine",
                    tag = "3.19",
                    cmd = { "sleep", "60" },
                    env = { FOO = "bar" },
                })
                assert(err == nil, tostring(err))
                assert(alpine:host() == "127.0.0.1")
                -- a port it was not started with
                local ok, unexposed = pcall(function() return alpine:port(1234) end)
                assert(not ok and tostring(unexposed):find("not_exposed: port 1234 was not exposed", 1, true), tostring(unexposed))
                assert(alpine:stop() == true)
                assert(alpine:stop() == true, "stopping a stopped container succeeds")

                -- a published port, reached from the host once nginx says it is ready
                local nginx = assert(testcontainers_modules.generic({
                    image = "nginx",
                    tag = "1.27-alpine",
                    ports = { 80 },
                    wait = { type = "stderr", message = "start worker processes" },
                }))
                local port = nginx:port(80)
                assert(type(port) == "number" and port > 0)
                local response, http_err = hyper.get("http://" .. nginx:host() .. ":" .. port .. "/")
                assert(http_err == nil, tostring(http_err))
                assert(response.status == 200)
                assert(nginx:stop())

                -- an image there is none of: Docker's refusal, typed
                local none, missing = testcontainers_modules.generic({ image = "neoc-no-such-image", tag = "0.0.0" })
                assert(none == nil and missing ~= nil)
                assert(missing.kind == "docker" and missing.op == "vnd:testcontainers_modules.generic", tostring(missing))
                "#,
            )
            .exec_async()
            .await
            .unwrap();
    }

    /// The typed shortcuts, each used through its driver: `cargo test -p neoc-vm2 testcontainers_modules --
    /// --ignored`. It pulls the MySQL and Postgres images the crate names.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs a running Docker daemon"]
    async fn mysql_and_postgres_containers_answer_their_drivers() {
        engine()
            .load(
                r#"
                local testcontainers_modules = require("vnd:testcontainers_modules")

                local mysql = assert(testcontainers_modules.mysql())
                assert(mysql:host() == "127.0.0.1" and mysql:port() > 0)
                assert(mysql:url() == "mysql://root@127.0.0.1:" .. mysql:port() .. "/test")
                local db = assert(require("vnd:sqlx_mysql").open(mysql:url()))
                assert(assert(db:query_one("SELECT 1 + 1 AS two")).two == 2)
                db:close()
                assert(mysql:stop())

                local postgres = assert(testcontainers_modules.postgres())
                assert(postgres:url() == "postgres://postgres:postgres@127.0.0.1:" .. postgres:port() .. "/postgres")
                local pg = assert(require("vnd:sqlx_postgres").open(postgres:url()))
                assert(assert(pg:query_one("SELECT 1 + 1 AS two")).two == 2)
                pg:close()
                assert(postgres:stop())
                "#,
            )
            .exec_async()
            .await
            .unwrap();
    }
}
