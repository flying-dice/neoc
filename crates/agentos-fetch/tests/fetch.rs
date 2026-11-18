#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use agentos_application::{ports::Fetcher, PortError};
use agentos_fetch::UrlFetcher;

#[tokio::test]
async fn file_and_http_load_identically_and_limits_apply() {
    let dir = std::env::temp_dir().join(format!("fetch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.luau"), "return 1").unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().fallback_service(tower_http::services::ServeDir::new(&dir));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let f = UrlFetcher::new(1024).unwrap();
    let a = f.fetch(&format!("file://{}/main.luau", dir.display())).await.unwrap();
    let b = f.fetch(&format!("http://{addr}/main.luau")).await.unwrap();
    assert_eq!(a, b"return 1");
    assert_eq!(a, b);
    assert!(matches!(f.fetch(&format!("http://{addr}/nope")).await, Err(PortError::NotFound(_))));
    assert!(matches!(f.fetch("ftp://x/y").await, Err(PortError::Rejected(_))));
    assert!(matches!(
        UrlFetcher::new(3).unwrap().fetch(&format!("file://{}/main.luau", dir.display())).await,
        Err(PortError::Rejected(_))
    ));
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}
