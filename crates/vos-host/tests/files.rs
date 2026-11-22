#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The Files page's host side, over the real adapters: the installed apps' Documents and This Computer (one shared
//! directory of the host's file system) through `host.files.*` on `/rpc`, and downloads and uploads over
//! `GET`/`PUT /files/...`, all behind the session guard and the Origin/Host guard.

mod common;

use common::{connect_rpc, pair, publish_package, rpc, start, temp, Running, Ws};
use serde_json::{json, Value};
use std::path::Path;
use vos_host::startup::prepare_data_dir;

/// A backend whose `seed` handler writes a few Documents (one of them a hostile page) and a cache file.
const BACKEND: &str = r#"
local docs = app.files.documents
return {
  seed = function()
    assert(docs:createDir("notes/2026"))
    assert(docs:write("notes/2026/todo.txt", "buy milk"))
    assert(docs:write("report.csv", "a,b\n1,2\n"))
    assert(docs:write("evil.html", "<script>alert(document.cookie)</script>"))
    assert(docs:write("été \"1\".txt", "x"))
    assert(app.files.cache:write("thumb.bin", "cache"))
    return true
  end,
}
"#;

/// A host with `notes` installed and seeded (and `empty` installed with no files), one paired browser, and its cookie.
/// This Computer is the whole machine; these tests keep to `<tmp>/home`, which is also the home directory it offers
/// (see [`home`] for its absolute path).
async fn host(tmp: &Path) -> (Running, Ws, String) {
    prepare_data_dir(&tmp.join("data")).unwrap();
    std::fs::create_dir_all(tmp.join("home/Documents")).unwrap();
    std::fs::write(tmp.join("home/Documents/letter.txt"), "Dear host").unwrap();
    std::fs::write(tmp.join("home/todo.md"), "- files").unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.home = Some(tmp.join("home").canonicalize().unwrap());
    })
    .await;
    for id in ["notes", "empty"] {
        let url =
            publish_package(&tmp.join(format!("pkg-{id}")), id, "app.luau", &[("app.luau", BACKEND.into())]);
        r.host.service.install_from_url(&url).await.unwrap();
    }
    let cookie = pair(&r, "Mac").await;
    let origin = format!("http://127.0.0.1:{}", r.addr.port());
    let mut ws =
        Ws { raw: connect_rpc(r.addr, Some(&cookie), Some(&origin)).await.unwrap(), backlog: vec![] };
    let seeded = rpc(&mut ws, 1, "app.call", json!({"app": "notes", "method": "seed"})).await;
    assert_eq!(seeded["result"], true, "{seeded}");
    (r, ws, cookie)
}

/// The absolute path of the test's home directory, as the wire has it (`/private/var/.../home`).
fn home(tmp: &Path) -> String {
    tmp.join("home").canonicalize().unwrap().to_str().unwrap().to_owned()
}

async fn get(r: &Running, path: &str, cookie: Option<&str>) -> reqwest::Response {
    let mut req = reqwest::Client::new().get(r.url(path));
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    req.send().await.unwrap()
}

