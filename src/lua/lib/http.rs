//! `lib:http` — glob-scoped HTTP/1.1 static file server.
//!
//! See docs/src/lib/reference/http.md for the public surface contract.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use globset::GlobSetBuilder;
use mlua::{Lua, Table};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tracing::instrument;
use walkdir::WalkDir;

struct StaticFile {
    abs_path: PathBuf,
    size: u64,
    modified: SystemTime,
    mime: &'static str,
}

type FileMap = HashMap<String, StaticFile>;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("serve_static", lua.create_async_function(serve_static)?)?;
    Ok(t)
}

#[instrument(name = "http.serve_static", target = "luau::lib::http", skip_all)]
async fn serve_static(
    _lua: Lua,
    (glob_pattern, opts): (String, Option<Table>),
) -> mlua::Result<()> {
    let port: u16 = opts
        .as_ref()
        .and_then(|o| o.get::<Option<u16>>("port").ok().flatten())
        .unwrap_or(8080);
    let host: String = opts
        .as_ref()
        .and_then(|o| o.get::<Option<String>>("host").ok().flatten())
        .unwrap_or_else(|| "0.0.0.0".to_string());

    let files = resolve_glob(&glob_pattern)
        .map_err(|e| mlua::Error::external(format!("http.serve_static: {e}")))?;
    let files = Arc::new(files);
    let index = Arc::new(build_index(&files, &glob_pattern));

    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| mlua::Error::external(format!("http.serve_static: bind {addr}: {e}")))?;

    tracing::info!("http static server on http://{addr} ({} files)", files.len());

    tokio::select! {
        _ = serve_loop(listener, files, index) => {},
        result = tokio::signal::ctrl_c() => {
            if let Err(e) = result {
                tracing::error!("signal error: {e}");
            }
        },
    }

    Ok(())
}

async fn serve_loop(listener: TcpListener, files: Arc<FileMap>, index: Arc<String>) {
    loop {
        match listener.accept().await {
            Ok((stream, _peer)) => {
                let files = files.clone();
                let index = index.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, &files, &index).await {
                        tracing::debug!("connection error: {e}");
                    }
                });
            }
            Err(e) => {
                tracing::error!("accept error: {e}");
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    files: &FileMap,
    index: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let (method, path) = parse_request_line(&request_line)?;

    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line).await?;
        if header_line == "\r\n" || header_line.is_empty() {
            break;
        }
    }

    if method != "GET" && method != "HEAD" {
        write_response(&mut writer, 405, "Method Not Allowed", "text/plain", None, b"Method Not Allowed").await?;
        return Ok(());
    }

    if path == "/" {
        write_response(&mut writer, 200, "OK", "text/html; charset=utf-8", None, index.as_bytes()).await?;
    } else {
        let rel = path.trim_start_matches('/');
        if rel.contains("..") {
            write_response(&mut writer, 403, "Forbidden", "text/plain", None, b"Forbidden").await?;
            return Ok(());
        }
        match files.get(rel) {
            Some(f) => {
                let data = tokio::fs::read(&f.abs_path).await?;
                write_response(&mut writer, 200, "OK", f.mime, Some(f.modified), &data).await?;
            }
            None => {
                write_response(&mut writer, 404, "Not Found", "text/plain", None, b"Not Found").await?;
            }
        }
    }
    Ok(())
}

fn parse_request_line(
    line: &str,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("empty request line")?.to_uppercase();
    let raw_path = parts.next().ok_or("missing path")?.to_string();
    let path = raw_path.split('?').next().unwrap_or("/").to_string();
    Ok((method, path))
}

