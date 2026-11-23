//! Reading and checking a request's options. Everything here runs in the synchronous part of the call, before the
//! future that performs the request is built (see `client`): the future holds no script value. The options are
//! read raw (no `__index`, no `__pairs`), so reading them runs no script code.
//!
//! A bad option is an `invalid_input` error returned as `(nil, err)` like every other failure of the call; an option
//! that no function reads is refused by name, so a misspelt option is never dropped.

use std::time::Duration;

use bytes::Bytes;
use hyper::Method;
use hyper::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, TRANSFER_ENCODING};
use mlua::{Lua, Table, Value};
use tokio_rustls::rustls::RootCertStore;
use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};

use super::error::HyperError;
use crate::lua::facade::args::{Field, Refused};
use crate::lua::facade::{DeclType, Map};

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

/// A string a request cannot do without (`url`, `method`).
pub(in crate::lua::vnd::hyper) struct Required(String);

impl DeclType for Required {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Field for Required {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match value {
            Value::String(s) => Ok(Self(s.to_string_lossy())),
            Value::Nil => Err(Refused(format!("missing option `{name}`"))),
            other => Err(Refused(format!(
                "{name} must be a string, got {}",
                lua_type(&other)
            ))),
        }
    }
}

/// A request body: a string's bytes or a buffer's (a number is sent as its text, as Lua converts it). Anything
/// else is an error, never an empty body.
pub(in crate::lua::vnd::hyper) struct Body(Value);

impl DeclType for Body {
    fn decl_type() -> String {
        "string | buffer".to_owned()
    }
}

impl Field for Body {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        match value {
            Value::String(_) | Value::Buffer(_) | Value::Integer(_) | Value::Number(_) => {
                Ok(Self(value))
            }
            other => Err(Refused(format!(
                "{name} must be a string or a buffer, got {}",
                lua_type(&other)
            ))),
        }
    }
}

impl Body {
    fn bytes(self, lua: &Lua) -> Result<Bytes, HyperError> {
        match self.0 {
            Value::Buffer(b) => Ok(Bytes::from(b.to_vec())),
            // a string's bytes, or Lua's own number-to-string conversion
            other => lua
                .coerce_string(other)
                .ok()
                .flatten()
                .map(|s| Bytes::copy_from_slice(&s.as_bytes()))
                .ok_or_else(|| HyperError::invalid_input("body could not be read")),
        }
    }
}

/// `follow_redirects`: `false` follows none, `true` the default, a number that many.
pub(in crate::lua::vnd::hyper) struct Redirects(u32);

impl DeclType for Redirects {
    fn decl_type() -> String {
        "boolean | number".to_owned()
    }
}

impl Field for Redirects {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let what = "a boolean or a positive whole number of redirects";
        let count = |n: f64| whole(n).map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        match value {
            Value::Boolean(false) => Ok(Self(0)),
            Value::Boolean(true) => Ok(Self(DEFAULT_REDIRECTS)),
            Value::Integer(i) => count(i as f64)
                .map(Self)
                .ok_or_else(|| Refused(format!("{name} must be {what}, got {i}"))),
            Value::Number(n) => count(n)
                .map(Self)
                .ok_or_else(|| Refused(format!("{name} must be {what}, got {n}"))),
            other => Err(Refused(format!(
                "{name} must be {what}, got {}",
                lua_type(&other)
            ))),
        }
    }
}

/// A number of seconds as a duration: positive and finite.
pub(in crate::lua::vnd::hyper) struct Seconds(Duration);

impl DeclType for Seconds {
    fn decl_type() -> String {
        f64::decl_type()
    }
}

impl Field for Seconds {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let secs = number(name, &value, "a positive, finite number of seconds")?;
        Duration::try_from_secs_f64(secs)
            .ok()
            .filter(|d| !d.is_zero())
            .map(Self)
            .ok_or_else(|| {
                Refused(format!(
                    "{name} must be a positive, finite number of seconds that a duration can hold, got {secs}"
                ))
            })
    }
}

