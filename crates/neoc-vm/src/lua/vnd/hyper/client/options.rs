//! Reading and checking a request's options. Everything here runs in the synchronous part of the call, before the
//! future that performs the request is built (see `client`): a script table can run script code when it is read,
//! and the future must hold none.
//!
//! A bad option is an `invalid_input` error returned as `(nil, err)` like every other failure of the call; an option
//! that no function reads is refused by name, so a misspelt option is never dropped.

use std::time::Duration;

use bytes::Bytes;
use hyper::Method;
use hyper::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, TRANSFER_ENCODING};
use mlua::{Table, Value};
use tokio_rustls::rustls::RootCertStore;
use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};

use super::error::HyperError;
use crate::lua::facade::{DeclType, Map, Shape};

/// No data for this long ends a request (`idle_timeout`).
pub(super) const DEFAULT_IDLE: Duration = Duration::from_secs(60);
/// Connecting, TLS handshake included, takes at most this long (`connect_timeout`).
pub(super) const DEFAULT_CONNECT: Duration = Duration::from_secs(10);
/// The most redirects followed (`follow_redirects`), `URLSession`'s documented threshold.
pub(super) const DEFAULT_REDIRECTS: u32 = 16;
/// The largest response body read (`max_body`): 64 MiB.
pub(super) const DEFAULT_MAX_BODY: u64 = 64 << 20;

/// The methods HTTP defines (RFC 9110 section 9.1). They are case-sensitive on the wire, so a script that writes one
/// in another case gets the standard method, as Node, Python and the Fetch standard do.
const STANDARD_METHODS: &[&str] = &[
    "GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH",
];

/// A request body: a string or a buffer.
pub(in crate::lua::vnd::hyper) struct Body;

impl DeclType for Body {
    fn decl_type() -> String {
        "string | buffer".to_owned()
    }
}

/// `follow_redirects`: whether to, or how many.
pub(in crate::lua::vnd::hyper) struct Redirects;

impl DeclType for Redirects {
    fn decl_type() -> String {
        "boolean | number".to_owned()
    }
}

/// The options of one entry point: the fields only it takes, then the ones all of them do. Any other key is an
/// error.
macro_rules! options {
    ($name:ident { $($lead:ident: $ty:ty,)* }) => {
        #[neoc_macros::lua_record(shape)]
        pub(in crate::lua::vnd::hyper) struct $name {
            $($lead: $ty,)*
            headers: Option<Map<String, String>>,
            ca_pem: Option<String>,
            connect_timeout: Option<f64>,
            idle_timeout: Option<f64>,
            timeout: Option<f64>,
            follow_redirects: Option<Redirects>,
            max_body: Option<u64>,
        }
    };
}

options!(GetOptions {});
options!(PostOptions {
    url: String,
    body: Option<Body>,
});
options!(RequestOptions {
    method: String,
    url: String,
    body: Option<Body>,
});

/// The limits of one request.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub connect: Duration,
    pub idle: Duration,
    /// For the whole request, redirects and body included; none by default.
    pub total: Option<Duration>,
    pub max_body: u64,
    /// The most redirects to follow; 0 follows none.
    pub redirects: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            connect: DEFAULT_CONNECT,
            idle: DEFAULT_IDLE,
            total: None,
            max_body: DEFAULT_MAX_BODY,
            redirects: DEFAULT_REDIRECTS,
        }
    }
}

/// A request as the options describe it, checked and ready to send.
pub(super) struct Prepared {
    pub method: String,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
    /// The certificates of `ca_pem`, checked: the only ones trusted, on every hop.
    pub ca_pem: Option<RootCertStore>,
    pub limits: Limits,
}

impl Prepared {
    /// `hyper.get(url, opts)`.
    pub(super) fn get(url: String, opts: Option<&Table>) -> Result<Self, HyperError> {
        let mut prepared = Self {
            method: "GET".to_owned(),
            url,
            headers: HeaderMap::new(),
            body: None,
            ca_pem: None,
            limits: Limits::default(),
        };
        if let Some(opts) = opts {
            check_options(opts, GetOptions::KEYS)?;
            prepared.read_common(opts)?;
        }
        Ok(prepared)
    }