async fn write_response(
    writer: &mut (impl AsyncWriteExt + Unpin),
    status: u16,
    reason: &str,
    content_type: &str,
    modified: Option<SystemTime>,
    body: &[u8],
) -> tokio::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(mtime) = modified {
        head.push_str(&format!("Last-Modified: {}\r\n", fmt_http_date(mtime)));
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

// ---------------------------------------------------------------------------
// Glob resolution
// ---------------------------------------------------------------------------

fn glob_root(pattern: &str) -> PathBuf {
    let cleaned = pattern.trim_start_matches("./");
    let mut parts: Vec<&str> = Vec::new();
    for seg in cleaned.split('/') {
        if seg.contains(['*', '?', '[', '{']) {
            break;
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(parts.join("/"))
    }
}

fn resolve_glob(
    pattern: &str,
) -> Result<FileMap, Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = GlobSetBuilder::new();
    builder.add(globset::Glob::new(pattern)?);
    let set = builder.build()?;

    let root = glob_root(pattern);
    let mut files: FileMap = HashMap::new();

    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !set.is_match(path) {
            continue;
        }
        let meta = entry.metadata()?;
        let rel = path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        files.insert(
            rel.clone(),
            StaticFile {
                abs_path: path.to_path_buf(),
                size: meta.len(),
                modified: meta.modified().unwrap_or(UNIX_EPOCH),
                mime: mime_for_ext(&ext),
            },
        );
    }

    Ok(files)
}

// ---------------------------------------------------------------------------
// MIME map (v1 inline; delegate to lib:mime when available)
// ---------------------------------------------------------------------------

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Index page
// ---------------------------------------------------------------------------

fn build_index(files: &FileMap, glob_pattern: &str) -> String {
    let mut entries: Vec<(&str, &StaticFile)> =
        files.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by_key(|(rel, _)| *rel);

    let rows: String = entries
        .iter()
        .map(|(rel, f)| {
            let size = human_size(f.size);
            let mtime = fmt_http_date(f.modified);
            let is_image = f.mime.starts_with("image/");
            let preview = if is_image {
                format!(
                    r#"<img src="/{rel}" loading="lazy" alt="" style="max-height:64px;max-width:128px;vertical-align:middle;margin-right:8px">"#
                )
            } else {
                String::new()
            };
            format!(
                r#"<tr><td>{preview}<a href="/{rel}">{rel}</a></td><td>{size}</td><td>{mtime}</td></tr>"#
            )
        })
        .collect();

    let count = entries.len();
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Index of {glob_pattern}</title>
<style>
:root{{color-scheme:light dark}}
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:system-ui,sans-serif;padding:1.5rem;max-width:960px;margin:auto}}
h1{{font-size:1.1rem;font-weight:600;margin-bottom:1rem;word-break:break-all}}
table{{width:100%;border-collapse:collapse;font-size:.875rem}}
th,td{{text-align:left;padding:.4rem .6rem;border-bottom:1px solid color-mix(in srgb,currentColor 12%,transparent)}}
th{{font-weight:600;white-space:nowrap}}
td:nth-child(2),td:nth-child(3){{white-space:nowrap;color:color-mix(in srgb,currentColor 60%,transparent)}}
a{{color:inherit;text-decoration:none}}
a:hover{{text-decoration:underline}}
</style>
</head>
<body>
<h1>Index of {glob_pattern} <small style="font-weight:normal;color:color-mix(in srgb,currentColor 50%,transparent)">({count} files)</small></h1>
<table>
<thead><tr><th>File</th><th>Size</th><th>Modified</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</body>
</html>
"#
    )
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ---------------------------------------------------------------------------
// HTTP date formatting (RFC 7231 §7.1.1.1)
// ---------------------------------------------------------------------------

fn fmt_http_date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs();
    let days = secs / 86400;
    // 1970-01-01 was a Thursday (index 4 in Sun=0 week).
    let dow = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][((days + 4) % 7) as usize];
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = days_to_ymd(days);
    let h = ((secs / 3600) % 24) as u32;
    let min = ((secs / 60) % 60) as u32;
    let sec = (secs % 60) as u32;
    format!("{dow}, {d:02} {} {y} {h:02}:{min:02}:{sec:02} GMT", months[(m - 1) as usize])
}

fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Gregorian calendar via Julian Day Number algorithm.
    let jdn = days + 2_440_588u64; // JDN of 1970-01-01
    let f = jdn + 1401 + (((4 * jdn + 274_277) / 146_097) * 3) / 4 - 38;
    let e = 4 * f + 3;
    let g = (e % 1461) / 4;
    let h = 5 * g + 2;
    let day = (h % 153) / 5 + 1;
    let month = (h / 153 + 2) % 12 + 1;
    let year = e / 1461 - 4716 + (14 - month) / 12;
    (year as u32, month as u32, day as u32)
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("http");

    g.bench_function("mime_for_ext_png", |b| {
        b.iter(|| mime_for_ext(black_box("png")));
    });

    g.bench_function("mime_for_ext_unknown", |b| {
        b.iter(|| mime_for_ext(black_box("xyz")));
    });

    g.bench_function("fmt_http_date", |b| {
        let t = SystemTime::now();
        b.iter(|| fmt_http_date(black_box(t)));
    });

    let _ = lua;
    g.finish();
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_known_types() {
        assert_eq!(mime_for_ext("png"), "image/png");
        assert_eq!(mime_for_ext("jpg"), "image/jpeg");
        assert_eq!(mime_for_ext("jpeg"), "image/jpeg");
        assert_eq!(mime_for_ext("html"), "text/html; charset=utf-8");
        assert_eq!(mime_for_ext("js"), "application/javascript");
        assert_eq!(mime_for_ext("json"), "application/json");
        assert_eq!(mime_for_ext("wasm"), "application/wasm");
    }

    #[test]
    fn mime_unknown_falls_back() {
        assert_eq!(mime_for_ext("xyz"), "application/octet-stream");
        assert_eq!(mime_for_ext(""), "application/octet-stream");
    }

    #[test]
    fn days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-15 = 19737 days since epoch
        let (y, m, d) = days_to_ymd(19737);
        assert_eq!((y, m, d), (2024, 1, 15));
    }

    #[test]
    fn fmt_http_date_epoch() {
        let t = UNIX_EPOCH;
        assert_eq!(fmt_http_date(t), "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn fmt_http_date_known() {
        // 2024-01-15 11:30:45 UTC = 19737*86400 + 11*3600 + 30*60 + 45 = 1705318245
        let t = UNIX_EPOCH + Duration::from_secs(1_705_318_245);
        assert_eq!(fmt_http_date(t), "Mon, 15 Jan 2024 11:30:45 GMT");
    }

    #[test]
    fn human_size_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
    }

    #[test]
    fn human_size_kib() {
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(2048), "2.0 KiB");
    }

    #[test]
    fn human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn glob_root_simple() {
        assert_eq!(glob_root("screenshots/**/*.png"), PathBuf::from("screenshots"));
        assert_eq!(glob_root("./**/*.png"), PathBuf::from("."));
        assert_eq!(glob_root("**/*.png"), PathBuf::from("."));
    }

    #[test]
    fn glob_root_nested() {
        assert_eq!(
            glob_root("assets/images/**/*.png"),
            PathBuf::from("assets/images")
        );
    }

    #[test]
    fn parse_request_line_get() {
        let (method, path) = parse_request_line("GET / HTTP/1.1\r\n").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/");
    }

    #[test]
    fn parse_request_line_strips_query() {
        let (method, path) = parse_request_line("GET /foo.png?v=1 HTTP/1.1\r\n").unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/foo.png");
    }

    #[test]
    fn build_index_empty() {
        let files: FileMap = HashMap::new();
        let html = build_index(&files, "*.png");
        assert!(html.contains("0 files"));
        assert!(html.contains("Index of *.png"));
    }

    #[test]
    fn build_index_has_links() {
        let mut files: FileMap = HashMap::new();
        files.insert(
            "shot.png".to_string(),
            StaticFile {
                abs_path: PathBuf::from("/tmp/shot.png"),
                size: 1024,
                modified: UNIX_EPOCH,
                mime: "image/png",
            },
        );
        let html = build_index(&files, "*.png");
        assert!(html.contains(r#"href="/shot.png""#));
        assert!(html.contains("1.0 KiB"));
    }

    #[test]
    fn resolve_glob_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let pattern = format!("{}/**/*.png", dir.path().display());
        let files = resolve_glob(&pattern).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn resolve_glob_finds_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"PNG").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"TXT").unwrap();
        let pattern = format!("{}/**/*.png", dir.path().display());
        let files = resolve_glob(&pattern).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("a.png"));
    }
}
