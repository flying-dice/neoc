//! `vnd:bollard` against a fake Docker API in this process: no test needs a Docker daemon. The fake answers the
//! endpoints the module calls with canned bodies in Docker's shapes, and keeps every request it saw, so a test can
//! hold the module to what it sent. `a_real_daemon_answers` is the one test against a real daemon, and is ignored.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use mlua::{Lua, Value};
use parking_lot::Mutex;
use serde_json::{Value as Json, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::lua::lifecycle::Life;
use crate::lua::sandbox::{make_engine, make_engine_in};

// ---------------------------------------------------------------------------
// the fake daemon
// ---------------------------------------------------------------------------

/// A request the fake daemon received.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    /// The path and query as sent: `/containers/json?all=true`.
    target: String,
    body: String,
}

#[derive(Default)]
struct Daemon {
    seen: Mutex<Vec<Seen>>,
    /// What feeds the body of `GET /events` and of a `follow` logs request: the test sends pieces, and dropping the
    /// sender ends the body.
    held_open: Mutex<Option<mpsc::Receiver<Bytes>>>,
}

type Body = BoxBody<Bytes, Infallible>;

fn full(status: u16, body: impl Into<Bytes>) -> Response<Body> {
    let mut response = Response::new(Full::new(body.into()).boxed());
    *response.status_mut() = StatusCode::from_u16(status).expect("status");
    response
}

fn answer(status: u16, body: &Json) -> Response<Body> {
    full(status, body.to_string())
}

fn refusal(status: u16, message: &str) -> Response<Body> {
    answer(status, &json!({ "message": message }))
}

/// One frame of Docker's multiplexed output: the stream (1 stdout, 2 stderr), three zero bytes, the length.
fn frame(stream: u8, data: &str) -> Vec<u8> {
    let mut out = vec![stream, 0, 0, 0];
    out.extend_from_slice(&u32::try_from(data.len()).expect("length").to_be_bytes());
    out.extend_from_slice(data.as_bytes());
    out
}

/// A volume as Docker describes one (bollard requires every one of these fields).
fn volume() -> Json {
    json!({
        "Name": "data",
        "Driver": "local",
        "Mountpoint": "/var/lib/docker/volumes/data/_data",
        "Labels": {},
        "Scope": "local",
        "Options": {},
    })
}

impl Daemon {
    /// A body that stays open: fed by the receiver a test put in `held_open`.
    fn held_open_body(&self) -> Response<Body> {
        let pieces = self
            .held_open
            .lock()
            .take()
            .expect("the test set no held-open body");
        let stream = futures::stream::unfold(pieces, |mut pieces| async {
            pieces
                .recv()
                .await
                .map(|piece| (Ok::<_, Infallible>(Frame::data(piece)), pieces))
        });
        Response::new(BodyExt::boxed(StreamBody::new(stream)))
    }

