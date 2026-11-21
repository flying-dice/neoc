//! What a fetch refuses: unbounded bodies and files, non-regular files, redirects off the origin, and
//! addresses inside the host's own network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers

use vos_application::{ports::Fetcher, PortError};
use vos_fetch::UrlFetcher;
use axum::{
    body::{Body, Bytes},
    response::Redirect,
    routing::get,
    Router,
};
use std::{convert::Infallible, net::SocketAddr, time::Duration};

/// Fails the test (instead of hanging it) when a fetch does not come back: the old fetcher read without
/// bound.
async fn quickly<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), work).await.expect("the fetch did not finish")
}

fn rejected(result: Result<Vec<u8>, PortError>) -> String {
    match result {
        Err(PortError::Rejected(why)) => why,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    addr
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fetch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_file_must_be_a_regular_file_within_the_cap() {
    let dir = temp_dir("files");
    std::fs::write(dir.join("small.lua"), "return 1").unwrap();
    std::fs::write(dir.join("big.lua"), vec![b'x'; 5000]).unwrap();
    let fifo = dir.join("pipe");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let f = UrlFetcher::new(1024).unwrap();
    let url = |p: &std::path::Path| format!("file://{}", p.display());

    assert_eq!(quickly(f.fetch(&url(&dir.join("small.lua")))).await.unwrap(), b"return 1");
    assert!(rejected(quickly(f.fetch(&url(&dir.join("big.lua")))).await).contains("larger than"));
    // a FIFO would block the host forever on open; a device would never end; a directory is not a file
    for not_a_file in [fifo, "/dev/zero".into(), "/dev/null".into(), dir.clone()] {
        let why = rejected(quickly(f.fetch(&url(&not_a_file))).await);
        assert!(why.contains("not a regular file"), "{not_a_file:?}: {why}");
    }
    assert!(matches!(quickly(f.fetch(&url(&dir.join("absent")))).await, Err(PortError::NotFound(_))));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_http_body_is_checked_by_content_length_and_never_read_past_the_cap() {
    let endless = futures_util::stream::repeat_with(|| Ok::<_, Infallible>(Bytes::from(vec![b'x'; 4096])));
    let addr = serve(
        Router::new()
            .route("/ok", get(|| async { "return 1" }))
            .route("/declared", get(|| async { vec![b'x'; 5000] }))
            .route("/endless", get(move || async move { Body::from_stream(endless) })),
    )
    .await;
    let f = UrlFetcher::new(1024).unwrap();
    assert_eq!(quickly(f.fetch(&format!("http://{addr}/ok"))).await.unwrap(), b"return 1");
    // a ref served from this machine may use loopback, by address or by name
    let by_name = format!("http://localhost:{}/ok", addr.port());
    assert_eq!(quickly(f.fetch(&by_name)).await.unwrap(), b"return 1");
    assert!(rejected(quickly(f.fetch(&format!("http://{addr}/declared"))).await).contains("larger than"));
    assert!(
        rejected(quickly(f.fetch(&format!("http://{addr}/endless"))).await).contains("larger than"),
        "a body with no length stops at the cap"
    );
}

#[tokio::test]
async fn a_redirect_is_followed_only_within_the_origin_and_only_so_far() {
    let elsewhere = serve(Router::new().route("/secret", get(|| async { "other origin" }))).await;
    let off_origin = format!("http://{elsewhere}/secret");
    let addr = serve(
        Router::new()
            .route("/ok", get(|| async { "return 1" }))
            .route("/same", get(|| async { Redirect::temporary("/ok") }))
            .route("/out", get(move || async move { Redirect::temporary(&off_origin) }))
            .route("/loop", get(|| async { Redirect::temporary("/loop") })),
    )
    .await;
    let f = UrlFetcher::new(1024).unwrap();
    assert_eq!(quickly(f.fetch(&format!("http://{addr}/same"))).await.unwrap(), b"return 1");
    let why = rejected(quickly(f.fetch(&format!("http://{addr}/out"))).await);
    assert!(why.contains("leaves the origin"), "{why}");
    let why = rejected(quickly(f.fetch(&format!("http://{addr}/loop"))).await);
    assert!(why.contains("too many redirects"), "{why}");
}

#[tokio::test]
async fn a_remote_ref_cannot_reach_private_link_local_or_metadata_addresses() {
    let f = UrlFetcher::new(1024).unwrap();
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/app.lua",
        "http://192.168.1.1/app.lua",
        "http://[fd00:ec2::254]/latest",
        "http://[::ffff:169.254.169.254]/latest",
        "http://0.0.0.0:7400/rpc",
    ] {
        // refused before any connection is tried, so this returns at once instead of timing out
        let why = rejected(quickly(f.fetch(url)).await);
        assert!(why.contains("not a public address"), "{url}: {why}");
    }
}