/// `max_body`: a positive whole number of bytes.
pub(in crate::lua::vnd::hyper) struct MaxBody(u64);

impl DeclType for MaxBody {
    fn decl_type() -> String {
        u64::decl_type()
    }
}

impl Field for MaxBody {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let what = "a positive whole number of bytes";
        let n = number(name, &value, what)?;
        whole(n)
            .map(Self)
            .ok_or_else(|| Refused(format!("{name} must be {what}, got {n}")))
    }
}

/// `ca_pem`: a string of PEM certificates, each usable as a trust anchor. Checked when the options are read,
/// before anything is sent and whatever the URL's scheme, so a bad value is never silently unused.
pub(in crate::lua::vnd::hyper) struct CaPem(RootCertStore);

impl DeclType for CaPem {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Field for CaPem {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let Value::String(pem) = value else {
            return Err(Refused(format!(
                "{name} must be a string of PEM certificates, got {}",
                lua_type(&value)
            )));
        };
        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(&pem.as_bytes()) {
            let cert = cert.map_err(|e| Refused(format!("{name} is not valid PEM: {e}")))?;
            roots
                .add(cert)
                .map_err(|e| Refused(format!("{name} holds an unusable certificate: {e}")))?;
        }
        if roots.is_empty() {
            return Err(Refused(format!("{name} holds no certificate")));
        }
        Ok(Self(roots))
    }
}

/// `headers`: a table of name to value strings, checked as header names and values. Anything else is an error,
/// never no headers. Names are case-insensitive, so one written twice (`Host` and `host`) is refused rather than
/// sent twice, and `Transfer-Encoding` is refused: the module frames the body, and a second framing would make
/// the request something other than what was asked for.
pub(in crate::lua::vnd::hyper) struct Headers(HeaderMap);

impl DeclType for Headers {
    fn decl_type() -> String {
        Map::<String, String>::decl_type()
    }
}

impl Field for Headers {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let Value::Table(table) = value else {
            return Err(Refused(format!(
                "{name} must be a table, got {}",
                lua_type(&value)
            )));
        };
        headers_of(&table).map(Self)
    }
}

/// The fields every entry point takes.
pub(super) struct Common {
    headers: Option<Headers>,
    ca_pem: Option<CaPem>,
    connect_timeout: Option<Seconds>,
    idle_timeout: Option<Seconds>,
    timeout: Option<Seconds>,
    follow_redirects: Option<Redirects>,
    max_body: Option<MaxBody>,
}

/// The options of one entry point: the fields only it takes, then the ones all of them do. Any other key is an
/// error.
macro_rules! options {
    ($name:ident { $($lead:ident: $ty:ty,)* }) => {
        #[neoc_macros::lua_record(from)]
        pub(in crate::lua::vnd::hyper) struct $name {
            $($lead: $ty,)*
            headers: Option<Headers>,
            ca_pem: Option<CaPem>,
            connect_timeout: Option<Seconds>,
            idle_timeout: Option<Seconds>,
            timeout: Option<Seconds>,
            follow_redirects: Option<Redirects>,
            max_body: Option<MaxBody>,
        }

        impl $name {
            /// The fields all the entry points take, taken out of this one's.
            fn common(&mut self) -> Common {
                Common {
                    headers: self.headers.take(),
                    ca_pem: self.ca_pem.take(),
                    connect_timeout: self.connect_timeout.take(),
                    idle_timeout: self.idle_timeout.take(),
                    timeout: self.timeout.take(),
                    follow_redirects: self.follow_redirects.take(),
                    max_body: self.max_body.take(),
                }
            }
        }
    };
}

options!(GetOptions {});
options!(PostOptions {
    url: Required,
    body: Option<Body>,
});
options!(RequestOptions {
    method: Required,
    url: Required,
    body: Option<Body>,
});