    fn respond(&self, method: &Method, path: &str, query: &str) -> Response<Body> {
        match (method.as_str(), path) {
            ("GET", "/version") => answer(
                200,
                &json!({ "Version": "25.0.3", "ApiVersion": "1.44", "Os": "linux" }),
            ),
            ("GET", "/_ping") => full(200, "OK"),
            ("GET", "/info") => answer(200, &json!({ "Containers": 2, "ServerVersion": "25.0.3" })),
            ("GET", "/containers/json") => answer(
                200,
                &json!([
                    {
                        "Id": "aaa111",
                        "Names": ["/web"],
                        "Image": "nginx:1.27",
                        "State": "running",
                        "Status": "Up 2 hours",
                        "Labels": { "com.docker.compose.project": "blog" },
                    },
                    { "Id": "bbb222", "Names": ["/db"], "Image": "postgres:16", "State": "exited", "Labels": {} },
                ]),
            ),
            ("GET", "/containers/web/json") => answer(
                200,
                &json!({
                    "Id": "aaa111",
                    "Name": "/web",
                    "State": { "Status": "running", "Running": true, "Health": null },
                    "Config": { "Image": "nginx:1.27", "Labels": { "app": "web" } },
                }),
            ),
            ("GET", "/containers/ghost/json") | ("POST", "/containers/ghost/start") => {
                refusal(404, "No such container: ghost")
            }
            ("POST", "/containers/create") => {
                answer(201, &json!({ "Id": "ccc333", "Warnings": [] }))
            }
            ("POST", "/containers/web/start") => full(304, ""),
            (
                "POST",
                "/containers/web/stop" | "/containers/web/restart" | "/containers/web/kill",
            ) => full(204, ""),
            ("DELETE", "/containers/web") if query.contains("force=true") => full(204, ""),
            ("DELETE", "/containers/web") => refusal(
                409,
                "cannot remove container \"/web\": container is running",
            ),
            ("GET", "/containers/web/logs") if query.contains("follow=true") => {
                self.held_open_body()
            }
            ("GET", "/containers/web/logs") => {
                let mut body = frame(1, "listening on :80\n");
                body.extend(frame(2, "warn: no config\n"));
                full(200, body)
            }
            ("GET", "/containers/ghost/logs") => refusal(404, "No such container: ghost"),
            ("GET", "/containers/web/stats") => answer(
                200,
                &json!({ "name": "/web", "memory_stats": { "usage": 1024, "limit": 4096 } }),
            ),
            ("GET", "/images/json") => answer(
                200,
                &json!([{
                    "Id": "sha256:111",
                    "ParentId": "",
                    "RepoTags": ["nginx:1.27"],
                    "RepoDigests": [],
                    "Created": 1_700_000_000,
                    "Size": 1000,
                    "SharedSize": -1,
                    "Labels": {},
                    "Containers": 1,
                }]),
            ),
            ("POST", "/images/create") if query.contains("fromImage=nope") => full(
                200,
                "{\"status\":\"Pulling\"}\n{\"errorDetail\":{\"message\":\"pull access denied for nope\"},\"error\":\"pull access denied for nope\"}\n",
            ),
            ("POST", "/images/create") => full(
                200,
                "{\"status\":\"Pulling from library/nginx\",\"id\":\"1.27\"}\n{\"status\":\"Downloading\",\"id\":\"abc\",\"progressDetail\":{\"current\":5,\"total\":10}}\n{\"status\":\"Status: Downloaded newer image for nginx:1.27\"}\n",
            ),
            ("DELETE", "/images/nginx:1.27") => answer(200, &json!([{ "Untagged": "nginx:1.27" }])),
            ("GET", "/volumes") => answer(200, &json!({ "Volumes": [volume()], "Warnings": [] })),
            ("POST", "/volumes/create") => answer(201, &volume()),
            ("DELETE", "/volumes/data") => refusal(409, "volume is in use"),
            ("GET", "/networks") => answer(
                200,
                &json!([{ "Id": "n1", "Name": "bridge", "Driver": "bridge" }]),
            ),
            ("POST", "/networks/create") => answer(201, &json!({ "Id": "n2", "Warning": "" })),
            ("DELETE", "/networks/backend") => full(204, ""),
            ("GET", "/events") => self.held_open_body(),
            _ => refusal(500, &format!("the fake daemon has no {method} {path}")),
        }
    }

    /// The requests seen so far, without the version negotiation.
    fn requests(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .iter()
            .filter(|seen| !seen.target.ends_with("/version"))
            .cloned()
            .collect()
    }

    /// The one request to `path` (given without the version prefix or the query).
    fn request_to(&self, path: &str) -> Seen {
        let matching: Vec<Seen> = self
            .requests()
            .into_iter()
            .filter(|seen| {
                seen.target
                    .split('?')
                    .next()
                    .is_some_and(|p| p.ends_with(path))
            })
            .collect();
        assert_eq!(matching.len(), 1, "requests to {path}: {matching:?}");
        matching[0].clone()
    }
}

