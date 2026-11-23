//! URL parsing, building, and query-string encoding — `lib:url`.
//!
//! `Url` UserData mirrors the WHATWG URL shape (component accessors, canonical
//! `tostring`) as an immutable handle: every `with_*` method returns a fresh
//! `Url` rather than mutating in place. Flat helpers (`parse`, `resolve`,
//! `encode`, `decode`, `encode_query`, `decode_query`) layer scripting
//! ergonomics on top of the UserData layer — they delegate, never re-decide.
//!
//! Backed by the `url` crate (parsing, building, `form_urlencoded`) and
//! `percent_encoding` (one-off encode/decode against WHATWG-aligned sets).

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};
use percent_encoding::{AsciiSet, CONTROLS, NON_ALPHANUMERIC, percent_decode_str, percent_encode};
use tracing::instrument;
use url::Url;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

// ---------------------------------------------------------------------------
// Percent-encode sets
// ---------------------------------------------------------------------------
//
// Names track the WHATWG URL spec § percent-encode sets. `COMPONENT` is the
// "encodeURIComponent" equivalent — RFC 3986 unreserved characters
// (`A-Za-z0-9-_.~`) pass through, everything else is encoded. It is the safe
// default for arbitrary string fragments and matches what Node / Deno /
// Python's `quote` produce.