fn names(list: &Value) -> Vec<&str> {
    list["result"]["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn an_apps_documents_are_listed_folder_by_folder_and_nothing_else_of_its_files() {
    let tmp = temp("files-list");
    let (r, mut ws, _) = host(&tmp).await;

    let top = rpc(&mut ws, 2, "host.files.list", json!({"app": "notes", "path": ""})).await;
    assert_eq!(names(&top), ["evil.html", "notes", "report.csv", "été \"1\".txt"], "sorted by name: {top}");
    let entries = top["result"]["entries"].as_array().unwrap();
    let report = entries.iter().find(|e| e["name"] == "report.csv").unwrap();
    assert_eq!((&report["kind"], &report["size"]), (&json!("file"), &json!(8)), "{report}");
    let modified = report["modifiedAt"].as_u64().expect("unix milliseconds");
    assert!(modified > 1_600_000_000_000, "{modified}");
    assert_eq!(entries.iter().find(|e| e["name"] == "notes").unwrap()["kind"], "dir");
    // an entry is a name, a kind, a size and a time: no path on the host
    assert!(!top.to_string().contains(tmp.to_str().unwrap()), "{top}");

    // without a path it is Documents itself; a folder is listed by its path
    assert_eq!(names(&rpc(&mut ws, 3, "host.files.list", json!({"app": "notes"})).await).len(), 4);
    let inner = rpc(&mut ws, 4, "host.files.list", json!({"app": "notes", "path": "notes/2026"})).await;
    assert_eq!(names(&inner), ["todo.txt"]);

    // the cache is the app's own business: no path reaches it
    assert!(!top.to_string().contains("thumb.bin"));
    for escape in ["../cache", "notes/../../cache", "/etc", "..", "notes\\2026"] {
        let refused = rpc(&mut ws, 5, "host.files.list", json!({"app": "notes", "path": escape})).await;
        assert_eq!(refused["error"]["code"], -32602, "{escape}: {refused}");
        assert_eq!(refused["error"]["data"]["kind"], "invalid_input", "{refused}");
    }

    // an app with no files has an empty Documents; an app that is not installed has none
    let empty = rpc(&mut ws, 6, "host.files.list", json!({"app": "empty", "path": ""})).await;
    assert_eq!(empty["result"], json!({"entries": [], "truncated": false}), "{empty}");
    let unknown = rpc(&mut ws, 7, "host.files.list", json!({"app": "nope", "path": ""})).await;
    assert_eq!(unknown["error"]["code"], -32002, "{unknown}");
    let missing = rpc(&mut ws, 8, "host.files.list", json!({"app": "notes", "path": "nowhere"})).await;
    assert_eq!(missing["error"]["code"], -32002, "{missing}");
    assert_eq!(missing["error"]["data"]["kind"], "not_found");
    assert!(!missing.to_string().contains(tmp.to_str().unwrap()), "no host path in an error: {missing}");
    let a_file = rpc(&mut ws, 9, "host.files.list", json!({"app": "notes", "path": "report.csv"})).await;
    assert_eq!(a_file["error"]["data"]["kind"], "not_a_directory", "{a_file}");

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn removing_an_entry_takes_a_file_or_a_whole_folder_and_never_documents_itself() {
    let tmp = temp("files-remove");
    let (r, mut ws, _) = host(&tmp).await;

    let removed = rpc(&mut ws, 2, "host.files.remove", json!({"app": "notes", "path": "report.csv"})).await;
    assert_eq!(removed.get("error"), None, "{removed}");
    assert_eq!(removed["result"], Value::Null);
    // a folder goes with what is in it
    // a folder that is not empty goes only when that is asked for
    let kept = rpc(&mut ws, 3, "host.files.remove", json!({"app": "notes", "path": "notes"})).await;
    assert_eq!(kept["error"]["data"]["kind"], "not_empty", "{kept}");
    let params = json!({"app": "notes", "path": "notes", "recursive": true});
    let folder = rpc(&mut ws, 3, "host.files.remove", params).await;
    assert_eq!(folder.get("error"), None, "{folder}");
    let left = rpc(&mut ws, 4, "host.files.list", json!({"app": "notes", "path": ""})).await;
    assert_eq!(names(&left), ["evil.html", "été \"1\".txt"]);

    let again = rpc(&mut ws, 5, "host.files.remove", json!({"app": "notes", "path": "report.csv"})).await;
    assert_eq!(again["error"]["code"], -32002, "{again}");
    for (path, why) in
        [("", "Documents itself"), (".", "Documents itself"), ("../cache/thumb.bin", "the cache")]
    {
        let refused = rpc(&mut ws, 6, "host.files.remove", json!({"app": "notes", "path": path})).await;
        assert_eq!(refused["error"]["code"], -32602, "{why}: {refused}");
    }
    let cache = tmp.join("data/apps/notes.files/cache/thumb.bin");
    assert!(cache.is_file(), "the cache was not touched");
    let unknown = rpc(&mut ws, 7, "host.files.remove", json!({"app": "nope", "path": "x"})).await;
    assert_eq!(unknown["error"]["code"], -32002, "{unknown}");

    // a new folder, and a rename, in the app's Documents; neither takes a name that is in use
    let made = rpc(&mut ws, 8, "host.files.createDir", json!({"app": "notes", "path": "archive"})).await;
    assert_eq!(made.get("error"), None, "{made}");
    let taken = rpc(&mut ws, 9, "host.files.createDir", json!({"app": "notes", "path": "archive"})).await;
    assert_eq!(
        (&taken["error"]["code"], &taken["error"]["data"]["kind"]),
        (&json!(-32004), &json!("already_exists"))
    );
    let params = json!({"app": "notes", "from": "evil.html", "to": "archive/harmless.txt"});
    let renamed = rpc(&mut ws, 10, "host.files.rename", params).await;
    assert_eq!(renamed.get("error"), None, "{renamed}");
    let inside = rpc(&mut ws, 11, "host.files.list", json!({"app": "notes", "path": "archive"})).await;
    assert_eq!(names(&inside), ["harmless.txt"]);

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn the_file_methods_take_exactly_their_params() {
    let tmp = temp("files-params");
    let (r, mut ws, _) = host(&tmp).await;
    let bad: Vec<(&str, Value)> = vec![
        ("host.files.list", json!({"app": "notes", "path": 3})),
        ("host.files.list", json!({"app": "notes", "path": "", "location": "cache"})),
        ("host.files.list", json!({"app": "Not An Id", "path": ""})),
        ("host.files.remove", json!({"app": "notes"})),
        ("host.files.remove", json!({"app": "notes", "path": "x", "recursive": "yes"})),
        ("host.files.remove", json!({"app": "notes", "path": "x", "force": true})),
        ("host.files.remove", json!({})),
        ("host.files.createDir", json!({"app": "notes"})),
        ("host.files.createDir", json!({"path": "x", "parents": true})),
        ("host.files.rename", json!({"from": "a"})),
        ("host.files.rename", json!({"from": "a", "to": "b", "replace": true})),
        ("host.files.locations", json!({"x": 1})),
    ];
    for (n, (method, params)) in (10_i64..).zip(bad) {
        let reply = rpc(&mut ws, n, method, params.clone()).await;
        assert_eq!(reply["error"]["code"], -32602, "{method} {params}: {reply}");
    }
    let unknown = rpc(&mut ws, 40, "host.files.read", json!({"app": "notes", "path": "report.csv"})).await;
    assert_eq!(unknown["error"]["code"], -32601, "bytes travel over HTTP, not the socket: {unknown}");
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn a_file_downloads_as_an_attachment_that_can_never_run_as_a_page_of_the_host() {
    let tmp = temp("files-download");
    let (r, _ws, cookie) = host(&tmp).await;

    let res = get(&r, "/files/app/notes/notes/2026/todo.txt", Some(&cookie)).await;
    assert_eq!(res.status(), 200);
    let header = |res: &reqwest::Response, name: &str| res.headers()[name].to_str().unwrap().to_owned();
    assert_eq!(
        header(&res, "content-disposition"),
        "attachment; filename=\"todo.txt\"; filename*=UTF-8''todo.txt"
    );
    assert_eq!(header(&res, "x-content-type-options"), "nosniff");
    assert_eq!(header(&res, "content-type"), "application/octet-stream");
    assert_eq!(header(&res, "cache-control"), "no-store");
    assert_eq!(res.text().await.unwrap(), "buy milk");

    // a page an app wrote is bytes to save like any other file: not HTML, not sniffed, and sandboxed if drawn anyway
    let evil = get(&r, "/files/app/notes/evil.html", Some(&cookie)).await;
    assert_eq!(evil.status(), 200);
    assert!(header(&evil, "content-disposition").starts_with("attachment;"));
    assert_eq!(header(&evil, "content-type"), "application/octet-stream");
    assert_eq!(header(&evil, "x-content-type-options"), "nosniff");
    assert_eq!(header(&evil, "content-security-policy"), "sandbox; default-src 'none'");

    // a name with spaces, quotes and accents: the path is percent-encoded, the header is safe
    let named = get(&r, "/files/app/notes/%C3%A9t%C3%A9%20%221%22.txt", Some(&cookie)).await;
    assert_eq!(named.status(), 200);
    assert_eq!(
        header(&named, "content-disposition"),
        "attachment; filename=\"_t_ _1_.txt\"; filename*=UTF-8''%C3%A9t%C3%A9%20%221%22.txt"
    );

    for (path, status) in [
        ("/files/app/notes/nowhere.txt", 404),
        ("/files/app/notes/notes", 400), // a folder is not a file
        ("/files/app/nope/report.csv", 404),
        ("/files/app/Not%20An%20Id/report.csv", 404),
        ("/files/app/notes/%2E%2E%2Fcache%2Fthumb.bin", 400), // an encoded `../cache/thumb.bin`
        ("/files/app/notes/notes%5C2026", 400),
        ("/files/app/notes/%2Fetc%2Fpasswd", 400),
    ] {
        let res = get(&r, path, Some(&cookie)).await;
        assert_eq!(res.status(), status, "{path}");
        assert_eq!(header(&res, "x-content-type-options"), "nosniff", "{path}");
        let body = res.text().await.unwrap();
        assert!(!body.contains(tmp.to_str().unwrap()) && !body.contains("cache"), "{path}: {body}");
    }
    let listing = get(&r, "/files/app/notes", Some(&cookie)).await.status();
    assert!(listing.is_client_error(), "there is no listing over HTTP: {listing}");

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn the_download_route_needs_a_paired_session_like_rpc_and_an_origin_of_the_host() {
    let tmp = temp("files-guard");
    let (r, mut ws, cookie) = host(&tmp).await;
    let path = "/files/app/notes/report.csv";
    let host_path = &format!("/files/host{}/todo.md", home(&tmp));

    // no cookie, a made-up one, a bearer token: the session guard answers, from loopback too
    let unpaired = get(&r, path, None).await;
    assert_eq!(unpaired.status(), 401);
    assert_eq!(unpaired.json::<Value>().await.unwrap()["error"], "pairing required");
    assert_eq!(get(&r, path, Some("vos_session=made-up")).await.status(), 401);
    let token = cookie.strip_prefix("vos_session=").unwrap();
    let bearer = reqwest::Client::new().get(r.url(path)).bearer_auth(token).send().await.unwrap();
    assert_eq!(bearer.status(), 401, "a bearer header never authenticates a client route");
    // a file that does not exist is not told apart from one that does without a session
    assert_eq!(get(&r, "/files/app/notes/nowhere.txt", None).await.status(), 401);
    assert_eq!(get(&r, host_path, None).await.status(), 401);
    assert_eq!(get(&r, host_path, Some("vos_session=made-up")).await.status(), 401);

    // another site cannot read it with the person's cookie: the origin guard answers first
    let foreign = reqwest::Client::new()
        .get(r.url(path))
        .header("cookie", &cookie)
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    let sandboxed = reqwest::Client::new()
        .get(r.url(path))
        .header("cookie", &cookie)
        .header("origin", "null")
        .send()
        .await
        .unwrap();
    assert_eq!(sandboxed.status(), 403, "an app's sandboxed frame is not let through here");

    // a revoked device's cookie stops working
    assert_eq!(get(&r, path, Some(&cookie)).await.status(), 200);
    assert_eq!(get(&r, host_path, Some(&cookie)).await.status(), 200);
    let me = rpc(&mut ws, 2, "host.devices.list", json!({})).await["result"]["devices"][0]["id"].clone();
    rpc(&mut ws, 3, "host.devices.revoke", json!({"id": me})).await;
    assert_eq!(get(&r, path, Some(&cookie)).await.status(), 401);
    assert_eq!(get(&r, host_path, Some(&cookie)).await.status(), 401);

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

// ---- This Computer ------------------------------------------------------------------------------------------

#[tokio::test]
async fn this_computer_is_listed_changed_and_downloaded_by_absolute_path_with_the_same_methods() {
    let tmp = temp("files-host");
    let (r, mut ws, cookie) = host(&tmp).await;
    let home = home(&tmp);
    let at = |inner: &str| format!("{home}/{inner}");

    let locations = rpc(&mut ws, 2, "host.files.locations", json!({})).await;
    assert_eq!(
        locations["result"],
        json!({"computer": true, "home": home, "maxUploadBytes": 64 * 1024 * 1024}),
        "{locations}"
    );

    // without `app` the place is the machine: the top of its file system, or any absolute path
    let top = rpc(&mut ws, 3, "host.files.list", json!({})).await;
    assert!(!top["result"]["entries"].as_array().unwrap().is_empty(), "{top}");
    assert_eq!(top["result"]["truncated"], false);
    let listed = rpc(&mut ws, 4, "host.files.list", json!({"path": home})).await;
    assert_eq!(names(&listed), ["Documents", "todo.md"], "{listed}");
    let todo = &listed["result"]["entries"][1];
    assert_eq!((&todo["kind"], &todo["size"], &todo["link"]), (&json!("file"), &json!(7), &json!(false)));
    let inner = rpc(&mut ws, 5, "host.files.list", json!({"path": at("Documents/../Documents")})).await;
    assert_eq!(names(&inner), ["letter.txt"], "`..` steps up, as in any path");
    // a path here is absolute: a relative one is refused, not guessed at
    let relative = rpc(&mut ws, 6, "host.files.list", json!({"path": "Documents"})).await;
    assert_eq!(relative["error"]["code"], -32602, "{relative}");

    let made = rpc(&mut ws, 7, "host.files.createDir", json!({"path": at("Documents/Scans")})).await;
    assert_eq!(made.get("error"), None, "{made}");
    assert!(tmp.join("home/Documents/Scans").is_dir());
    let again = rpc(&mut ws, 8, "host.files.createDir", json!({"path": at("Documents/Scans")})).await;
    assert_eq!(again["error"]["data"]["kind"], "already_exists", "{again}");
    let params = json!({"from": at("todo.md"), "to": at("Documents/Scans/done.md")});
    assert_eq!(rpc(&mut ws, 9, "host.files.rename", params).await.get("error"), None);
    assert_eq!(std::fs::read_to_string(tmp.join("home/Documents/Scans/done.md")).unwrap(), "- files");

    let res = get(&r, &format!("/files/host{home}/Documents/letter.txt"), Some(&cookie)).await;
    assert_eq!(res.status(), 200);
    let disposition = res.headers()["content-disposition"].to_str().unwrap().to_owned();
    assert!(disposition.starts_with("attachment; filename=\"letter.txt\""), "{disposition}");
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert_eq!(res.headers()["content-type"], "application/octet-stream");
    assert_eq!(res.headers()["content-security-policy"], "sandbox; default-src 'none'");
    assert_eq!(res.text().await.unwrap(), "Dear host");
    assert_eq!(get(&r, &format!("/files/host{home}/nowhere.txt"), Some(&cookie)).await.status(), 404);
    assert_eq!(get(&r, &format!("/files/host{home}/Documents"), Some(&cookie)).await.status(), 400);

    // a folder with something in it is not removed until that is asked for by name
    let kept = rpc(&mut ws, 10, "host.files.remove", json!({"path": at("Documents/Scans")})).await;
    assert_eq!(kept["error"]["data"]["kind"], "not_empty", "{kept}");
    assert!(tmp.join("home/Documents/Scans/done.md").is_file());
    let params = json!({"path": at("Documents/Scans"), "recursive": true});
    assert_eq!(rpc(&mut ws, 11, "host.files.remove", params).await.get("error"), None);
    assert!(!tmp.join("home/Documents/Scans").exists());
    for top in ["", "/"] {
        let refused = rpc(&mut ws, 12, "host.files.remove", json!({"path": top, "recursive": true})).await;
        assert_eq!(refused["error"]["code"], -32602, "the top of the file system stays: {refused}");
    }

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[cfg(unix)]
#[tokio::test]
async fn links_are_followed_and_what_the_operating_system_refuses_is_said_about_that_entry() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let tmp = temp("files-links");
    let (r, mut ws, cookie) = host(&tmp).await;
    let home = home(&tmp);
    let at = |inner: &str| format!("{home}/{inner}");
    let elsewhere = tmp.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("far.txt"), "far away").unwrap();
    symlink(&elsewhere, tmp.join("home/shortcut")).unwrap();
    symlink(tmp.join("home/gone"), tmp.join("home/broken")).unwrap();
    let locked = tmp.join("home/locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let listed = rpc(&mut ws, 2, "host.files.list", json!({"path": home})).await;
    assert_eq!(names(&listed), ["Documents", "broken", "locked", "shortcut", "todo.md"], "{listed}");
    let entry = |name: &str| {
        listed["result"]["entries"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap().clone()
    };
    assert_eq!((&entry("shortcut")["kind"], &entry("shortcut")["link"]), (&json!("dir"), &json!(true)));
    assert_eq!((&entry("broken")["kind"], &entry("broken")["link"]), (&json!("link"), &json!(true)));
    assert!(entry("broken")["problem"].as_str().unwrap().contains("not there"));
    assert_eq!(entry("Documents").get("problem"), None);

    // a linked folder is opened like any folder, and its files download
    let through = rpc(&mut ws, 3, "host.files.list", json!({"path": at("shortcut")})).await;
    assert_eq!(names(&through), ["far.txt"]);
    let far = get(&r, &format!("/files/host{home}/shortcut/far.txt"), Some(&cookie)).await;
    assert_eq!(far.text().await.unwrap(), "far away");
    assert_eq!(get(&r, &format!("/files/host{home}/broken"), Some(&cookie)).await.status(), 404);

    // no permission: the answer says so, for that folder; the folder around it listed fine
    if std::fs::read_dir(&locked).is_err() {
        let refused = rpc(&mut ws, 4, "host.files.list", json!({"path": at("locked")})).await;
        assert_eq!(refused["error"]["data"]["kind"], "permission_denied", "{refused}");
        assert_eq!(refused["error"]["message"], "locked: You do not have permission to open this folder.");
        let put = reqwest::Client::new()
            .put(r.url(&format!("/files/host{home}/locked/new.txt")))
            .header("cookie", &cookie)
            .body("x")
            .send()
            .await
            .unwrap();
        assert_eq!(put.status(), 403);
        assert!(put.text().await.unwrap().contains("permission to save a file here"));
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();

    // removing a link removes the link, never what it leads to
    let params = json!({"path": at("shortcut"), "recursive": true});
    assert_eq!(rpc(&mut ws, 5, "host.files.remove", params).await.get("error"), None);
    assert!(elsewhere.join("far.txt").is_file());

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn a_directory_with_more_entries_than_the_cap_is_cut_off_with_a_note() {
    let tmp = temp("files-cap");
    let (r, mut ws, _) = host(&tmp).await;
    let cap = vos_domain::limits::MAX_LIST_ENTRIES;
    let many = tmp.join("home/many");
    std::fs::create_dir(&many).unwrap();
    for n in 0..cap + 7 {
        std::fs::write(many.join(format!("f{n:05}")), "").unwrap();
    }
    let listed = rpc(&mut ws, 2, "host.files.list", json!({"path": format!("{}/many", home(&tmp))})).await;
    assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), cap, "capped");
    assert_eq!(listed["result"]["truncated"], true);
    let small = rpc(&mut ws, 3, "host.files.list", json!({"path": home(&tmp)})).await;
    assert_eq!(small["result"]["truncated"], false);
    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn a_host_with_its_file_system_turned_off_offers_only_the_apps() {
    let tmp = temp("files-off");
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.host_files = false;
        c.home = Some(std::env::temp_dir());
    })
    .await;
    let cookie = pair(&r, "Mac").await;
    let mut ws = Ws { raw: connect_rpc(r.addr, Some(&cookie), None).await.unwrap(), backlog: vec![] };

    let locations = rpc(&mut ws, 1, "host.files.locations", json!({})).await;
    assert_eq!(locations["result"]["computer"], false, "{locations}");
    assert_eq!(locations["result"].get("home"), None, "{locations}");
    for (method, params) in [
        ("host.files.list", json!({})),
        ("host.files.list", json!({"path": "/"})),
        ("host.files.createDir", json!({"path": "/tmp/vos-should-not-exist"})),
        ("host.files.remove", json!({"path": "/tmp/vos-should-not-exist"})),
    ] {
        let refused = rpc(&mut ws, 2, method, params).await;
        assert_eq!(refused["error"]["data"]["kind"], "unsupported", "{method}: {refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains("host_files"), "{refused}");
    }
    assert_eq!(get(&r, "/files/host/etc/hosts", Some(&cookie)).await.status(), 404);
    let put = reqwest::Client::new().put(r.url("/files/host/tmp/x.txt")).header("cookie", &cookie).body("x");
    assert_eq!(put.send().await.unwrap().status(), 404);

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

// ---- uploads ------------------------------------------------------------------------------------------------

async fn put(r: &Running, path: &str, cookie: &str, body: Vec<u8>) -> reqwest::Response {
    reqwest::Client::new().put(r.url(path)).header("cookie", cookie).body(body).send().await.unwrap()
}

#[tokio::test]
async fn an_upload_lands_whole_in_the_folder_and_never_replaces_a_file_unasked() {
    let tmp = temp("files-upload");
    let (r, mut ws, cookie) = host(&tmp).await;
    let home = home(&tmp);
    let bytes: Vec<u8> = (0..=255).cycle().take(300_000).collect(); // not text: every byte value

    // to This Computer
    let saved = put(&r, &format!("/files/host{home}/Documents/photo%201.bin"), &cookie, bytes.clone()).await;
    assert_eq!(saved.status(), 201, "{:?}", saved.text().await);
    assert_eq!(std::fs::read(tmp.join("home/Documents/photo 1.bin")).unwrap(), bytes);
    let listed = rpc(&mut ws, 2, "host.files.list", json!({"path": format!("{home}/Documents")})).await;
    assert_eq!(
        names(&listed),
        ["letter.txt", "photo 1.bin"],
        "nothing temporary is left beside it: {listed}"
    );
    let back = get(&r, &format!("/files/host{home}/Documents/photo%201.bin"), Some(&cookie)).await;
    assert_eq!(back.bytes().await.unwrap().to_vec(), bytes, "it downloads back the same");

    // a file that is there is kept, and the answer says so; `replace=true` replaces it
    let kept =
        put(&r, &format!("/files/host{home}/Documents/letter.txt"), &cookie, b"replaced".to_vec()).await;
    assert_eq!(kept.status(), 409);
    assert_eq!(kept.headers()["x-content-type-options"], "nosniff");
    assert!(kept.text().await.unwrap().contains("letter.txt"));
    assert_eq!(std::fs::read_to_string(tmp.join("home/Documents/letter.txt")).unwrap(), "Dear host");
    let replaced = put(
        &r,
        &format!("/files/host{home}/Documents/letter.txt?replace=true"),
        &cookie,
        b"replaced".to_vec(),
    )
    .await;
    assert_eq!(replaced.status(), 201);
    assert_eq!(std::fs::read_to_string(tmp.join("home/Documents/letter.txt")).unwrap(), "replaced");

    // to an app's Documents, where the app then reads it like any file of its own
    let to_app = put(&r, "/files/app/notes/notes/2026/from-me.txt", &cookie, b"hello app".to_vec()).await;
    assert_eq!(to_app.status(), 201);
    assert_eq!(
        std::fs::read_to_string(tmp.join("data/apps/notes.files/documents/notes/2026/from-me.txt")).unwrap(),
        "hello app"
    );
    assert_eq!(put(&r, "/files/app/notes/report.csv", &cookie, b"x".to_vec()).await.status(), 409);

    // not onto a folder, not into a folder that is not there, not for an app that is not installed, no other option
    for (path, status) in [
        (format!("/files/host{home}/Documents"), 409),
        (format!("/files/host{home}/Documents?replace=true"), 400),
        (format!("/files/host{home}/Nowhere/a.txt"), 404),
        ("/files/app/nope/a.txt".to_owned(), 404),
        (format!("/files/host{home}/a.txt?overwrite=1"), 400),
    ] {
        assert_eq!(put(&r, &path, &cookie, b"x".to_vec()).await.status(), status, "{path}");
    }
    assert!(!tmp.join("home/Nowhere").exists() && !tmp.join("home/a.txt").exists());
    assert!(tmp.join("home/Documents").is_dir());

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn an_upload_over_the_limit_is_refused_and_leaves_nothing() {
    let tmp = temp("files-limit");
    let (r, _ws, cookie) = host(&tmp).await;
    let home = home(&tmp);
    let limit = vos_domain::limits::MAX_UPLOAD_BYTES;

    let over = put(&r, &format!("/files/host{home}/too-big.bin"), &cookie, vec![7; limit + 1]).await;
    assert_eq!(over.status(), 413);
    assert!(!tmp.join("home/too-big.bin").exists());
    let left: Vec<_> = std::fs::read_dir(tmp.join("home")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left.len(), 2, "nothing half-written is left: {left:?}");

    // exactly the limit is taken
    let at = put(&r, &format!("/files/host{home}/just-fits.bin"), &cookie, vec![7; limit]).await;
    assert_eq!(at.status(), 201);
    assert_eq!(std::fs::metadata(tmp.join("home/just-fits.bin")).unwrap().len(), limit as u64);

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}

#[tokio::test]
async fn an_upload_needs_a_paired_session_and_cannot_be_forged_from_another_origin() {
    let tmp = temp("files-upload-guard");
    let (r, _ws, cookie) = host(&tmp).await;
    let home = home(&tmp);
    let client = reqwest::Client::new();
    let own = format!("http://127.0.0.1:{}", r.addr.port());

    for path in [format!("/files/host{home}/forged.txt"), "/files/app/notes/forged.txt".to_owned()] {
        let path = path.as_str();
        let send = |cookie: Option<&str>, origin: Option<&str>| {
            let mut req = client.put(r.url(path)).body("forged");
            if let Some(c) = cookie {
                req = req.header("cookie", c);
            }
            if let Some(o) = origin {
                req = req.header("origin", o);
            }
            req.send()
        };
        // no session: the guard answers, from loopback and from the host's own origin too
        assert_eq!(send(None, None).await.unwrap().status(), 401, "{path}");
        assert_eq!(send(None, Some(&own)).await.unwrap().status(), 401, "{path}");
        assert_eq!(send(Some("vos_session=made-up"), Some(&own)).await.unwrap().status(), 401, "{path}");
        // another site, with the person's cookie (which a browser would not even send: it is SameSite=Strict)
        assert_eq!(send(Some(&cookie), Some("https://evil.example")).await.unwrap().status(), 403, "{path}");
        assert_eq!(
            send(Some(&cookie), Some("http://127.0.0.1:1")).await.unwrap().status(),
            403,
            "another port"
        );
        // a sandboxed frame (an app's page): its `Origin: null` is let through for reads of public files only
        assert_eq!(send(Some(&cookie), Some("null")).await.unwrap().status(), 403, "{path}");
        // a cross-origin PUT is preflighted, and no route answers the preflight with permission
        let preflight = client
            .request(reqwest::Method::OPTIONS, r.url(path))
            .header("origin", "https://evil.example")
            .header("access-control-request-method", "PUT")
            .send()
            .await
            .unwrap();
        assert!(preflight.status().is_client_error(), "{path}: {}", preflight.status());
        assert!(!preflight.headers().contains_key("access-control-allow-origin"));
    }
    assert!(!tmp.join("home/forged.txt").exists());
    assert!(!tmp.join("data/apps/notes.files/documents/forged.txt").exists());

    // the shell's own page, paired: taken
    let ok = client
        .put(r.url(&format!("/files/host{home}/mine.txt")))
        .header("cookie", &cookie)
        .header("origin", &own)
        .body("mine");
    assert_eq!(ok.send().await.unwrap().status(), 201);

    r.stop().await;
    let _ = std::fs::remove_dir_all(tmp);
}
