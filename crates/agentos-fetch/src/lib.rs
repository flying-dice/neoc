//! Driven adapter: the `Fetcher` port over `file://`, `http://` and `https://` (reqwest with rustls, plain
//! files). App refs and every file they list load through it, and nothing is read past a size cap:
//!
//! - a file must be a regular file (a FIFO or `/dev/zero` is refused, not opened) and is read in chunks up to
//!   the cap;
//! - an HTTP body is refused up front when `Content-Length` is over the cap and is otherwise read in chunks
//!   until the cap, never buffered whole;
//! - a redirect is followed only to the same origin, and a URL (or a name it resolves to) that is a private,
//!   link-local, metadata or other non-public address is refused unless the URL itself names loopback
//!   ([`guard`]);
//! - no proxy is used, whatever the environment says: the guard has to see every connection.

mod guard;

use agentos_application::{
    ports::{Fetcher, PortResult},
    PortError,
};
use async_trait::async_trait;
use reqwest::{redirect::Policy, Url};
use std::{io::Read, time::Duration};

/// How long one fetch (a whole file read or a whole HTTP exchange, body included) may take.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// How much of a file or body is read at a time.
const CHUNK: usize = 64 * 1024;

/// One `Fetcher` for all three schemes; the app ref and every file it lists load identically.
pub struct UrlFetcher {
    http: reqwest::Client,
    max_bytes: u64,
}

impl UrlFetcher {
    pub fn new(max_bytes: u64) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            // a proxy (HTTP(S)_PROXY, ALL_PROXY) would resolve and connect to the target itself, past the guard
            .no_proxy()
            .dns_resolver(guard::GuardedResolver::shared())
            .redirect(Policy::custom(|attempt| {
                let origin = attempt.previous().first().cloned();
                if attempt.previous().len() > guard::MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if origin.is_some_and(|first| guard::same_origin(&first, attempt.url())) {
                    attempt.follow()
                } else {
                    let to = attempt.url().to_string();
                    attempt.error(format!("a redirect to {to} leaves the origin the file was requested from"))
                }
            }))
            .build()?;
        Ok(Self { http, max_bytes })
    }

    fn too_large(&self, url: &str) -> PortError {
        PortError::Rejected(format!("{url} is larger than {} bytes", self.max_bytes))
    }

    async fn fetch_file(&self, url: &str, path: &str) -> PortResult<Vec<u8>> {
        let (path, max) = (path.to_owned(), self.max_bytes);
        let read = tokio::task::spawn_blocking(move || read_file(&path, max));
        let read = tokio::time::timeout(FETCH_TIMEOUT, read)
            .await
            .map_err(|_| PortError::Rejected(format!("{url}: reading took longer than {FETCH_TIMEOUT:?}")))?;
        match read.map_err(|e| PortError::Internal(format!("{url}: {e}")))? {
            Ok(bytes) => Ok(bytes),
            Err(ReadError::TooLarge) => Err(self.too_large(url)),
            Err(ReadError::NotRegular) => Err(PortError::Rejected(format!("{url} is not a regular file"))),
            Err(ReadError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(PortError::NotFound(url.to_owned()))
            }
            Err(ReadError::Io(e)) => Err(PortError::Rejected(format!("{url}: {e}"))),
        }
    }

    async fn fetch_http(&self, url: &str) -> PortResult<Vec<u8>> {
        let rejected = |e: &dyn std::error::Error| PortError::Rejected(format!("{url}: {}", describe(e)));
        let parsed = Url::parse(url).map_err(|e| rejected(&e))?;
        guard::check_url(&parsed).map_err(|why| PortError::Rejected(format!("{url}: {why}")))?;
        let mut res = self.http.get(parsed).send().await.map_err(|e| rejected(&e))?;
        let status = res.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(PortError::NotFound(url.to_owned()));
        }
        if !status.is_success() {
            return Err(PortError::Rejected(format!("{url}: HTTP status {status}")));
        }
        if res.content_length().is_some_and(|n| n > self.max_bytes) {
            return Err(self.too_large(url));
        }
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(|e| rejected(&e))? {
            if (body.len() + chunk.len()) as u64 > self.max_bytes {
                return Err(self.too_large(url));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

#[async_trait]
impl Fetcher for UrlFetcher {
    async fn fetch(&self, url: &str) -> PortResult<Vec<u8>> {
        if let Some(path) = url.strip_prefix("file://") {
            self.fetch_file(url, path).await
        } else if url.starts_with("http://") || url.starts_with("https://") {
            self.fetch_http(url).await
        } else {
            Err(PortError::Rejected(format!("unsupported scheme in {url}")))
        }
    }
}

/// An error and its causes on one line (reqwest puts the useful part, a refused redirect or address, in
/// the source chain).
fn describe(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

enum ReadError {
    TooLarge,
    NotRegular,
    Io(std::io::Error),
}

impl From<std::io::Error> for ReadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Read a regular file of at most `max` bytes. The path is checked before it is opened (opening a FIFO
/// blocks) and the open handle again after (the path may have changed), then read in chunks up to the cap.
fn read_file(path: &str, max: u64) -> Result<Vec<u8>, ReadError> {
    let regular = |m: &std::fs::Metadata| m.is_file();
    if !regular(&std::fs::metadata(path)?) {
        return Err(ReadError::NotRegular);
    }
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !regular(&meta) {
        return Err(ReadError::NotRegular);
    }
    if meta.len() > max {
        return Err(ReadError::TooLarge);
    }
    let mut out = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            return Ok(out);
        }
        if (out.len() + n) as u64 > max {
            return Err(ReadError::TooLarge);
        }
        out.extend_from_slice(&chunk[..n]);
    }
}
