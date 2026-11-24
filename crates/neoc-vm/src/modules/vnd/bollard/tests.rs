//! `vnd:bollard` against a fake Docker API in this process: no test here needs a Docker daemon, except the two at
//! the end, which are ignored unless asked for. The fake answers the endpoints the module calls with canned bodies in
//! Docker's shapes, and keeps every request it saw, so a test can hold the module to what it sent.

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
use mlua::Lua;
use parking_lot::Mutex;
use serde_json::{Value as Json, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::lifecycle::Life;
use crate::{make_engine, make_engine_in};

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
        "CreatedAt": "2024-05-01T10:00:00Z",
        "Labels": {},
        "Scope": "local",
        "Options": {},
    })
}

impl Daemon {
    /// A body that stays open: fed by the receiver a test put in `held_open`.
    fn held_open_body(&self) -> Response<Body> {
        let pieces = self.held_open.lock().take().expect("the test set no held-open body");
        let stream = futures::stream::unfold(pieces, |mut pieces| async {
            pieces.recv().await.map(|piece| (Ok::<_, Infallible>(Frame::data(piece)), pieces))
        });
        Response::new(BodyExt::boxed(StreamBody::new(stream)))
    }

    fn respond(&self, method: &Method, path: &str, query: &str) -> Response<Body> {
        match (method.as_str(), path) {
            ("GET", "/version") => {
                answer(200, &json!({ "Version": "25.0.3", "ApiVersion": "1.44", "Os": "linux" }))
            }
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
            ("POST", "/containers/create") => answer(201, &json!({ "Id": "ccc333", "Warnings": [] })),
            ("POST", "/containers/web/start") => full(304, ""),
            ("POST", "/containers/web/stop" | "/containers/web/restart" | "/containers/web/kill") => {
                full(204, "")
            }
            ("DELETE", "/containers/web") if query.contains("force=true") => full(204, ""),
            ("DELETE", "/containers/web") => {
                refusal(409, "cannot remove container \"/web\": container is running")
            }
            ("GET", "/containers/web/logs") if query.contains("follow=true") => self.held_open_body(),
            ("GET", "/containers/web/logs") => {
                let mut body = frame(1, "listening on :80\n");
                body.extend(frame(2, "warn: no config\n"));
                full(200, body)
            }
            ("GET", "/containers/ghost/logs") => refusal(404, "No such container: ghost"),
            ("GET", "/containers/web/stats") => {
                answer(200, &json!({ "name": "/web", "memory_stats": { "usage": 1024, "limit": 4096 } }))
            }
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
            ("GET", "/networks") => {
                answer(200, &json!([{ "Id": "n1", "Name": "bridge", "Driver": "bridge" }]))
            }
            ("POST", "/networks/create") => answer(201, &json!({ "Id": "n2", "Warning": "" })),
            ("DELETE", "/networks/backend") => full(204, ""),
            ("GET", "/events") => self.held_open_body(),
            _ => refusal(500, &format!("the fake daemon has no {method} {path}")),
        }
    }

    /// The requests seen so far, without the version negotiation.
    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().iter().filter(|seen| !seen.target.ends_with("/version")).cloned().collect()
    }

    /// The one request to `path` (given without the version prefix or the query).
    fn request_to(&self, path: &str) -> Seen {
        let matching: Vec<Seen> = self
            .requests()
            .into_iter()
            .filter(|seen| seen.target.split('?').next().is_some_and(|p| p.ends_with(path)))
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
                        let body = request.into_body().collect().await.expect("body").to_bytes();
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
    lua.load(script).call_async(host).await.unwrap_or_else(|e| panic!("the script failed: {e}"))
}

/// Run `script`, which returns the `(value, err)` of a failed call: the error's kind, its reason and its text.
async fn error_of(host: &str, script: &str) -> (String, String, String) {
    let wrapped = format!(
        r#"
        local value, err = (function(...) {script} end)(...)
        assert(value == nil, "the call succeeded")
        return err.kind, err.reason or "", tostring(err)
        "#
    );
    run(host, &wrapped).await
}

const TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// requests and answers
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_is_dockers_own_tables_and_the_options_are_its_query() {
    let (daemon, host) = daemon().await;
    let (names, project, count, state): (String, String, i64, String) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local list = assert(docker:list_containers({
            all = true,
            limit = 5,
            filters = { label = { "com.docker.compose.project=blog" }, status = { "running" } },
        }))
        -- what Docker did not send is nil, not a null
        assert(list[2].Status == nil and list[1].Ports == nil)
        return list[1].Names[1] .. "," .. list[2].Names[1], list[1].Labels["com.docker.compose.project"], #list,
            list[2].State
        "#,
    )
    .await;
    assert_eq!((names.as_str(), project.as_str(), count, state.as_str()), ("/web,/db", "blog", 2, "exited"));

    let seen = daemon.request_to("/containers/json");
    assert_eq!(seen.method, "GET");
    assert!(seen.target.contains("all=true"), "{}", seen.target);
    assert!(seen.target.contains("limit=5"), "{}", seen.target);
    let filters = url::form_urlencoded::parse(seen.target.split_once('?').expect("query").1.as_bytes())
        .find(|(key, _)| key == "filters")
        .expect("filters")
        .1
        .into_owned();
    let filters: Json = serde_json::from_str(&filters).expect("filters are JSON");
    assert_eq!(filters, json!({ "label": ["com.docker.compose.project=blog"], "status": ["running"] }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_asks_the_daemon_for_its_version_once() {
    let (daemon, host) = daemon().await;
    let (): () = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        assert(docker:ping() == true)
        assert(docker:list_images())
        assert(assert(docker:version()).ApiVersion == "1.44")
        "#,
    )
    .await;
    let versions = daemon.seen.lock().iter().filter(|seen| seen.target.ends_with("/version")).count();
    // one to negotiate, one for the script's own `version()`
    assert_eq!(versions, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_sends_dockers_body_and_the_name_beside_it() {
    let (daemon, host) = daemon().await;
    let id: String = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local made = assert(docker:create_container({
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
        }, { name = "web" }))
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
    assert_eq!(body["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"], "8088");
    assert_eq!(body["HostConfig"]["RestartPolicy"]["Name"], "unless-stopped");
    assert!(body.get("name").is_none(), "the call's own `name` is not Docker's");
    // the field is a list, so an empty table is an empty list (it is not left out)
    assert_eq!(body["Cmd"], json!([]));
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
        assert(assert(docker:list_images())[1].RepoTags[1] == "nginx:1.27")
        assert(assert(docker:remove_image("nginx:1.27"))[1].Untagged == "nginx:1.27")
        assert(assert(docker:create_network({ Name = "backend", Driver = "bridge" })).Id == "n2")
        "#,
    )
    .await;
    assert!(daemon.request_to("/containers/web/stop").target.contains("t=5"));
    assert!(daemon.request_to("/containers/web/restart").target.contains("signal=SIGINT"));
    assert!(daemon.request_to("/containers/web/kill").target.contains("signal=SIGHUP"));
    let removed = daemon.request_to("/containers/web");
    assert_eq!(removed.method, "DELETE");
    assert!(removed.target.contains("force=true") && removed.target.contains("v=true"));
    let network: Json = serde_json::from_str(&daemon.request_to("/networks/create").body).expect("JSON");
    assert_eq!(network["Name"], "backend");
    assert_eq!(network["Driver"], "bridge");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inspect_is_a_table_with_nil_for_null_and_nil_for_no_such_container() {
    let (_daemon, host) = daemon().await;
    let (image, app, health, missing, err): (String, String, mlua::Value, mlua::Value, mlua::Value) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local web = assert(docker:inspect_container("web"))
        assert(web.Id == "aaa111" and web.State.Running == true and web.State.Status == "running")
        local ghost, err = docker:inspect_container("ghost")
        return web.Config.Image, web.Config.Labels.app, web.State.Health, ghost, err
        "#,
    )
    .await;
    assert_eq!((image.as_str(), app.as_str()), ("nginx:1.27", "web"));
    assert!(health.is_nil(), "a JSON null is nil, got {health:?}");
    assert!(missing.is_nil() && err.is_nil(), "no such container is (nil, nil)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_a_containers_use_the_volumes_and_the_networks_are_read() {
    let (daemon, host) = daemon().await;
    let (): () = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local info = assert(docker:info())
        assert(info.Containers == 2 and info.ServerVersion == "25.0.3" and info.Images == nil)

        local stats = assert(docker:stats("web"))
        assert(stats.name == "/web" and stats.memory_stats.usage == 1024 and stats.memory_stats.limit == 4096)
        local none, no_stats = docker:stats("ghost")
        assert(none == nil and no_stats.kind == "api" and no_stats.op == "vnd:bollard.Client:stats")

        local listed = assert(docker:list_volumes({ filters = { label = { "app=web" } } }))
        assert(#listed.Volumes == 1 and #listed.Warnings == 0)
        local volume = listed.Volumes[1]
        assert(volume.Name == "data" and volume.Driver == "local" and volume.Scope == "local")
        assert(volume.Mountpoint == "/var/lib/docker/volumes/data/_data")
        -- a date is the text Docker sent
        assert(volume.CreatedAt == "2024-05-01T10:00:00Z", volume.CreatedAt)
        assert(next(volume.Labels) == nil and next(volume.Options) == nil and volume.UsageData == nil)
        assert(#assert(docker:list_volumes()).Volumes == 1)

        local made = assert(docker:create_volume({ Name = "data", Driver = "local", Labels = { app = "web" } }))
        assert(made.Name == "data" and made.CreatedAt == "2024-05-01T10:00:00Z")

        local networks = assert(docker:list_networks({ filters = { driver = { "bridge" } } }))
        assert(#networks == 1 and networks[1].Name == "bridge" and networks[1].Id == "n1")
        assert(networks[1].Created == nil)
        "#,
    )
    .await;
    let stats = daemon.request_to("/containers/web/stats");
    assert!(
        stats.target.contains("stream=false") && stats.target.contains("one-shot=true"),
        "{}",
        stats.target
    );
    let filters_of = |target: &str| -> Option<Json> {
        let query = target.split_once('?')?.1;
        let (_, filters) = url::form_urlencoded::parse(query.as_bytes()).find(|(key, _)| key == "filters")?;
        serde_json::from_str(&filters).ok()
    };
    let volumes: Vec<Seen> = daemon
        .requests()
        .into_iter()
        .filter(|seen| seen.target.split('?').next() == Some("/volumes"))
        .collect();
    assert_eq!(volumes.len(), 2, "{volumes:?}");
    assert_eq!(filters_of(&volumes[0].target), Some(json!({ "label": ["app=web"] })));
    assert_eq!(filters_of(&daemon.request_to("/networks").target), Some(json!({ "driver": ["bridge"] })));
    let created: Json = serde_json::from_str(&daemon.request_to("/volumes/create").body).expect("JSON");
    assert_eq!(created, json!({ "Name": "data", "Driver": "local", "Labels": { "app": "web" } }));
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
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local _, not_found = docker:start_container("ghost")
        local _, conflict = docker:remove_container("web")
        local _, in_use = docker:remove_volume("data")
        local _, other = docker:pause_container("web")
        assert(other.kind == "api" and other.status == 500, tostring(other))
        -- the message is Docker's sentence, and so is the reason
        for _, err in { not_found, conflict, in_use } do
            assert(err.message == err.reason and err.message ~= "")
        end
        assert(not_found.op == "vnd:bollard.Client:start_container")
        return tostring(not_found), tostring(conflict), tostring(in_use),
            table.concat({ not_found.kind, conflict.kind, in_use.kind }, ",")
        "#,
    )
    .await;
    assert_eq!(not_found, "vnd:bollard.Client:start_container: not_found: No such container: ghost");
    assert_eq!(
        conflict,
        "vnd:bollard.Client:remove_container: conflict: cannot remove container \"/web\": container is running"
    );
    assert_eq!(in_use, "vnd:bollard.Client:remove_volume: conflict: volume is in use");
    assert_eq!(kinds, "not_found,conflict,conflict");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_with_no_daemon_is_a_connect_error() {
    // a port nothing listens on: bound, read, and closed again
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        format!("tcp://{}", listener.local_addr().expect("address"))
    };
    let (kind, reason, text) = error_of(
        &closed,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        return docker:list_containers()
        "#,
    )
    .await;
    assert_eq!(kind, "connect");
    assert!(reason.starts_with("the Docker daemon could not be reached"), "{reason}");
    assert!(text.starts_with("vnd:bollard.Client:list_containers: connect: the Docker daemon"), "{text}");

    // a socket that is not there (where there are sockets: bollard has no `unix://` on Windows)
    let connect = r#"return require("vnd:bollard").connect({ host = ... })"#;
    let (kind, reason, text) = error_of("unix:///nowhere/at/all/docker.sock", connect).await;
    if cfg!(unix) {
        assert_eq!(kind, "connect");
        assert!(reason.starts_with("there is no Docker socket at"), "{reason}");
        assert!(reason.contains("/nowhere/at/all/docker.sock"), "{reason}");
        assert!(text.starts_with("vnd:bollard.connect: connect: there is no Docker socket at"), "{text}");
        // a bare path is a socket path
        assert_eq!(error_of("/nowhere/docker.sock", connect).await.0, "connect");
    } else {
        assert_eq!(kind, "invalid_input", "{text}");
    }
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
    let (kind, _, text) = error_of(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ..., timeout = 0.2 }))
        return docker:ping()
        "#,
    )
    .await;
    assert_eq!(kind, "timed_out");
    assert_eq!(text, "vnd:bollard.Client:ping: timed_out: the Docker daemon did not answer in time");
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
        -- a wrong argument is raised with its place; what only the call can tell is its `invalid_input`
        local function refuse(call)
            local ok, value, err = pcall(call)
            if ok then
                assert(value == nil, "accepted")
                assert(err.kind == "invalid_input", tostring(err))
                table.insert(refused, tostring(err))
            else
                assert(tostring(value):find("vnd:bollard.", 1, true), tostring(value))
                table.insert(refused, tostring(value))
            end
        end
        refuse(function() return docker:start_container("../images/json") end)
        refuse(function() return docker:start_container("") end)
        refuse(function() return docker:stop_container("web?size=1") end)
        refuse(function() return docker:list_containers({ al = true }) end)
        refuse(function() return docker:list_containers({ all = "yes" }) end)
        refuse(function() return docker:list_containers({ filters = { status = 1 } }) end)
        refuse(function() return docker:list_containers({ filters = { status = "running" } }) end)
        refuse(function() return docker:stop_container("web", { signal = "TERM; rm" }) end)
        refuse(function() return docker:logs("web", { tail = -1 }) end)
        refuse(function() return docker:logs("web/..", {}) end)
        refuse(function() return docker:pull_image("nginx?tag=x") end)
        refuse(function() return docker:remove_image("../containers/web") end)
        refuse(function() return docker:exec("web", {}) end)
        refuse(function() return docker:exec("web", { "ls", 1 }) end)
        refuse(function() return docker:create_container("nginx") end)
        refuse(function() return docker:create_container({ Image = "nginx" }, { name = "a/b" }) end)
        refuse(function() return docker:create_container({ Image = { "not", "a", "string" } }) end)
        refuse(function() return docker:create_container({ Image = "nginx", name = "web" }) end)
        refuse(function() return docker:create_container({ Image = "nginx", HostConfig = { Memroy = 1 } }) end)
        refuse(function() return docker:create_network({ Name = true }) end)
        refuse(function() return bollard.connect({ host = "tcp://127.0.0.1:1", tls = { ca = "ca.pem" } }) end)
        refuse(function() return bollard.connect({ host = "tcp://127.0.0.1:1", timeout = -1 }) end)
        refuse(function() return bollard.connect({ host = "tcp://127.0.0.1:1", timeout = 0 }) end)
        refuse(function() return bollard.connect({ hots = "tcp://127.0.0.1:1" }) end)
        refuse(function() return bollard.connect({ host = "gopher://127.0.0.1" }) end)
        refuse(function() return docker:inspect_container("web?size=1") end)
        refuse(function() return docker:stats("../info") end)
        refuse(function() return docker:create_volume({ Name = true }) end)
        refuse(function() return docker:create_volume({ Name = "data", Lables = {} }) end)
        refuse(function() return docker:list_volumes({ filters = { name = "data" } }) end)
        refuse(function() return docker:list_networks({ filter = { name = { "bridge" } } }) end)
        return refused
        "#,
    )
    .await;
    assert_eq!(refused.len(), 31);
    let place =
        "vnd:bollard.Client:start_container: argument #1 'name': the container must be a name or an id";
    assert!(refused[0].contains(place), "{}", refused[0]);
    assert!(refused[3].contains("argument #1 'opts': unknown field `al`"), "{}", refused[3]);
    assert!(refused[12].contains("invalid_input: the command must be a list of strings"), "{}", refused[12]);
    assert!(refused[17].contains("argument #1 'body': unknown field `name`"), "{}", refused[17]);
    assert!(refused[18].contains("HostConfig: unknown field `Memroy`"), "{}", refused[18]);
    assert!(
        refused[21].contains("vnd:bollard.connect: invalid_input: option \"timeout\""),
        "{}",
        refused[21]
    );
    let place =
        "vnd:bollard.Client:inspect_container: argument #1 'name': the container must be a name or an id";
    assert!(refused[25].contains(place), "{}", refused[25]);
    assert!(refused[28].contains("argument #1 'body': unknown field `Lables`"), "{}", refused[28]);
    assert!(refused[30].contains("argument #1 'opts': unknown field `filter`"), "{}", refused[30]);
    assert!(daemon.requests().is_empty(), "a refused call reached the daemon: {:?}", daemon.requests());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_buffer_is_not_a_field_of_a_body() {
    let (daemon, host) = daemon().await;
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
    assert!(message.contains("argument #1 'body': Image: expected a string, got buffer"), "{message}");
    assert!(daemon.requests().is_empty());
}

// ---------------------------------------------------------------------------
// streams
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_are_pieces_by_stream_and_then_the_end() {
    let (daemon, host) = daemon().await;
    let (out, iterated, ended, missing): (String, String, bool, String) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local logs = docker:logs("web", { tail = 2, timestamps = true })
        local out = {}
        while true do
            local piece, err = logs:next()
            assert(err == nil, tostring(err))
            if piece == nil then break end
            table.insert(out, piece.stream .. "=" .. piece.data)
        end
        local again, again_err = logs:next()
        local iterated = {}
        for piece in docker:logs("web"):items() do
            table.insert(iterated, piece.stream)
        end
        -- the request is the stream's: a container that does not exist is the first next()'s error
        local ghost = docker:logs("ghost")
        local none, missing = ghost:next()
        assert(none == nil and missing.kind == "not_found" and missing.reason == "No such container: ghost")
        -- and the error was the end
        local after, after_err = ghost:next()
        assert(after == nil and after_err == nil)
        return table.concat(out), table.concat(iterated, ","), again == nil and again_err == nil, tostring(missing)
        "#,
    )
    .await;
    assert_eq!(out, "stdout=listening on :80\nstderr=warn: no config\n");
    assert_eq!(iterated, "stdout,stderr");
    assert!(ended, "after the end, next() stays (nil, nil)");
    assert_eq!(missing, "vnd:bollard.Client:logs: not_found: No such container: ghost");
    let seen: Vec<String> = daemon
        .requests()
        .into_iter()
        .filter(|seen| seen.target.contains("/containers/web/logs"))
        .map(|seen| seen.target)
        .collect();
    for part in ["tail=2", "timestamps=true", "stdout=true", "stderr=true", "follow=false"] {
        assert!(seen[0].contains(part), "{part} is not in {}", seen[0]);
    }
    assert!(seen[1].contains("tail=all"), "{}", seen[1]);
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
    let (kind, action, name, ended): (String, String, String, bool) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local events = docker:events({ filters = { type = { "container" } } })
        local event = assert(events:next())
        events:close()
        local after, err = events:next()
        return event.Type, event.Action, event.Actor.Attributes.name, after == nil and err == nil
        "#,
    )
    .await;
    assert_eq!((kind.as_str(), action.as_str(), name.as_str()), ("container", "start", "web"));
    assert!(ended, "a closed stream is at its end");
    // closing dropped the connection: the daemon's side of the body has nobody to send to
    tokio::time::timeout(TIMEOUT, feed.closed()).await.expect("the daemon saw the stream close");
    let filters = daemon.request_to("/events").target;
    assert!(filters.contains("filters="), "{filters}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_stops_with_the_script_that_made_it() {
    let (daemon, host) = daemon().await;
    let (feed, held) = mpsc::channel(4);
    *daemon.held_open.lock() = Some(held);
    feed.send(Bytes::from(frame(1, "first\n"))).await.expect("feed");

    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let first: String = run_in(
        &lua,
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        following = docker:logs("web", { follow = true })
        return assert(following:next()).data
        "#,
    )
    .await;
    assert_eq!(first, "first\n");

    // the script still holds the stream (a global), and is shut down: the reading task goes with it
    life.shutdown();
    tokio::time::timeout(TIMEOUT, feed.closed()).await.expect("the daemon saw the stream close");
}