/// Start a fake daemon on a local port: the daemon and its address as a `host` (`tcp://127.0.0.1:<port>`).
async fn daemon() -> (Arc<Daemon>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let host = format!("tcp://{}", listener.local_addr().expect("address"));
    let daemon = Arc::new(Daemon::default());
    let serving = Arc::clone(&daemon);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let daemon = Arc::clone(&serving);
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let daemon = Arc::clone(&daemon);
                    async move {
                        let method = request.method().clone();
                        let target = request.uri().to_string();
                        let body = request
                            .into_body()
                            .collect()
                            .await
                            .expect("body")
                            .to_bytes();
                        daemon.seen.lock().push(Seen {
                            method: method.to_string(),
                            target: target.clone(),
                            body: String::from_utf8_lossy(&body).into_owned(),
                        });
                        // a client may name the API version first (`/v1.44/containers/json`): the daemon serves
                        // both forms
                        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                        let path = path
                            .strip_prefix("/v")
                            .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
                            .and_then(|rest| rest.find('/').map(|slash| &rest[slash..]))
                            .unwrap_or(path);
                        Ok::<_, Infallible>(daemon.respond(&method, path, query))
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (daemon, host)
}

/// Run `script` with `host` as its argument, in a new engine.
async fn run<R: mlua::FromLuaMulti>(host: &str, script: &str) -> R {
    run_in(&make_engine().expect("engine"), host, script).await
}

async fn run_in<R: mlua::FromLuaMulti>(lua: &Lua, host: &str, script: &str) -> R {
    lua.load(script)
        .call_async(host)
        .await
        .unwrap_or_else(|e| panic!("the script failed: {e}"))
}

/// Run `script`, which returns the `(value, err)` of a failed call: the error string.
async fn error_of(host: &str, script: &str) -> String {
    let (value, err): (Value, Option<String>) = run(host, script).await;
    assert!(value.is_nil(), "the call succeeded: {value:?}");
    err.expect("an error string")
}

const TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// requests and answers
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_is_dockers_own_tables_and_the_options_are_its_query() {
    let (daemon, host) = daemon().await;
    let (names, project, count): (String, String, i64) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local list = assert(docker:list_containers({
            all = true,
            limit = 5,
            filters = { label = { "com.docker.compose.project=blog" }, status = "running" },
        }))
        return list[1].Names[1] .. "," .. list[2].Names[1], list[1].Labels["com.docker.compose.project"], #list
        "#,
    )
    .await;
    assert_eq!(
        (names.as_str(), project.as_str(), count),
        ("/web,/db", "blog", 2)
    );

    let seen = daemon.request_to("/containers/json");
    assert_eq!(seen.method, "GET");
    assert!(seen.target.contains("all=true"), "{}", seen.target);
    assert!(seen.target.contains("limit=5"), "{}", seen.target);
    let filters =
        url::form_urlencoded::parse(seen.target.split_once('?').expect("query").1.as_bytes())
            .find(|(key, _)| key == "filters")
            .expect("filters")
            .1
            .into_owned();
    let filters: Json = serde_json::from_str(&filters).expect("filters are JSON");
    assert_eq!(
        filters,
        json!({ "label": ["com.docker.compose.project=blog"], "status": ["running"] })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_asks_the_daemon_for_its_version_once() {
    let (daemon, host) = daemon().await;
    let (): () = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        assert(docker:ping())
        assert(docker:info())
        assert(docker:version().ApiVersion == "1.44")
        "#,
    )
    .await;
    let versions = daemon
        .seen
        .lock()
        .iter()
        .filter(|seen| seen.target.ends_with("/version"))
        .count();
    // one to negotiate, one for the script's own `version()`
    assert_eq!(versions, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inspect_is_a_table_with_nil_for_null_and_nil_for_no_such_container() {
    let (_daemon, host) = daemon().await;
    let (image, app, health, missing, err): (String, String, Value, Value, Value) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local web = assert(docker:inspect_container("web"))
        local ghost, err = docker:inspect_container("ghost")
        return web.Config.Image, web.Config.Labels.app, web.State.Health, ghost, err
        "#,
    )
    .await;
    assert_eq!((image.as_str(), app.as_str()), ("nginx:1.27", "web"));
    assert!(health.is_nil(), "a JSON null is nil, got {health:?}");
    assert!(
        missing.is_nil() && err.is_nil(),
        "no such container is (nil, nil)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_passes_dockers_body_through_and_takes_the_name_out() {
    let (daemon, host) = daemon().await;
    let id: String = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local made = assert(docker:create_container({
            name = "web",
            Image = "nginx:1.27",
            Env = { "A=1" },
            Cmd = {},
            Labels = { app = "web" },
            HostConfig = {
                Memory = 64 * 1024 * 1024,
                Binds = { "/srv/www:/usr/share/nginx/html:ro" },
                PortBindings = { ["80/tcp"] = { { HostPort = "8088" } } },
                RestartPolicy = { Name = "unless-stopped" },
            },
        }))
        return made.Id
        "#,
    )
    .await;
    assert_eq!(id, "ccc333");

    let seen = daemon.request_to("/containers/create");
    assert_eq!(seen.method, "POST");
    assert!(seen.target.contains("name=web"), "{}", seen.target);
    let body: Json = serde_json::from_str(&seen.body).expect("a JSON body");
    assert_eq!(body["Image"], "nginx:1.27");
    assert_eq!(body["Env"], json!(["A=1"]));
    assert_eq!(body["Labels"], json!({ "app": "web" }));
    assert_eq!(body["HostConfig"]["Memory"], json!(67_108_864));
    assert_eq!(
        body["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"],
        "8088"
    );
    assert_eq!(
        body["HostConfig"]["RestartPolicy"]["Name"],
        "unless-stopped"
    );
    assert!(
        body.get("name").is_none(),
        "the call's own `name` is not Docker's"
    );
    assert!(body.get("Cmd").is_none(), "an empty table is left out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_lifecycle_calls_send_their_options() {
    let (daemon, host) = daemon().await;
    let (): () = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        assert(docker:start_container("web") == true)
        assert(docker:stop_container("web", { timeout = 5 }))
        assert(docker:restart_container("web", { signal = "SIGINT" }))
        assert(docker:kill_container("web", { signal = "SIGHUP" }))
        assert(docker:remove_container("web", { force = true, volumes = true }))
        assert(docker:remove_network("backend"))
        assert(docker:stats("web").memory_stats.usage == 1024)
        assert(docker:list_images()[1].RepoTags[1] == "nginx:1.27")
        assert(docker:remove_image("nginx:1.27")[1].Untagged == "nginx:1.27")
        assert(docker:list_volumes().Volumes[1].Name == "data")
        assert(docker:create_volume({ Name = "data", Labels = { app = "web" } }).Name == "data")
        assert(docker:list_networks()[1].Name == "bridge")
        assert(docker:create_network({ Name = "backend", Driver = "bridge" }).Id == "n2")
        "#,
    )
    .await;
    assert!(
        daemon
            .request_to("/containers/web/stop")
            .target
            .contains("t=5")
    );
    assert!(
        daemon
            .request_to("/containers/web/restart")
            .target
            .contains("signal=SIGINT")
    );
    assert!(
        daemon
            .request_to("/containers/web/kill")
            .target
            .contains("signal=SIGHUP")
    );
    let removed = daemon.request_to("/containers/web");
    assert_eq!(removed.method, "DELETE");
    assert!(removed.target.contains("force=true") && removed.target.contains("v=true"));
    let volume: Json =
        serde_json::from_str(&daemon.request_to("/volumes/create").body).expect("JSON");
    assert_eq!(
        volume,
        json!({ "Name": "data", "Labels": { "app": "web" } })
    );
    let network: Json =
        serde_json::from_str(&daemon.request_to("/networks/create").body).expect("JSON");
    assert_eq!(network["Name"], "backend");
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dockers_refusals_keep_its_message_under_a_stable_kind() {
    let (_daemon, host) = daemon().await;
    let (not_found, conflict, in_use, kinds): (String, String, String, String) = run(
        &host,
        r#"
        local bollard = require("vnd:bollard")
        local docker = assert(bollard.connect({ host = ... }))
        local _, not_found = docker:start_container("ghost")
        local _, conflict = docker:remove_container("web")
        local _, in_use = docker:remove_volume("data")
        local kinds = table.concat({
            bollard.error_kind(not_found), bollard.error_kind(conflict), bollard.error_kind(in_use),
            tostring(bollard.error_kind("something else")), tostring(bollard.error_kind(nil)),
        }, ",")
        -- the message is the string after its kind, and only this module's strings have one
        for _, err in { not_found, conflict, in_use } do
            local message = assert(bollard.error_message(err), err)
            assert(message ~= "" and string.sub(err, -#message) == message, err)
        end
        assert(bollard.error_message("bollard.ping: connect: nothing listens") == "nothing listens")
        assert(bollard.error_message("bollard.ping: nonsense: x") == nil)
        assert(bollard.error_message("keyring.store: no_store: x") == nil)
        assert(bollard.error_message("something else") == nil and bollard.error_message(nil) == nil)
        return not_found, conflict, in_use, kinds
        "#,
    )
    .await;
    assert_eq!(
        not_found,
        "bollard.start_container: not_found: No such container: ghost"
    );
    assert_eq!(
        conflict,
        "bollard.remove_container: conflict: cannot remove container \"/web\": container is running"
    );
    assert_eq!(in_use, "bollard.remove_volume: conflict: volume is in use");
    assert_eq!(kinds, "not_found,conflict,conflict,nil,nil");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_with_no_daemon_is_a_connect_error() {
    // a port nothing listens on: bound, read, and closed again
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        format!("tcp://{}", listener.local_addr().expect("address"))
    };
    let err = error_of(
        &closed,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        return docker:list_containers()
        "#,
    )
    .await;
    assert!(
        err.starts_with("bollard.list_containers: connect: the Docker daemon could not be reached"),
        "{err}"
    );

    let err = error_of(
        "unix:///nowhere/at/all/docker.sock",
        r#"return require("vnd:bollard").connect({ host = ... })"#,
    )
    .await;
    assert!(
        err.starts_with("bollard.connect: connect: there is no Docker socket at"),
        "{err}"
    );
    assert!(err.contains("/nowhere/at/all/docker.sock"), "{err}");

    // a bare path is a socket path
    let err = error_of(
        "/nowhere/docker.sock",
        r#"return require("vnd:bollard").connect({ host = ... })"#,
    )
    .await;
    assert!(err.starts_with("bollard.connect: connect:"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_daemon_that_does_not_answer_times_out() {
    // accepts and never answers
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let host = format!("tcp://{}", listener.local_addr().expect("address"));
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            open.push(stream);
        }
    });
    let err = error_of(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ..., timeout = 0.2 }))
        return docker:ping()
        "#,
    )
    .await;
    assert!(err.starts_with("bollard.ping: timed_out:"), "{err}");
    held.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_cannot_be_sent_is_refused_before_anything_is_sent() {
    let (daemon, host) = daemon().await;
    let refused: Vec<String> = run(
        &host,
        r#"
        local bollard = require("vnd:bollard")
        local docker = assert(bollard.connect({ host = ... }))
        local refused = {}
        local function refuse(value, err)
            assert(value == nil, "accepted")
            assert(bollard.error_kind(err) == "invalid_input", tostring(err))
            table.insert(refused, err)
        end
        refuse(docker:start_container("../images/json"))
        refuse(docker:start_container(""))
        refuse(docker:inspect_container("web?size=1"))
        refuse(docker:list_containers({ al = true }))
        refuse(docker:list_containers({ all = "yes" }))
        refuse(docker:list_containers({ filters = { status = 1 } }))
        refuse(docker:stop_container("web", { signal = "TERM; rm" }))
        refuse(docker:logs("web", { tail = -1 }))
        refuse(docker:logs("web/..", {}))
        refuse(docker:pull_image("nginx?tag=x"))
        refuse(docker:remove_image("../containers/web"))
        refuse(docker:exec("web", {}))
        refuse(docker:exec("web", { "ls", 1 }))
        refuse(docker:create_container("nginx"))
        refuse(docker:create_container({ name = "a/b", Image = "nginx" }))
        refuse(docker:create_container({ Image = { "not", "a", "string" } }))
        refuse(docker:create_volume({ Name = true }))
        refuse(bollard.connect({ host = "tcp://127.0.0.1:1", tls_ca = "ca.pem" }))
        refuse(bollard.connect({ host = "tcp://127.0.0.1:1", timeout = -1 }))
        refuse(bollard.connect({ hots = "tcp://127.0.0.1:1" }))
        refuse(bollard.connect({ host = "gopher://127.0.0.1" }))
        return refused
        "#,
    )
    .await;
    assert!(
        refused[0]
            .starts_with("bollard.start_container: invalid_input: the container must be a name")
    );
    assert!(refused[3].contains("unknown option \"al\""), "{}", refused[3]);
    assert!(
        daemon.requests().is_empty(),
        "a refused call reached the daemon: {:?}",
        daemon.requests()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_buffer_is_refused_as_every_serde_conversion_refuses_it() {
    let (_daemon, host) = daemon().await;
    let message: String = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local ok, err = pcall(docker.create_container, docker, { Image = buffer.create(2) })
        assert(not ok)
        return tostring(err)
        "#,
    )
    .await;
    assert!(
        message.contains("bollard: a buffer cannot be serialized"),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// streams
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_are_pieces_by_stream_and_then_the_end() {
    let (daemon, host) = daemon().await;
    let (out, ended, missing): (String, bool, String) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local logs = assert(docker:logs("web", { tail = 2, timestamps = true }))
        local out = {}
        while true do
            local piece, err = logs:next()
            assert(err == nil, err)
            if piece == nil then break end
            table.insert(out, piece.stream .. "=" .. piece.data)
        end
        local again, again_err = logs:next()
        -- the request is the stream's: a container that does not exist is the first next()'s error
        local ghost = assert(docker:logs("ghost"))
        local _, missing = ghost:next()
        return table.concat(out), again == nil and again_err == nil, missing
        "#,
    )
    .await;
    assert_eq!(out, "stdout=listening on :80\nstderr=warn: no config\n");
    assert!(ended, "after the end, next() stays (nil, nil)");
    assert_eq!(missing, "bollard.logs: not_found: No such container: ghost");
    let seen = daemon.request_to("/containers/web/logs");
    for part in [
        "tail=2",
        "timestamps=true",
        "stdout=true",
        "stderr=true",
        "follow=false",
    ] {
        assert!(
            seen.target.contains(part),
            "{part} is not in {}",
            seen.target
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_arrive_as_they_happen_until_the_stream_is_closed() {
    let (daemon, host) = daemon().await;
    let (feed, held) = mpsc::channel(4);
    *daemon.held_open.lock() = Some(held);
    feed.send(Bytes::from(
        "{\"Type\":\"container\",\"Action\":\"start\",\"Actor\":{\"ID\":\"aaa111\",\"Attributes\":{\"name\":\"web\"}}}\n",
    ))
    .await
    .expect("feed");
    let (action, name, ended): (String, String, bool) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local events = assert(docker:events({ filters = { type = "container" } }))
        local event = assert(events:next())
        events:close()
        local after, err = events:next()
        return event.Action, event.Actor.Attributes.name, after == nil and err == nil
        "#,
    )
    .await;
    assert_eq!((action.as_str(), name.as_str()), ("start", "web"));
    assert!(ended, "a closed stream is at its end");
    // closing dropped the connection: the daemon's side of the body has nobody to send to
    tokio::time::timeout(TIMEOUT, feed.closed())
        .await
        .expect("the daemon saw the stream close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_stops_with_the_script_that_made_it() {
    let (daemon, host) = daemon().await;
    let (feed, held) = mpsc::channel(4);
    *daemon.held_open.lock() = Some(held);
    feed.send(Bytes::from(frame(1, "first\n")))
        .await
        .expect("feed");

    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let first: String = run_in(
        &lua,
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        following = assert(docker:logs("web", { follow = true }))
        return assert(following:next()).data
        "#,
    )
    .await;
    assert_eq!(first, "first\n");

    // the script still holds the stream (a global), and is shut down: the reading task goes with it
    life.shutdown();
    tokio::time::timeout(TIMEOUT, feed.closed())
        .await
        .expect("the daemon saw the stream close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pull_names_latest_reports_progress_and_fails_as_docker_says() {
    let (daemon, host) = daemon().await;
    let (lines, total, failure): (i64, i64, String) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local pull = assert(docker:pull_image("nginx"))
        local lines, total = 0, 0
        while true do
            local progress, err = pull:next()
            assert(err == nil, err)
            if progress == nil then break end
            lines += 1
            if progress.progressDetail and progress.progressDetail.total then
                total = progress.progressDetail.total
            end
        end
        local failing = assert(docker:pull_image("nope:1"))
        local failure
        while failure == nil do
            local progress, err = failing:next()
            assert(progress ~= nil or err ~= nil, "the failed pull ended without an error")
            failure = err
        end
        return lines, total, failure
        "#,
    )
    .await;
    assert_eq!((lines, total), (3, 10));
    assert_eq!(
        failure,
        "bollard.pull_image: api: pull access denied for nope"
    );
    let pulls: Vec<String> = daemon
        .requests()
        .into_iter()
        .filter(|seen| seen.target.contains("/images/create"))
        .map(|seen| seen.target)
        .collect();
    assert!(
        pulls[0].contains("fromImage=nginx") && pulls[0].contains("tag=latest"),
        "{pulls:?}"
    );
    assert!(
        pulls[1].contains("fromImage=nope") && pulls[1].contains("tag=1"),
        "{pulls:?}"
    );
}

// ---------------------------------------------------------------------------
// hostile input
// ---------------------------------------------------------------------------

/// Every function and method with values no caller should pass, one, two and three at a time: nothing panics and
/// nothing hangs. Each call ends in a value, a `(nil, err)` or a raised error; all three are fine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_panics_on_hostile_input() {
    let (_daemon, host) = daemon().await;
    let calls: i64 = tokio::time::timeout(
        Duration::from_secs(120),
        run(
            &host,
            r#"
            local bollard = require("vnd:bollard")
            local docker = assert(bollard.connect({ host = ..., timeout = 2 }))
            local self_ref = {}
            self_ref.me = self_ref
            local deep = {}
            do
                local at = deep
                for _ = 1, 300 do
                    at.next = {}
                    at = at.next
                end
            end
            local hostile = {
                true, 0, -1, 1.5, 2 ^ 63, 0 / 0, math.huge, "", "\0", "a\0b", "../..", "%2e%2e", "a b",
                string.rep("x", 100000), "\255\254", {}, { 1, 2 }, { [1] = "a", x = 1 }, self_ref, deep,
                { filters = { a = { b = { 1 } } } }, { signal = "\n" }, { tail = 0 / 0 }, { name = {} },
                function() end, coroutine.create(function() end), buffer.create(1),
                setmetatable({}, { __index = function() error("boom") end }),
            }
            local calls = 0
            local function try(f, ...)
                pcall(f, ...)
                calls += 1
            end
            for _, a in hostile do
                try(bollard.connect, a)
                try(bollard.error_kind, a)
                try(bollard.error_message, a)
            end
            local methods = {
                "ping", "version", "info", "list_containers", "inspect_container", "create_container",
                "start_container", "stop_container", "restart_container", "kill_container", "pause_container",
                "unpause_container", "remove_container", "logs", "stats", "exec", "list_images", "pull_image",
                "remove_image", "list_volumes", "create_volume", "remove_volume", "list_networks",
                "create_network", "remove_network", "events",
            }
            for _, name in methods do
                local method = docker[name]
                assert(method, name)
                try(method)
                try(method, docker)
                for _, a in hostile do
                    try(method, a)
                    local made = select(2, pcall(method, docker, a))
                    calls += 1
                    try(method, docker, "web", a)
                    try(method, docker, "web", { "ls" }, a)
                    -- a stream a hostile call did make is closed, not left reading
                    if type(made) == "userdata" then pcall(function() made:close() end) end
                end
            end
            return calls
            "#,
        ),
    )
    .await
    .expect("a hostile call hung");
    assert!(calls > 2000, "only {calls} calls were made");
}

// ---------------------------------------------------------------------------
// a real daemon
// ---------------------------------------------------------------------------

/// Against the Docker daemon of this machine: `cargo test -p neoc-vm bollard -- --ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a running Docker daemon"]
async fn a_real_daemon_answers() {
    let (version, containers): (String, i64) = run(
        "",
        r#"
        local docker = assert(require("vnd:bollard").connect())
        assert(docker:ping())
        local version = assert(docker:version())
        local containers = assert(docker:list_containers({ all = true }))
        assert(docker:list_images())
        assert(docker:list_volumes())
        assert(docker:list_networks())
        return version.Version, #containers
        "#,
    )
    .await;
    assert!(!version.is_empty());
    assert!(containers >= 0);
}
