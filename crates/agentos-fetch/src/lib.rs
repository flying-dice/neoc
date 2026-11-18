//! Driven adapter: the `Fetcher` port over `file://`, `http://` and `https://` (reqwest with rustls,
//! `tokio::fs` for files). App refs and every file they list load through it, with a size cap.

use agentos_application::{
    ports::{Fetcher, PortResult},
    PortError,
};
use async_trait::async_trait;
use std::time::Duration;

/// One `Fetcher` for all three schemes; the app ref and every file it lists load identically.
pub struct UrlFetcher {
    http: reqwest::Client,
    max_bytes: u64,
}

impl UrlFetcher {
    pub fn new(max_bytes: u64) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        Ok(Self { http, max_bytes })
    }
}

#[async_trait]
impl Fetcher for UrlFetcher {
    async fn fetch(&self, url: &str) -> PortResult<Vec<u8>> {
        let bytes = if let Some(path) = url.strip_prefix("file://") {
            tokio::fs::read(path).await.map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => PortError::NotFound(url.to_owned()),
                _ => PortError::Rejected(format!("{url}: {e}")),
            })?
        } else if url.starts_with("http://") || url.starts_with("https://") {
            let res =
                self.http.get(url).send().await.map_err(|e| PortError::Rejected(format!("{url}: {e}")))?;
            if res.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PortError::NotFound(url.to_owned()));
            }
            let res = res.error_for_status().map_err(|e| PortError::Rejected(format!("{url}: {e}")))?;
            res.bytes().await.map_err(|e| PortError::Rejected(format!("{url}: {e}")))?.to_vec()
        } else {
            return Err(PortError::Rejected(format!("unsupported scheme in {url}")));
        };
        if bytes.len() as u64 > self.max_bytes {
            return Err(PortError::Rejected(format!("{url} is larger than {} bytes", self.max_bytes)));
        }
        Ok(bytes)
    }
}
