//! The bytes of the files the Files page browses, behind the session guard like `/rpc`:
//!
//! | route                                 | what                                                           |
//! |---------------------------------------|----------------------------------------------------------------|
//! | `GET /files/app/{app}/{path...}`      | download a file of an installed app's Documents                 |
//! | `GET /files/host/{path...}`           | download a file of This Computer: `/files/host/Users/me/a.txt` is `/Users/me/a.txt` |
//! | `PUT` on either, `?replace=true`      | upload: the request body is the file                             |
//!
//! The page lists and changes folders over `/rpc` (`host.files.*`) and moves bytes here, because a download is the
//! browser's to make (a link with a cookie) and an upload is a request body, not base64 on a socket.
//!
//! **A file is never a page of the host.** Whoever wrote a file decides its name and contents, and this route is on
//! the shell's origin, where a paired browser's cookie is: served as a document, `evil.html` would be a script with
//! the person's session. So every answer is `Content-Disposition: attachment` (the browser saves it, whatever it
//! is), `application/octet-stream` with `X-Content-Type-Options: nosniff` (it is never guessed to be HTML, a script or
//! a style), and a `Content-Security-Policy` of `sandbox` and `default-src 'none'` for a browser that shows it all the
//! same.
//!
//! **An upload cannot be forged from another site.** The Origin/Host guard is over every route: a browser sends
//! `Origin` with every `PUT`, and one that is not this host's is answered `403` before this handler runs (a
//! sandboxed frame's `Origin: null` too: that exception is for reads of public files only). The session cookie is
//! `SameSite=Strict`, so another site's request would not carry it either, and a `PUT` with a body is never a "simple"
//! cross-origin request: the browser asks first (`OPTIONS`), which no route here answers.
//!
//! **An upload is whole or absent.** The body is taken up to [`MAX_UPLOAD_BYTES`] (more is `413`, with nothing
//! written), then written by the adapter under a temporary name beside the destination and renamed into place. An
//! existing file is replaced only with `?replace=true`; otherwise the answer is `409` and nothing is written.
//!
//! Paths keep the rules of their place (`vos_application::UserFiles`): an app's Documents are confined (no `..`, no
//! absolute path, no link followed, no host path in an error); This Computer is the whole file system.
//!
//! Security: This Computer is unrestricted on purpose. The host is the person's own machine, and a paired device can
//! already install apps (arbitrary code on the host) and run containers on it, so reading and writing its files
//! gives it nothing it did not have; an unpaired client cannot reach these routes.