impl From<Refused> for HyperError {
    fn from(refused: Refused) -> Self {
        HyperError::invalid_input(refused.0)
    }
}

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
    pub(super) fn get(url: String, opts: Option<GetOptions>) -> Result<Self, HyperError> {
        let common = opts.map(|mut opts| opts.common());
        Self::new("GET".to_owned(), url, None, common)
    }

    /// `hyper.post(opts)`.
    pub(super) fn post(lua: &Lua, mut opts: PostOptions) -> Result<Self, HyperError> {
        let common = opts.common();
        let body = opts.body.map(|body| body.bytes(lua)).transpose()?;
        Self::new("POST".to_owned(), opts.url.0, body, Some(common))
    }

    /// `hyper.request(opts)`.
    pub(super) fn request(lua: &Lua, mut opts: RequestOptions) -> Result<Self, HyperError> {
        let common = opts.common();
        let method = normalize_method(&opts.method.0)?;
        let body = opts.body.map(|body| body.bytes(lua)).transpose()?;
        Self::new(method, opts.url.0, body, Some(common))
    }

    /// The request the options describe, after the check that the headers agree with the body.
    fn new(
        method: String,
        url: String,
        body: Option<Bytes>,
        common: Option<Common>,
    ) -> Result<Self, HyperError> {
        let mut limits = Limits::default();
        let (mut headers, mut ca_pem) = (HeaderMap::new(), None);
        if let Some(common) = common {
            headers = common.headers.map(|headers| headers.0).unwrap_or_default();
            ca_pem = common.ca_pem.map(|roots| roots.0);
            if let Some(connect) = common.connect_timeout {
                limits.connect = connect.0;
            }
            if let Some(idle) = common.idle_timeout {
                limits.idle = idle.0;
            }
            limits.total = common.timeout.map(|total| total.0);
            if let Some(max_body) = common.max_body {
                limits.max_body = max_body.0;
            }
            if let Some(redirects) = common.follow_redirects {
                limits.redirects = redirects.0;
            }
        }
        let prepared = Self {
            method,
            url,
            headers,
            body,
            ca_pem,
            limits,
        };
        prepared.check_content_length()?;
        Ok(prepared)
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

/// The headers a table of names and values says.
fn headers_of(table: &Table) -> Result<HeaderMap, Refused> {
    let mut pairs = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (name, value) = pair.map_err(|e| Refused(e.to_string()))?;
        // a key that is not a string is a mistake (`{ "Accept: text/plain" }` has the key 1), never a header named "1"
        let Value::String(name) = name else {
            return Err(Refused(format!(
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
                return Err(Refused(format!(
                    "the value of header `{name}` must be a string, got {}",
                    lua_type(&other)
                )));
            }
        };
        pairs.push((name, value));
    }
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        let header = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Refused(format!("`{name}` is not a valid header name")))?;
        let value = HeaderValue::from_bytes(&value)
            .map_err(|_| Refused(format!("the value of header `{name}` is not valid")))?;
        if header == TRANSFER_ENCODING {
            return Err(Refused(
                "the Transfer-Encoding header is set by the module; drop it".to_owned(),
            ));
        }
        if headers.contains_key(&header) {
            return Err(Refused(format!(
                "header `{header}` is given twice (header names are case-insensitive)"
            )));
        }
        headers.insert(header, value);
    }
    Ok(headers)
}

fn whole(n: f64) -> Option<u64> {
    // a float above 2^63 saturates, which is what "no limit worth naming" should mean
    (n.is_finite() && n >= 1.0 && n.fract() == 0.0).then_some(n as u64)
}

/// A number option as a float; anything that is not a number is an error.
fn number(name: &str, value: &Value, what: &str) -> Result<f64, Refused> {
    match value {
        Value::Integer(i) => Ok(*i as f64),
        Value::Number(n) => Ok(*n),
        other => Err(Refused(format!(
            "{name} must be {what}, got {}",
            lua_type(other)
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