const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');
const PATH: &AsciiSet = &QUERY.add(b'?').add(b'`').add(b'{').add(b'}');
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn set_for(name: &str) -> Option<&'static AsciiSet> {
    match name {
        "component" => Some(COMPONENT),
        "path" => Some(PATH),
        "query" => Some(QUERY),
        "fragment" => Some(FRAGMENT),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:url` surface through the facade.
///
/// The `Url` constructor sub-table and every flat function are set verbatim
/// with [`crate::lua::facade::Module::set`]: `url_parse_method` carries its own
/// `#[instrument(name = "url.Url.parse")]` span (which the facade's `type_`
/// constructor path could not reproduce), and each flat function carries its
/// own `#[instrument]` span plus a baked `"url.<fn>: "` prefix built through
/// `err` / `ok`. The `UrlHandle` userdata keeps its own `impl UserData` and is
/// recorded here via [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:url")?;

    let url_sub = lua.create_table()?;
    url_sub.set("parse", lua.create_function(url_parse_method)?)?;
    m.set("Url", url_sub)?;

    m.declared(flat_parse::lua(), lua.create_function(flat_parse)?)?;

    m.declared(flat_resolve::lua(), lua.create_function(flat_resolve)?)?;

    m.declared(flat_encode::lua(), lua.create_function(flat_encode)?)?;

    m.declared(flat_decode::lua(), lua.create_function(flat_decode)?)?;

    m.declared(
        flat_encode_query::lua(),
        lua.create_function(flat_encode_query)?,
    )?;

    m.declared(
        flat_decode_query::lua(),
        lua.create_function(flat_decode_query)?,
    )?;

    m.userdata("Url", |u| {
        UrlHandle::lua(u);
        u.static_of(url_parse_method::lua());
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Url UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct UrlHandle(pub Url);

#[neoc_macros::lua_userdata]
impl UserData for UrlHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        /// The scheme, lower-cased (`https`).
        #[lua("string")]
        fields.add_field_method_get("scheme", |_, this| Ok(this.0.scheme().to_owned()));
        /// The username, or an empty string.
        #[lua("string")]
        fields.add_field_method_get("username", |_, this| Ok(this.0.username().to_owned()));
        /// The password, or nil.
        #[lua("string?")]
        fields.add_field_method_get("password", |_, this| {
            Ok(this.0.password().map(str::to_owned))
        });
        /// The host, or nil for a URL without one.
        #[lua("string?")]
        fields.add_field_method_get("host", |_, this| Ok(this.0.host_str().map(str::to_owned)));
        /// The explicit port, or nil when it is the scheme's default.
        #[lua("number?")]
        fields.add_field_method_get("port", |_, this| Ok(this.0.port().map(|p| p as i64)));
        /// The percent-encoded path.
        #[lua("string")]
        fields.add_field_method_get("path", |_, this| Ok(this.0.path().to_owned()));
        /// The query string without the `?`, or nil.
        #[lua("string?")]
        fields.add_field_method_get("query", |_, this| Ok(this.0.query().map(str::to_owned)));
        /// The fragment without the `#`, or nil.
        #[lua("string?")]
        fields.add_field_method_get("fragment", |_, this| {
            Ok(this.0.fragment().map(str::to_owned))
        });
        /// The serialised origin (`https://example.com:8080`).
        #[lua("string")]
        fields.add_field_method_get("origin", |_, this| {
            Ok(this.0.origin().ascii_serialization())
        });
        /// The whole URL as a string.
        #[lua("string")]
        fields.add_field_method_get("href", |_, this| Ok(this.0.as_str().to_owned()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Resolve `reference` against this URL using RFC 3986 § 5 rules.
        #[lua("Result<Url, string>")]
        methods.add_method("resolve", |lua, this, reference: String| {
            join_url(lua, &this.0, &reference, "url.Url.resolve")
        });

        /// Return a new `Url` with the scheme replaced. Most special/non-special swaps are rejected by the spec.
        #[lua("Result<Url, string>")]
        methods.add_method("with_scheme", |lua, this, scheme: String| {
            let mut u = this.0.clone();
            match u.set_scheme(&scheme) {
                Ok(()) => ok(lua, UrlHandle(u)),
                Err(()) => err(
                    lua,
                    format!("url.Url.with_scheme: cannot set scheme {scheme:?}"),
                ),
            }
        });

        /// Return a new `Url` with the host replaced. Returns `(nil, err)` if the host is malformed for the scheme.
        #[lua("Result<Url, string>")]
        methods.add_method("with_host", |lua, this, host: String| {
            let mut u = this.0.clone();
            match u.set_host(Some(&host)) {
                Ok(()) => ok(lua, UrlHandle(u)),
                Err(e) => err(lua, format!("url.Url.with_host: {e}")),
            }
        });

        // Validate range and integer-ness here rather than leaning on mlua's
        // `FromLua<u16>` coercion. That coercion raises a raw type-conversion
        // error, which would breach the documented `(Url?, string?)` contract
        // of the `url` module — every other `with_*` honours it, so this one must
        // too. Floats with a fractional part are rejected for the same
        // reason: `with_port(8080.5)` silently truncating to 8080 is a
        // footgun, not a feature.
        /// Return a new `Url` with the port set (or cleared when `nil`). `port` must be an integer in `0..=65535`.
        #[lua("Result<Url, string>")]
        methods.add_method("with_port", |lua, this, port: Option<f64>| {
            let port = match port {
                None => None,
                Some(n) if !n.is_finite() || n.trunc() != n => {
                    return err(
                        lua,
                        format!("url.Url.with_port: port {n} is not an integer"),
                    );
                }
                Some(n) if !(0.0..=65535.0).contains(&n) => {
                    return err(
                        lua,
                        format!("url.Url.with_port: port {n} out of range (expected 0..=65535)"),
                    );
                }
                Some(n) => Some(n as u16),
            };
            let mut u = this.0.clone();
            match u.set_port(port) {
                Ok(()) => ok(lua, UrlHandle(u)),
                Err(()) => err(
                    lua,
                    "url.Url.with_port: cannot set port on this URL".to_string(),
                ),
            }
        });

        /// Return a new `Url` with the path replaced. Always succeeds — the path is re-encoded against the WHATWG path set.
        #[lua("Url")]
        methods.add_method("with_path", |_, this, path: String| {
            let mut u = this.0.clone();
            u.set_path(&path);
            Ok(UrlHandle(u))
        });

        /// Return a new `Url` with the query string replaced (or cleared when `nil`).
        #[lua("Url")]
        methods.add_method("with_query", |_, this, query: Option<String>| {
            let mut u = this.0.clone();
            u.set_query(query.as_deref());
            Ok(UrlHandle(u))
        });

        /// Return a new `Url` with the fragment replaced (or cleared when `nil`).
        #[lua("Url")]
        methods.add_method("with_fragment", |_, this, fragment: Option<String>| {
            let mut u = this.0.clone();
            u.set_fragment(fragment.as_deref());
            Ok(UrlHandle(u))
        });

        /// Query pairs in order, including any repeated keys.
        #[lua("LuaValue")]
        methods.add_method("query_pairs", |lua, this, _: ()| {
            let t = lua.create_table()?;
            for (i, (k, v)) in this.0.query_pairs().enumerate() {
                let pair = lua.create_table()?;
                pair.set(1, k.into_owned())?;
                pair.set(2, v.into_owned())?;
                t.set(i + 1, pair)?;
            }
            Ok(t)
        });

        // Map view of the query string. Repeated keys collapse to the **last**
        // value — `?a=1&a=2` → `{a = "2"}`. Matches the JavaScript
        // `Object.fromEntries(new URLSearchParams(…))` shape. Callers who need
        // every value should use `query_pairs()` instead.
        /// Query as a map. Last value wins on repeated keys.
        #[lua("LuaValue")]
        methods.add_method("query_map", |lua, this, _: ()| {
            let t = lua.create_table()?;
            for (k, v) in this.0.query_pairs() {
                t.set(k.into_owned(), v.into_owned())?;
            }
            Ok(t)
        });

        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<UrlHandle>()?.0.clone();
            let b = args.1.borrow::<UrlHandle>()?.0.clone();
            Ok(a == b)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(this.0.as_str().to_owned())
        });
    }
}

// ---------------------------------------------------------------------------
// Parse / resolve
// ---------------------------------------------------------------------------

/// Parse an absolute URL. Returns `(nil, err)` when it is invalid.
#[neoc_macros::lua_fn(name = "parse", returns = "Result<Url, string>")]
#[instrument(name = "url.Url.parse", target = "lua::lib::url", skip_all)]
fn url_parse_method(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Url::parse(&s) {
        Ok(u) => ok(lua, UrlHandle(u)),
        Err(e) => err(lua, format!("url.Url.parse: invalid url {s:?}: {e}")),
    }
}

/// Alias for `Url.parse`. Returns `(nil, err)` on malformed input.
#[neoc_macros::lua_fn(name = "parse", returns = "Result<Url, string>")]
#[instrument(name = "url.parse", target = "lua::lib::url", skip_all)]
fn flat_parse(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Url::parse(&s) {
        Ok(u) => ok(lua, UrlHandle(u)),
        Err(e) => err(lua, format!("url.parse: invalid url {s:?}: {e}")),
    }
}

/// Resolve a relative reference against `base` (a `Url` or string) using RFC 3986 § 5 rules.
#[neoc_macros::lua_fn(name = "resolve", returns = "Result<Url, string>")]
#[instrument(name = "url.resolve", target = "lua::lib::url", skip_all)]
fn flat_resolve(lua: &Lua, (base, reference): (Value, String)) -> mlua::Result<(Value, Value)> {
    let base_url = match coerce_to_url(base, "url.resolve") {
        Ok(u) => u,
        Err(msg) => return err(lua, msg),
    };
    join_url(lua, &base_url, &reference, "url.resolve")
}

fn join_url(lua: &Lua, base: &Url, reference: &str, prefix: &str) -> mlua::Result<(Value, Value)> {
    match base.join(reference) {
        Ok(u) => ok(lua, UrlHandle(u)),
        Err(e) => err(lua, format!("{prefix}: cannot resolve {reference:?}: {e}")),
    }
}

fn coerce_to_url(v: Value, prefix: &str) -> Result<Url, String> {
    match v {
        Value::String(s) => {
            let s = s
                .to_str()
                .map_err(|e| format!("{prefix}: {e}"))?
                .to_string();
            Url::parse(&s).map_err(|e| format!("{prefix}: invalid url {s:?}: {e}"))
        }
        Value::UserData(ud) => match ud.borrow::<UrlHandle>() {
            Ok(h) => Ok(h.0.clone()),
            Err(_) => Err(format!("{prefix}: expected Url or string")),
        },
        other => Err(format!(
            "{prefix}: expected Url or string, got {}",
            other.type_name()
        )),
    }
}

// ---------------------------------------------------------------------------
// Percent encode / decode
// ---------------------------------------------------------------------------

/// Percent-encode `input` against the named `set` (defaults to `component`). Returns `(nil, err)` only on an unknown set name.
#[neoc_macros::lua_fn(name = "encode", returns = "Result<string, string>")]
#[instrument(name = "url.encode", target = "lua::lib::url", skip_all)]
fn flat_encode(lua: &Lua, (input, set): (String, Option<String>)) -> mlua::Result<(Value, Value)> {
    let set_name = set.as_deref().unwrap_or("component");
    match set_for(set_name) {
        Some(ascii_set) => ok(lua, percent_encode(input.as_bytes(), ascii_set).to_string()),
        None => err(
            lua,
            format!(
                "url.encode: unknown set {set_name:?}: must be one of \"component\", \"path\", \"query\", \"fragment\""
            ),
        ),
    }
}

/// Percent-decode `input`. Returns `(nil, err)` when the decoded bytes are not valid UTF-8. Does not convert `+` to space.
#[neoc_macros::lua_fn(name = "decode", returns = "Result<string, string>")]
#[instrument(name = "url.decode", target = "lua::lib::url", skip_all)]
fn flat_decode(lua: &Lua, input: String) -> mlua::Result<(Value, Value)> {
    match percent_decode_str(&input).decode_utf8() {
        Ok(cow) => ok(lua, cow.into_owned()),
        Err(e) => err(lua, format!("url.decode: invalid utf-8: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Query string round-trip
// ---------------------------------------------------------------------------

/// Form-encode an ordered list of `{ key, value }` pairs into a query string. Spaces become `+`.
#[neoc_macros::lua_fn(name = "encode_query", returns = "Result<string, string>")]
#[instrument(name = "url.encode_query", target = "lua::lib::url", skip_all)]
fn flat_encode_query(lua: &Lua, pairs: Table) -> mlua::Result<(Value, Value)> {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    let len = pairs.raw_len();
    for i in 1..=len {
        let pair: Table = match pairs.raw_get(i) {
            Ok(p) => p,
            Err(e) => return err(lua, format!("url.encode_query: pair {i}: {e}")),
        };
        let k: String = match pair.raw_get(1) {
            Ok(k) => k,
            Err(e) => return err(lua, format!("url.encode_query: pair {i} key: {e}")),
        };
        let v: String = match pair.raw_get(2) {
            Ok(v) => v,
            Err(e) => return err(lua, format!("url.encode_query: pair {i} value: {e}")),
        };
        ser.append_pair(&k, &v);
    }
    ok(lua, ser.finish())
}

/// Parse an `application/x-www-form-urlencoded` payload into an ordered list of `{ key, value }` pairs. `+` decodes to space.
#[neoc_macros::lua_fn(name = "decode_query")]
#[instrument(name = "url.decode_query", target = "lua::lib::url", skip_all)]
fn flat_decode_query(lua: &Lua, input: String) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (i, (k, v)) in url::form_urlencoded::parse(input.as_bytes()).enumerate() {
        let pair = lua.create_table()?;
        pair.set(1, k.into_owned())?;
        pair.set(2, v.into_owned())?;
        t.set(i + 1, pair)?;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use std::hint::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("url");

    g.bench_function("library/parse", |b| {
        b.iter(|| {
            let _ = Url::parse(black_box(
                "https://user:pass@example.com:8443/a/b?x=1&y=2#frag",
            ))
            .unwrap();
        });
    });

    g.bench_function("library/encode_query", |b| {
        b.iter(|| {
            let mut s = url::form_urlencoded::Serializer::new(String::new());
            s.append_pair(black_box("name"), black_box("ada lovelace"));
            s.append_pair(black_box("role"), black_box("engineer/lead"));
            let _ = s.finish();
        });
    });

    let ident: mlua::Function = lua.create_function(|_, s: String| Ok(s)).expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: String = ident
                .call(black_box("https://example.com/".to_string()))
                .unwrap();
        });
    });

    let parse: mlua::Function = lua
        .load(r#"return function(s) return (require("lib:url")).parse(s) end"#)
        .eval()
        .expect("parse chunk");
    g.bench_function("through_lua/parse", |b| {
        b.iter(|| {
            let _: mlua::Value = parse
                .call(black_box(
                    "https://user:pass@example.com:8443/a/b?x=1&y=2#frag".to_string(),
                ))
                .unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:url")"#).eval().unwrap();
        lua.globals().set("url", m).unwrap();
        lua
    }

    #[test]
    fn parse_fields() {
        let lua = engine();
        lua.load(r#"u = url.parse("https://ada:secret@example.com:8443/a/b?x=1&y=2#frag")"#)
            .exec()
            .unwrap();
        let scheme: String = lua.load("return u.scheme").eval().unwrap();
        let user: String = lua.load("return u.username").eval().unwrap();
        let pass: Option<String> = lua.load("return u.password").eval().unwrap();
        let host: Option<String> = lua.load("return u.host").eval().unwrap();
        let port: Option<i64> = lua.load("return u.port").eval().unwrap();
        let path: String = lua.load("return u.path").eval().unwrap();
        let query: Option<String> = lua.load("return u.query").eval().unwrap();
        let frag: Option<String> = lua.load("return u.fragment").eval().unwrap();
        assert_eq!(scheme, "https");
        assert_eq!(user, "ada");
        assert_eq!(pass.as_deref(), Some("secret"));
        assert_eq!(host.as_deref(), Some("example.com"));
        assert_eq!(port, Some(8443));
        assert_eq!(path, "/a/b");
        assert_eq!(query.as_deref(), Some("x=1&y=2"));
        assert_eq!(frag.as_deref(), Some("frag"));
    }

    #[test]
    fn parse_default_port_is_nil() {
        let lua = engine();
        let port: Option<i64> = lua
            .load(r#"return (url.parse("https://example.com/")).port"#)
            .eval()
            .unwrap();
        assert!(port.is_none());
    }

    #[test]
    fn parse_invalid_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) =
            lua.load(r#"return url.parse("not a url")"#).eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("url.parse:"));
    }

    #[test]
    fn tostring_canonical_round_trip() {
        let lua = engine();
        let (s, eq_self): (String, bool) = lua
            .load(
                r#"
                local raw = "https://example.com/a%20b?x=1#frag"
                local u = url.parse(raw)
                return tostring(u), tostring(u) == u.href
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "https://example.com/a%20b?x=1#frag");
        assert!(eq_self);
    }

    #[test]
    fn url_equality_metamethod() {
        let lua = engine();
        let (eq, ne): (bool, bool) = lua
            .load(
                r#"
                local a = url.parse("https://example.com/a")
                local b = url.parse("https://example.com/a")
                local c = url.parse("https://example.com/b")
                return a == b, a == c
            "#,
            )
            .eval()
            .unwrap();
        assert!(eq);
        assert!(!ne);
    }

    #[test]
    fn with_methods_return_new_url() {
        let lua = engine();
        let (orig, modified): (String, String) = lua
            .load(
                r#"
                local u = url.parse("https://example.com/old")
                local v = u:with_path("/new"):with_query("a=1"):with_fragment("here")
                return tostring(u), tostring(v)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(orig, "https://example.com/old");
        assert_eq!(modified, "https://example.com/new?a=1#here");
    }

    #[test]
    fn with_port_clears_when_nil() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local u = url.parse("https://example.com:9999/")
                return tostring(u:with_port(nil))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "https://example.com/");
    }

    #[test]
    fn with_port_out_of_range_returns_err() {
        let lua = engine();
        let (high, low): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local u = url.parse("https://example.com/")
                local _, h_err = u:with_port(99999)
                local _, l_err = u:with_port(-1)
                return h_err, l_err
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            high.as_deref(),
            Some("url.Url.with_port: port 99999 out of range (expected 0..=65535)")
        );
        assert_eq!(
            low.as_deref(),
            Some("url.Url.with_port: port -1 out of range (expected 0..=65535)")
        );
    }

    #[test]
    fn with_port_non_integer_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(
                r#"
                local u = url.parse("https://example.com/")
                return u:with_port(8080.5)
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        assert_eq!(
            e.as_deref(),
            Some("url.Url.with_port: port 8080.5 is not an integer")
        );
    }

    #[test]
    fn with_port_valid_sets_port() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local u = url.parse("https://example.com/"):with_port(8443)
                return tostring(u)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "https://example.com:8443/");
    }

    #[test]
    fn with_scheme_invalid_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(
                r#"
                local u = url.parse("https://example.com/")
                return u:with_scheme("not a scheme")
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("url.Url.with_scheme:"));
    }

    #[test]
    fn resolve_method_and_flat() {
        let lua = engine();
        let (a, b, c): (String, String, String) = lua
            .load(
                r#"
                local base = url.parse("https://example.com/dir/page")
                return
                    tostring(base:resolve("other")),
                    tostring(url.resolve(base, "/abs")),
                    tostring(url.resolve("https://example.com/dir/page", "?q=1"))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, "https://example.com/dir/other");
        assert_eq!(b, "https://example.com/abs");
        assert_eq!(c, "https://example.com/dir/page?q=1");
    }

    #[test]
    fn query_pairs_preserve_order_and_repeats() {
        let lua = engine();
        let flat: Vec<String> = lua
            .load(
                r#"
                local u = url.parse("https://example.com/?a=1&b=2&a=3")
                local out = {}
                for _, p in ipairs(u:query_pairs()) do
                    out[#out + 1] = p[1] .. "=" .. p[2]
                end
                return out
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(flat, vec!["a=1", "b=2", "a=3"]);
    }

    #[test]
    fn query_map_last_wins() {
        let lua = engine();
        let (a, b): (String, String) = lua
            .load(
                r#"
                local m = (url.parse("https://example.com/?a=1&b=2&a=3")):query_map()
                return m.a, m.b
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, "3");
        assert_eq!(b, "2");
    }

    #[test]
    fn encode_component_default() {
        let lua = engine();
        let s: String = lua
            .load(r#"return url.encode("hello world!/?")"#)
            .eval()
            .unwrap();
        // RFC 3986 unreserved survives; everything else encoded.
        assert_eq!(s, "hello%20world%21%2F%3F");
    }

    #[test]
    fn encode_sets_differ() {
        let lua = engine();
        let (q, p, f): (String, String, String) = lua
            .load(
                r#"
                return
                    url.encode("a b\"c#d", "query"),
                    url.encode("a b?c{d}", "path"),
                    url.encode("a b<c>d", "fragment")
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(q, "a%20b%22c%23d");
        assert_eq!(p, "a%20b%3Fc%7Bd%7D");
        assert_eq!(f, "a%20b%3Cc%3Ed");
    }

    #[test]
    fn encode_unknown_set_errors() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(r#"return url.encode("x", "bogus")"#)
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("url.encode:"));
    }

    #[test]
    fn decode_round_trip() {
        let lua = engine();
        let s: String = lua
            .load(r#"return url.decode("hello%20world%21")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "hello world!");
    }

    #[test]
    fn decode_invalid_utf8_errors() {
        let lua = engine();
        // %FF on its own is not valid UTF-8.
        let (v, e): (Value, Option<String>) =
            lua.load(r#"return url.decode("%FF%FE")"#).eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("url.decode:"));
    }

    #[test]
    fn encode_query_basic() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                return url.encode_query({
                    {"name", "ada lovelace"},
                    {"role", "engineer/lead"},
                })
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "name=ada+lovelace&role=engineer%2Flead");
    }

    #[test]
    fn decode_query_pairs_order_preserved() {
        let lua = engine();
        let flat: Vec<String> = lua
            .load(
                r#"
                local out = {}
                for _, p in ipairs(url.decode_query("name=ada+lovelace&role=engineer%2Flead")) do
                    out[#out + 1] = p[1] .. "=" .. p[2]
                end
                return out
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(flat, vec!["name=ada lovelace", "role=engineer/lead"]);
    }

    #[test]
    fn query_round_trip() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local pairs_in = { {"a", "1 2"}, {"b", "c&d"}, {"e", "f=g"} }
                local s = url.encode_query(pairs_in)
                local pairs_out = url.decode_query(s)
                if #pairs_in ~= #pairs_out then return false end
                for i = 1, #pairs_in do
                    if pairs_in[i][1] ~= pairs_out[i][1] then return false end
                    if pairs_in[i][2] ~= pairs_out[i][2] then return false end
                end
                return true
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn origin_special_scheme() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (url.parse("https://example.com:8443/a")).origin"#)
            .eval()
            .unwrap();
        assert_eq!(s, "https://example.com:8443");
    }

    #[test]
    fn build_from_pieces_via_with() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local q = url.encode_query({ {"x", "1"}, {"y", "2"} })
                local u = url.parse("https://example.com/")
                    :with_path("/api/v1/items")
                    :with_query(q)
                    :with_fragment("top")
                return tostring(u)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "https://example.com/api/v1/items?x=1&y=2#top");
    }

    /// RUSTSEC-2026-0592: `url` 2.5.8 and earlier recursed once per `blob:` prefix in `Url::origin()`, so a script reading
    /// `.origin` of a deeply nested `blob:` URL overflowed the stack and aborted the host (`catch_unwind` cannot catch
    /// it). Run on a thread with tokio's default 2 MiB stack, the smallest a host thread polling a VM may have, so the
    /// old recursion would abort this test binary instead of passing.
    #[test]
    fn origin_of_a_deeply_nested_blob_url_is_opaque_and_does_not_overflow() {
        let nested = format!("{}https://example.com/", "blob:".repeat(10_000));
        let cases = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let lua = engine();
                lua.globals().set("nested", nested).unwrap();
                let origins: Vec<String> = lua
                    .load(
                        r#"
                        local out = {}
                        for _, s in ipairs({
                            nested,
                            "blob:https://example.com:8443/uuid",
                            "blob:blob:https://example.com/uuid",
                            "blob:ws://example.com/uuid",
                            "blob:not a url",
                            "data:text/plain,hi",
                            "file:///tmp/x",
                            "ws://example.com/",
                            "mailto:ada@example.com",
                        }) do
                            local u = assert(url.parse(s))
                            table.insert(out, u.origin)
                        end
                        return out
                        "#,
                    )
                    .eval()
                    .unwrap();
                origins
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(
            cases,
            [
                "null",
                "https://example.com:8443",
                "null",
                "null",
                "null",
                "null",
                "null",
                "ws://example.com",
                "null",
            ]
        );
    }
}