use axum::{
    body::Bytes,
    extract::{
        rejection::{BytesRejection, QueryRejection},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::{fmt::Write as _, sync::Arc};
use vos_application::{FilesService, Place, UserFiles};
use vos_domain::{limits::MAX_UPLOAD_BYTES, AppId, FileError, FileErrorKind};

/// The policy of a downloaded file, should a browser ever draw one: an opaque origin, and nothing may load or run.
const DOWNLOAD_CSP: &str = "sandbox; default-src 'none'";

/// The download and upload routes. Mount them behind [`crate::require_session`].
pub fn files_router(files: Arc<FilesService>) -> Router {
    Router::new()
        .route("/files/host/{*path}", get(download_host).put(upload_host))
        .route("/files/app/{app}/{*path}", get(download_app).put(upload_app))
        // the body of an upload: at most the limit, whatever `Content-Length` says
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .with_state(files)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadQuery {
    /// Replace a file that is already there (otherwise `409`).
    #[serde(default)]
    replace: bool,
}

type Body = Result<Bytes, BytesRejection>;

async fn download_host(State(files): State<Arc<FilesService>>, Path(path): Path<String>) -> Response {
    download(files.open(&Place::Computer), &format!("/{path}")).await
}

async fn download_app(
    State(files): State<Arc<FilesService>>,
    Path((app, path)): Path<(String, String)>,
) -> Response {
    download(open_app(&files, &app), &path).await
}

async fn upload_host(
    State(files): State<Arc<FilesService>>,
    Path(path): Path<String>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    body: Body,
) -> Response {
    upload(files.open(&Place::Computer), &format!("/{path}"), query, body).await
}

async fn upload_app(
    State(files): State<Arc<FilesService>>,
    Path((app, path)): Path<(String, String)>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    body: Body,
) -> Response {
    upload(open_app(&files, &app), &path, query, body).await
}

fn open_app(files: &FilesService, app: &str) -> Result<UserFiles, FileError> {
    let id = AppId::parse(app).map_err(|_| FileError::new(FileErrorKind::NotFound, "no such app"))?;
    files.open(&Place::App(id))
}

async fn download(place: Result<UserFiles, FileError>, path: &str) -> Response {
    let read = match place {
        Ok(place) => place.read(path).await,
        Err(e) => Err(e),
    };
    match read {
        Ok(bytes) => {
            let name = path.rsplit('/').find(|part| !part.is_empty()).unwrap_or("file");
            let mut response = (StatusCode::OK, bytes).into_response();
            if let Ok(value) = HeaderValue::from_str(&attachment(name)) {
                response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
            }
            contain(response.headers_mut());
            response
        }
        Err(e) => refuse(status_of(e.kind), &e.message),
    }
}

async fn upload(
    place: Result<UserFiles, FileError>,
    path: &str,
    query: Result<Query<UploadQuery>, QueryRejection>,
    body: Body,
) -> Response {
    let Ok(Query(query)) = query else {
        return refuse(StatusCode::BAD_REQUEST, "the only option of an upload is `replace=true`");
    };
    let place = match place {
        Ok(place) => place,
        Err(e) => return refuse(status_of(e.kind), &e.message),
    };
    let data = match body {
        Ok(data) => data,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            let limit = format!("an upload may be at most {MAX_UPLOAD_BYTES} bytes");
            return refuse(StatusCode::PAYLOAD_TOO_LARGE, &limit);
        }
        Err(rejection) => return refuse(rejection.status(), "the upload did not arrive whole"),
    };
    match place.upload(path, data.to_vec(), query.replace).await {
        Ok(()) => refuse(StatusCode::CREATED, "saved"),
        Err(e) => refuse(status_of(e.kind), &e.message),
    }
}

/// The status of a failed file call. The message beside it is the use case's: it names the path as it was asked for.
fn status_of(kind: FileErrorKind) -> StatusCode {
    match kind {
        FileErrorKind::NotFound | FileErrorKind::Unsupported => StatusCode::NOT_FOUND,
        FileErrorKind::InvalidInput | FileErrorKind::IsDirectory | FileErrorKind::NotADirectory => {
            StatusCode::BAD_REQUEST
        }
        FileErrorKind::AlreadyExists => StatusCode::CONFLICT,
        FileErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        FileErrorKind::PermissionDenied | FileErrorKind::ReadOnlyFilesystem => StatusCode::FORBIDDEN,
        FileErrorKind::StorageFull => StatusCode::INSUFFICIENT_STORAGE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// The headers every answer of these routes carries, a file or a message: never a document, never sniffed, never kept.
fn contain(headers: &mut HeaderMap) {
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(DOWNLOAD_CSP));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
}

fn refuse(status: StatusCode, message: &str) -> Response {
    let mut response = (status, message.to_owned()).into_response();
    contain(response.headers_mut());
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

/// `Content-Disposition` for a file called `name` (RFC 6266): a plain ASCII name every browser reads, and the real
/// name percent-encoded for those that read `filename*`. The name is an app's: nothing of it goes into the header as it is.
fn attachment(name: &str) -> String {
    let plain: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect();
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_reaches_the_header_as_a_plain_name_and_an_encoded_one() {
        assert_eq!(attachment("notes.txt"), "attachment; filename=\"notes.txt\"; filename*=UTF-8''notes.txt");
        assert_eq!(
            attachment("my \"plan\".md"),
            "attachment; filename=\"my _plan_.md\"; filename*=UTF-8''my%20%22plan%22.md"
        );
        assert_eq!(
            attachment("été.txt"),
            "attachment; filename=\"_t_.txt\"; filename*=UTF-8''%C3%A9t%C3%A9.txt"
        );
        // nothing an app names a file can end the header or start another
        let hostile = attachment("a\r\nSet-Cookie: x=1;.html");
        assert!(
            !hostile.contains(['\r', '\n', ';'].as_slice()) || hostile.matches(';').count() == 2,
            "{hostile}"
        );
        assert!(HeaderValue::from_str(&hostile).is_ok());
    }
}
