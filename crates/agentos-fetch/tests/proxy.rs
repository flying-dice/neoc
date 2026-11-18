#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A proxy in the environment must not carry a fetch: it would resolve (and connect to) the target itself, past
//! the address guard. Kept in its own test binary because it sets process-wide environment variables.

use agentos_application::ports::Fetcher;
use agentos_fetch::UrlFetcher;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn a_proxy_in_the_environment_is_never_used() {
    let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_addr = target.local_addr().unwrap();
    let app = axum::Router::new().fallback(|| async { "direct" });
    tokio::spawn(async move { axum::serve(target, app).await });

    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = proxy.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            drop(stream);
        }
    });

    for var in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        std::env::set_var(var, format!("http://{proxy_addr}"));
    }
    for var in ["NO_PROXY", "no_proxy"] {
        std::env::remove_var(var);
    }

    let fetcher = UrlFetcher::new(1024).unwrap();
    let body = fetcher.fetch(&format!("http://{target_addr}/x")).await.unwrap();
    assert_eq!(body, b"direct");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the request went to the proxy, past the address guard");
}