/// A stream nobody reads is read all the same, up to its queue: the request is made when the stream is, so `events`
/// misses nothing between the call and the first `next()`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_makes_its_request_when_it_is_made() {
    let (daemon, host) = daemon().await;
    let (_feed, held) = mpsc::channel::<Bytes>(4);
    *daemon.held_open.lock() = Some(held);
    let lua = make_engine().expect("engine");
    let (): () = run_in(
        &lua,
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        unread = docker:events()
        "#,
    )
    .await;
    tokio::time::timeout(TIMEOUT, async {
        while !daemon.requests().iter().any(|seen| seen.target.contains("/events")) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the events request was made without a next()");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pull_names_latest_reports_progress_and_fails_as_docker_says() {
    let (daemon, host) = daemon().await;
    let (lines, total, failure, kind): (i64, i64, String, String) = run(
        &host,
        r#"
        local docker = assert(require("vnd:bollard").connect({ host = ... }))
        local pull = docker:pull_image("nginx")
        local lines, total = 0, 0
        while true do
            local progress, err = pull:next()
            assert(err == nil, tostring(err))
            if progress == nil then break end
            lines += 1
            if progress.progressDetail and progress.progressDetail.total then
                total = progress.progressDetail.total
            end
        end
        local failing = docker:pull_image("nope:1")
        local failure
        while failure == nil do
            local progress, err = failing:next()
            assert(progress ~= nil or err ~= nil, "the failed pull ended without an error")
            failure = err
        end
        assert(failure.status == nil, "an error inside a stream has no status")
        -- iterating raises what next() returns
        local ok, raised = pcall(function()
            for _ in docker:pull_image("nope:1"):items() do end
        end)
        assert(not ok and tostring(raised):find("api: pull access denied for nope", 1, true), tostring(raised))
        return lines, total, tostring(failure), failure.kind
        "#,
    )
    .await;
    assert_eq!((lines, total), (3, 10));
    assert_eq!(kind, "api");
    assert_eq!(failure, "vnd:bollard.Client:pull_image: api: pull access denied for nope");
    let pulls: Vec<String> = daemon
        .requests()
        .into_iter()
        .filter(|seen| seen.target.contains("/images/create"))
        .map(|seen| seen.target)
        .collect();
    assert!(pulls[0].contains("fromImage=nginx") && pulls[0].contains("tag=latest"), "{pulls:?}");
    assert!(pulls[1].contains("fromImage=nope") && pulls[1].contains("tag=1"), "{pulls:?}");
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
                { HostConfig = self_ref }, { Labels = deep }, { Image = 0 / 0 },
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
                    try(method, docker, { Image = "nginx" }, a)
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
        local info = assert(docker:info())
        assert(type(info.ServerVersion) == "string" and info.ServerVersion == version.Version)
        assert(type(info.Containers) == "number" and type(info.OperatingSystem) == "string")
        -- a date in Docker's answer is its text
        assert(info.SystemTime:match("^%d%d%d%d%-%d%d%-%d%dT"), info.SystemTime)
        local volumes = assert(docker:list_volumes())
        assert(type(volumes.Volumes) == "table" and type(volumes.Warnings) == "table")
        local networks = assert(docker:list_networks())
        local bridge
        for _, network in networks do
            if network.Name == "bridge" then bridge = network end
        end
        assert(bridge ~= nil and bridge.Driver == "bridge" and bridge.Scope == "local")
        assert(bridge.Created:match("^%d%d%d%d%-%d%d%-%d%dT"), bridge.Created)
        assert(#assert(docker:list_networks({ filters = { name = { "neoc-vm-no-such-network" } } })) == 0)
        local ghost, no_ghost = docker:inspect_container("neoc-vm-bollard-no-such-container")
        assert(ghost == nil and no_ghost == nil, "no such container is (nil, nil)")
        return version.Version, #containers
        "#,
    )
    .await;
    assert!(!version.is_empty());
    assert!(containers >= 0);
}

/// A container's whole life against the Docker daemon of this machine: `cargo test -p neoc-vm bollard --
/// --ignored`. It pulls `alpine:3.19`, and removes the container, the volume and the network it makes
/// (`neoc-vm-bollard-*`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a running Docker daemon"]
async fn a_real_daemon_runs_a_container_from_pull_to_removal() {
    let (): () = tokio::time::timeout(
        Duration::from_secs(300),
        run(
            "",
            r#"
            local docker = assert(require("vnd:bollard").connect())
            local name, network, volume = "neoc-vm-bollard-test", "neoc-vm-bollard-net", "neoc-vm-bollard-vol"
            -- what an earlier run that failed left behind
            docker:remove_container(name, { force = true })
            docker:remove_network(network)
            docker:remove_volume(volume, { force = true })
            local date = "^%d%d%d%d%-%d%d%-%d%dT%d%d:%d%d:%d%d"

            local data = assert(docker:create_volume({ Name = volume, Labels = { ["neoc.test"] = "bollard" } }))
            assert(data.Name == volume and data.Driver == "local" and data.Scope == "local")
            assert(data.Labels["neoc.test"] == "bollard" and data.Mountpoint:find(volume, 1, true))
            assert(data.CreatedAt:match(date), data.CreatedAt)
            local same = assert(docker:create_volume({ Name = volume }))
            assert(same.Name == volume and same.CreatedAt == data.CreatedAt, "creating one that exists returns it")
            local labelled = assert(docker:list_volumes({ filters = { label = { "neoc.test=bollard" } } }))
            assert(#labelled.Volumes == 1 and labelled.Volumes[1].Name == volume)
            assert(labelled.Volumes[1].Mountpoint == data.Mountpoint and #labelled.Warnings == 0)
            local unknown_driver, refused = docker:create_volume({ Name = volume .. "-x", Driver = "neoc-no-such-driver" })
            assert(unknown_driver == nil and refused ~= nil, "a driver Docker does not have was accepted")

            local statuses = 0
            for progress in docker:pull_image("alpine:3.19"):items() do
                assert(type(progress.status) == "string")
                statuses += 1
            end
            assert(statuses > 0)
            local listed = assert(docker:list_images({ filters = { reference = { "alpine:3.19" } } }))
            assert(#listed == 1 and listed[1].RepoTags[1] == "alpine:3.19")

            local events = docker:events({ filters = { type = { "container" }, container = { name } } })
            local made = assert(docker:create_container({
                Image = "alpine:3.19",
                Cmd = { "sh", "-c", "echo out; echo err >&2; sleep 300" },
                Env = { "GREETING=hello" },
                Labels = { ["neoc.test"] = "bollard" },
                HostConfig = { Memory = 64 * 1024 * 1024, Binds = { volume .. ":/data" } },
            }, { name = name }))
            assert(#made.Id == 64)
            local again, in_use = docker:create_container({ Image = "alpine:3.19" }, { name = name })
            assert(again == nil and in_use.kind == "conflict", tostring(in_use))

            assert(docker:start_container(name))
            assert(docker:start_container(name), "starting a running container succeeds")
            local seen = {}
            while not (seen.create and seen.start) do
                local event = assert(events:next())
                seen[event.Action] = event.Actor.Attributes.name == name
            end
            events:close()

            local mine = assert(docker:list_containers({ filters = { label = { "neoc.test=bollard" } } }))
            assert(#mine == 1 and mine[1].Id == made.Id and mine[1].State == "running" and mine[1].Names[1] == "/" .. name)

            local inspected = assert(docker:inspect_container(name))
            assert(inspected.Id == made.Id and inspected.Name == "/" .. name)
            assert(inspected.State.Running == true and inspected.State.Status == "running")
            assert(inspected.Config.Image == "alpine:3.19" and inspected.Config.Labels["neoc.test"] == "bollard")
            assert(inspected.HostConfig.Memory == 64 * 1024 * 1024)
            assert(inspected.Mounts[1].Name == volume and inspected.Mounts[1].Destination == "/data")
            assert(inspected.Created:match(date), inspected.Created)
            assert(inspected.State.StartedAt:match(date), inspected.State.StartedAt)
            local in_use_volume = select(2, docker:remove_volume(volume))
            assert(in_use_volume.kind == "conflict", tostring(in_use_volume))

            local stats = assert(docker:stats(name))
            assert(stats.name == "/" .. name and stats.id == made.Id)
            assert(stats.memory_stats.usage > 0 and stats.memory_stats.limit == 64 * 1024 * 1024)
            assert(stats.cpu_stats.cpu_usage.total_usage >= 0)
            assert(stats.read:match(date), stats.read)

            local ran = assert(docker:exec(name, { "sh", "-c", "echo $GREETING $EXTRA; echo oops >&2; exit 3" }, {
                env = { "EXTRA=world" },
            }))
            assert(ran.exit_code == 3 and ran.stdout == "hello world\n" and ran.stderr == "oops\n")

            local out = {}
            for piece in docker:logs(name):items() do
                out[piece.stream] = (out[piece.stream] or "") .. piece.data
            end
            assert(out.stdout == "out\n" and out.stderr == "err\n")

            assert(docker:pause_container(name))
            assert(docker:unpause_container(name))
            local _, running = docker:remove_container(name)
            assert(running.kind == "conflict", tostring(running))
            assert(docker:kill_container(name, { signal = "SIGKILL" }))
            assert(docker:stop_container(name, { timeout = 1 }), "stopping a stopped container succeeds")
            assert(docker:remove_container(name, { volumes = true }))
            local _, gone = docker:start_container(name)
            assert(gone.kind == "not_found", tostring(gone))
            local _, no_logs = docker:logs(name):next()
            assert(no_logs.kind == "not_found", tostring(no_logs))
            local gone_container, no_inspect = docker:inspect_container(name)
            assert(gone_container == nil and no_inspect == nil, "no such container is (nil, nil)")
            local _, no_stats = docker:stats(name)
            assert(no_stats.kind == "not_found", tostring(no_stats))
            assert(docker:remove_volume(volume))
            assert(#assert(docker:list_volumes({ filters = { name = { volume } } })).Volumes == 0)

            local net = assert(docker:create_network({ Name = network, Driver = "bridge", Labels = { ["neoc.test"] = "bollard" } }))
            assert(#net.Id > 0)
            local nets = assert(docker:list_networks({ filters = { name = { network } } }))
            assert(#nets == 1 and nets[1].Id == net.Id and nets[1].Name == network and nets[1].Driver == "bridge")
            assert(nets[1].Labels["neoc.test"] == "bollard" and nets[1].Created:match(date), nets[1].Created)
            assert(docker:remove_network(network))
            local _, no_network = docker:remove_network(network)
            assert(no_network.kind == "not_found", tostring(no_network))
            local _, no_volume = docker:remove_volume("neoc-vm-bollard-no-such-volume")
            assert(no_volume.kind == "not_found", tostring(no_volume))
            local failing, no_image = docker:pull_image("neoc-no-such-image:0.0.0"), nil
            while no_image == nil do
                local progress, err = failing:next()
                assert(progress ~= nil or err ~= nil, "the failed pull ended without an error")
                no_image = err
            end
            assert(no_image.kind == "not_found" or no_image.kind == "api", tostring(no_image))
            "#,
        ),
    )
    .await
    .expect("the daemon answered in time");
}