    /// `hyper.post(opts)`.
    pub(super) fn post(opts: &Table) -> Result<Self, HyperError> {
        check_options(opts, PostOptions::KEYS)?;
        Self::from_table("POST".to_owned(), opts)
    }

    /// `hyper.request(opts)`.
    pub(super) fn request(opts: &Table) -> Result<Self, HyperError> {
        check_options(opts, RequestOptions::KEYS)?;
        let method: String = required_string(opts, "method")?;
        Self::from_table(normalize_method(&method)?, opts)
    }

    fn from_table(method: String, opts: &Table) -> Result<Self, HyperError> {
        let mut prepared = Self {
            method,
            url: required_string(opts, "url")?,
            headers: HeaderMap::new(),
            body: body_from_opts(opts)?,
            ca_pem: None,
            limits: Limits::default(),
        };
        prepared.read_common(opts)?;
        Ok(prepared)
    }

    /// The options every entry point reads, then the check that the headers agree with the body.
    fn read_common(&mut self, opts: &Table) -> Result<(), HyperError> {
        self.headers = headers_from_opts(opts)?;
        self.ca_pem = ca_pem(opts)?;
        if let Some(d) = seconds(opts, "connect_timeout")? {
            self.limits.connect = d;
        }
        if let Some(d) = seconds(opts, "idle_timeout")? {
            self.limits.idle = d;
        }
        self.limits.total = seconds(opts, "timeout")?;
        if let Some(n) = whole_number(opts, "max_body", "a positive whole number of bytes")? {
            self.limits.max_body = n;
        }
        if let Some(n) = redirects(opts)? {
            self.limits.redirects = n;
        }
        self.check_content_length()
    }

    /// A `Content-Length` the caller set must be the length of the body: a shorter one would cut the body off
    /// silently and a longer one would hang the request.
    fn check_content_length(&self) -> Result<(), HyperError> {
        let Some(value) = self.headers.get(CONTENT_LENGTH) else {
            return Ok(());
        };
        let declared: u64 = value
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| {
                HyperError::invalid_input("the Content-Length header is not a whole number")
            })?;
        let actual = self.body.as_ref().map_or(0, |b| b.len() as u64);
        if declared == actual {
            Ok(())
        } else {
            Err(HyperError::invalid_input(format!(
                "the Content-Length header says {declared} but the body is {actual} bytes; \
                 drop the header and the module sets it"
            )))
        }
    }
}

/// A method as the script wrote it: a standard one in any case is the standard one (`put` is `PUT`), any other is
/// sent as written if it is a valid method token.
fn normalize_method(method: &str) -> Result<String, HyperError> {
    let upper = method.to_ascii_uppercase();
    let method = if STANDARD_METHODS.contains(&upper.as_str()) {
        upper
    } else {
        method.to_owned()
    };
    Method::from_bytes(method.as_bytes())
        .map(|_| method)
        .map_err(|_| HyperError::invalid_input("method is not a valid HTTP method name"))
}

/// An error naming the first key of `opts` that is not one of `known` (read raw: no metamethod runs).
fn check_options(opts: &Table, known: &[&str]) -> Result<(), HyperError> {
    for pair in opts.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|e| HyperError::invalid_input(e.to_string()))?;
        let name = match &key {
            Value::String(s) => s.to_string_lossy(),
            other => {
                return Err(HyperError::invalid_input(format!(
                    "option names must be strings, got {}",
                    lua_type(other)
                )));
            }
        };
        if !known.contains(&name.as_str()) {
            return Err(HyperError::invalid_input(format!(
                "unknown option `{name}` (expected one of: {})",
                known.join(", ")
            )));
        }
    }
    Ok(())
}

fn required_string(opts: &Table, name: &str) -> Result<String, HyperError> {
    match opts
        .get::<Value>(name)
        .map_err(|e| HyperError::invalid_input(e.to_string()))?
    {
        Value::String(s) => Ok(s.to_string_lossy()),
        Value::Nil => Err(HyperError::invalid_input(format!(
            "missing option `{name}`"
        ))),
        other => Err(HyperError::invalid_input(format!(
            "{name} must be a string, got {}",
            lua_type(&other)
        ))),
    }
}

/// The optional `body`: a string's bytes or a buffer's (a number is sent as its text, as Lua converts it). Anything
/// else is an error, never an empty body.
fn body_from_opts(opts: &Table) -> Result<Option<Bytes>, HyperError> {
    let bad = |e: mlua::Error| HyperError::invalid_input(e.to_string());
    match opts.get::<Value>("body").map_err(bad)? {
        Value::Nil => Ok(None),
        Value::String(s) => Ok(Some(Bytes::copy_from_slice(&s.as_bytes()))),
        Value::Buffer(b) => Ok(Some(Bytes::from(b.to_vec()))),
        // Lua's own number-to-string conversion, as the body was read before
        Value::Integer(_) | Value::Number(_) => opts
            .get::<mlua::LuaString>("body")
            .map(|s| Some(Bytes::copy_from_slice(&s.as_bytes())))
            .map_err(bad),
        other => Err(HyperError::invalid_input(format!(
            "body must be a string or a buffer, got {}",
            lua_type(&other)
        ))),
    }
}

/// The optional `ca_pem`: a string of PEM certificates, each usable as a trust anchor. Checked here, before anything is
/// sent and whatever the URL's scheme, so a bad value is never silently unused.
fn ca_pem(opts: &Table) -> Result<Option<RootCertStore>, HyperError> {
    let pem = match opts
        .get::<Value>("ca_pem")
        .map_err(|e| HyperError::invalid_input(e.to_string()))?
    {
        Value::Nil => return Ok(None),
        Value::String(s) => s,
        other => {
            return Err(HyperError::invalid_input(format!(
                "ca_pem must be a string of PEM certificates, got {}",
                lua_type(&other)
            )));
        }
    };
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(&pem.as_bytes()) {
        let cert =
            cert.map_err(|e| HyperError::invalid_input(format!("ca_pem is not valid PEM: {e}")))?;
        roots.add(cert).map_err(|e| {
            HyperError::invalid_input(format!("ca_pem holds an unusable certificate: {e}"))
        })?;
    }
    if roots.is_empty() {
        return Err(HyperError::invalid_input("ca_pem holds no certificate"));
    }
    Ok(Some(roots))
}

/// The optional `headers` table of name to value strings, checked as header names and values. Anything else is an
/// error, never no headers. Names are case-insensitive, so one written twice (`Host` and `host`) is refused rather
/// than sent twice, and `Transfer-Encoding` is refused: the module frames the body, and a second framing would make
/// the request something other than what was asked for.
fn headers_from_opts(opts: &Table) -> Result<HeaderMap, HyperError> {
    let table = match opts
        .get::<Value>("headers")
        .map_err(|e| HyperError::invalid_input(e.to_string()))?
    {
        Value::Nil => return Ok(HeaderMap::new()),
        Value::Table(t) => t,
        other => {
            return Err(HyperError::invalid_input(format!(
                "headers must be a table, got {}",
                lua_type(&other)
            )));
        }
    };
    let mut pairs = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (name, value) = pair.map_err(|e| HyperError::invalid_input(e.to_string()))?;
        // a key that is not a string is a mistake (`{ "Accept: text/plain" }` has the key 1), never a header named "1"
        let Value::String(name) = name else {
            return Err(HyperError::invalid_input(format!(
                "headers must map header names to values; a key is a {}",
                lua_type(&name)
            )));
        };
        let name = name.to_string_lossy();
        let value = match value {
            // the bytes as the script wrote them (`HeaderValue` refuses control characters, CR and LF included)
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Integer(i) => i.to_string().into_bytes(),
            Value::Number(n) => n.to_string().into_bytes(),
            other => {
                return Err(HyperError::invalid_input(format!(
                    "the value of header `{name}` must be a string, got {}",
                    lua_type(&other)
                )));
            }
        };
        pairs.push((name, value));
    }
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        let header = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            HyperError::invalid_input(format!("`{name}` is not a valid header name"))
        })?;
        let value = HeaderValue::from_bytes(&value).map_err(|_| {
            HyperError::invalid_input(format!("the value of header `{name}` is not valid"))
        })?;
        if header == TRANSFER_ENCODING {
            return Err(HyperError::invalid_input(
                "the Transfer-Encoding header is set by the module; drop it",
            ));
        }
        if headers.contains_key(&header) {
            return Err(HyperError::invalid_input(format!(
                "header `{header}` is given twice (header names are case-insensitive)"
            )));
        }
        headers.insert(header, value);
    }
    Ok(headers)
}

/// A number option as a duration in seconds: positive and finite.
fn seconds(opts: &Table, name: &str) -> Result<Option<Duration>, HyperError> {
    let Some(secs) = number(opts, name, "a positive, finite number of seconds")? else {
        return Ok(None);
    };
    Duration::try_from_secs_f64(secs)
        .ok()
        .filter(|d| !d.is_zero())
        .map(Some)
        .ok_or_else(|| {
            HyperError::invalid_input(format!(
                "{name} must be a positive, finite number of seconds that a duration can hold, got {secs}"
            ))
        })
}

/// A number option that must be a positive whole number.
fn whole_number(opts: &Table, name: &str, what: &str) -> Result<Option<u64>, HyperError> {
    let Some(n) = number(opts, name, what)? else {
        return Ok(None);
    };
    whole(n)
        .map(Some)
        .ok_or_else(|| HyperError::invalid_input(format!("{name} must be {what}, got {n}")))
}

fn whole(n: f64) -> Option<u64> {
    // a float above 2^63 saturates, which is what "no limit worth naming" should mean
    (n.is_finite() && n >= 1.0 && n.fract() == 0.0).then_some(n as u64)
}

/// `follow_redirects`: `false` follows none, `true` the default, a number that many.
fn redirects(opts: &Table) -> Result<Option<u32>, HyperError> {
    let what = "a boolean or a positive whole number of redirects";
    let bad = |e: mlua::Error| HyperError::invalid_input(e.to_string());
    match opts.get::<Value>("follow_redirects").map_err(bad)? {
        Value::Nil => Ok(None),
        Value::Boolean(false) => Ok(Some(0)),
        Value::Boolean(true) => Ok(Some(DEFAULT_REDIRECTS)),
        Value::Integer(i) => whole(i as f64)
            .map(|n| Some(u32::try_from(n).unwrap_or(u32::MAX)))
            .ok_or_else(|| {
                HyperError::invalid_input(format!("follow_redirects must be {what}, got {i}"))
            }),
        Value::Number(n) => whole(n)
            .map(|n| Some(u32::try_from(n).unwrap_or(u32::MAX)))
            .ok_or_else(|| {
                HyperError::invalid_input(format!("follow_redirects must be {what}, got {n}"))
            }),
        other => Err(HyperError::invalid_input(format!(
            "follow_redirects must be {what}, got {}",
            lua_type(&other)
        ))),
    }
}

/// A number option as a float; anything that is not a number is an error.
fn number(opts: &Table, name: &str, what: &str) -> Result<Option<f64>, HyperError> {
    match opts
        .get::<Value>(name)
        .map_err(|e| HyperError::invalid_input(e.to_string()))?
    {
        Value::Nil => Ok(None),
        Value::Integer(i) => Ok(Some(i as f64)),
        Value::Number(n) => Ok(Some(n)),
        other => Err(HyperError::invalid_input(format!(
            "{name} must be {what}, got {}",
            lua_type(&other)
        ))),
    }
}

/// The name of a value's type as a Luau script knows it: a number is a `number`, whether mlua holds it as an integer
/// or a float.
fn lua_type(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) | Value::Number(_) => "number",
        other => other.type_name(),
    }
}
