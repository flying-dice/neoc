//! URL parsing, building and query-string encoding: `lib:url`.
//!
//! `Url` mirrors the WHATWG URL shape (component fields, a canonical `tostring`) as an immutable handle: every
//! `with_*` method returns a new `Url`. The flat helpers (`parse`, `resolve`, `encode`, `decode`, `encode_query`,
//! `decode_query`) sit on top of it. Backed by the `url` crate (parsing, building, `form_urlencoded`) and
//! `percent_encoding` (encoding and decoding against the WHATWG sets).
//!
//! Ported from the old VM's `lib:url`. What changed on purpose: `==` is `a:equals(b)` (operators are not supported),
//! the port is a `u16` (a value out of range or fractional is refused as an argument, not returned as an error), the
//! percent-encode set is a `"component" | "path" | "query" | "fragment"` union and `encode` cannot fail, query pairs
//! are `{ key, value }` records (they were `{ key, value }` arrays), `resolve` takes a `Url` or a string as a typed
//! union, and every failure is a typed error table.

use percent_encoding::{AsciiSet, CONTROLS, NON_ALPHANUMERIC};

// The names track the WHATWG URL spec's percent-encode sets. `COMPONENT` is "encodeURIComponent": the RFC 3986
// unreserved characters (`A-Za-z0-9-_.~`) pass through and everything else is encoded.
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');
const PATH: &AsciiSet = &QUERY.add(b'?').add(b'`').add(b'{').add(b'}');
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[mlua_ir::module("lib:url")]
pub mod url {
    use std::collections::BTreeMap;

    use mlua_ir::{LuaError, Object, OneOf, Shape};
    use percent_encoding::{percent_decode_str, percent_encode};
    use serde::{Deserialize, Serialize};

    use super::{COMPONENT, FRAGMENT, PATH, QUERY};

    /// Why a URL operation failed.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum UrlError {
        /// The text is not an absolute URL.
        #[error("invalid url {input:?}: {reason}")]
        InvalidUrl {
            /// The text as given.
            input: String,
            /// What is wrong with it.
            reason: String,
        },
        /// A reference could not be resolved against the base.
        #[error("cannot resolve {reference:?}: {reason}")]
        CannotResolve {
            /// The reference as given.
            reference: String,
            /// Why it cannot.
            reason: String,
        },
        /// The scheme is not valid, or the spec does not allow this URL's scheme to change to it (special and
        /// non-special schemes do not swap).
        #[error("cannot set scheme {scheme:?}")]
        InvalidScheme {
            /// The scheme as given.
            scheme: String,
        },
        /// The host is not valid for this URL.
        #[error("invalid host {host:?}: {reason}")]
        InvalidHost {
            /// The host as given.
            host: String,
            /// What is wrong with it.
            reason: String,
        },
        /// This URL cannot have a port (it has no host, or its scheme is `file`).
        #[error("cannot set a port on this URL")]
        CannotSetPort,
        /// The percent-decoded bytes are not UTF-8.
        #[error("the decoded bytes are not valid UTF-8: {reason}")]
        InvalidUtf8 {
            /// Where the bytes stop being UTF-8.
            reason: String,
        },
    }

    /// Which characters `encode` leaves alone: the WHATWG percent-encode sets.
    #[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Set {
        /// Everything but `A-Za-z0-9-_.~`: what `encodeURIComponent` does, safe for any string fragment.
        #[default]
        Component,
        /// What a URL path leaves alone: encodes control characters, space, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`.
        Path,
        /// What a query string leaves alone: encodes control characters, space, `"`, `#`, `<`, `>`.
        Query,
        /// What a fragment leaves alone: encodes control characters, space, `"`, `<`, `>`, `` ` ``.
        Fragment,
    }

    /// One `key=value` of a query string.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    pub struct QueryPair {
        /// The key, decoded.
        pub key: String,
        /// The value, decoded.
        pub value: String,
    }

    /// An absolute URL, immutable: the `with_*` methods return a changed copy. `tostring` gives the canonical
    /// form.
    #[derive(Debug, Clone, Object)]
    pub struct Url {
        inner: ::url::Url,
    }

    /// A base URL for `resolve`: a `Url`, or text that is one.
    #[derive(Debug, Clone, OneOf)]
    pub enum Base {
        /// A parsed URL.
        Url(Url),
        /// An absolute URL as text.
        Text(String),
    }

    fn parse_text(input: &str) -> Result<Url, UrlError> {
        ::url::Url::parse(input)
            .map(|inner| Url { inner })
            .map_err(|e| UrlError::InvalidUrl { input: input.to_owned(), reason: e.to_string() })
    }

    fn join(base: &::url::Url, reference: &str) -> Result<Url, UrlError> {
        base.join(reference)
            .map(|inner| Url { inner })
            .map_err(|e| UrlError::CannotResolve { reference: reference.to_owned(), reason: e.to_string() })
    }

    #[mlua_ir::object]
    impl Url {
        /// Parse an absolute URL.
        ///
        /// ```luau
        /// local u = assert(url.Url.parse("https://ada:secret@example.com:8443/a/b?x=1#frag"))
        /// assert(u.host == "example.com" and u.port == 8443 and u.query == "x=1")
        /// local _, err = url.Url.parse("not a url")
        /// assert(err ~= nil and err.kind == "invalid_url")
        /// ```
        pub fn parse(input: String) -> Result<Url, UrlError> {
            parse_text(&input)
        }

        /// The scheme, lower-cased (`https`).
        #[get]
        pub fn scheme(&self) -> String {
            self.inner.scheme().to_owned()
        }

        /// The username, or an empty string.
        #[get]
        pub fn username(&self) -> String {
            self.inner.username().to_owned()
        }

        /// The password, or `nil`.
        #[get]
        pub fn password(&self) -> Option<String> {
            self.inner.password().map(str::to_owned)
        }

        /// The host, or `nil` for a URL without one.
        #[get]
        pub fn host(&self) -> Option<String> {
            self.inner.host_str().map(str::to_owned)
        }

        /// The explicit port, or `nil` when it is the scheme's default.
        #[get]
        pub fn port(&self) -> Option<u16> {
            self.inner.port()
        }

        /// The percent-encoded path.
        #[get]
        pub fn path(&self) -> String {
            self.inner.path().to_owned()
        }

        /// The query string without the `?`, or `nil`.
        #[get]
        pub fn query(&self) -> Option<String> {
            self.inner.query().map(str::to_owned)
        }

        /// The fragment without the `#`, or `nil`.
        #[get]
        pub fn fragment(&self) -> Option<String> {
            self.inner.fragment().map(str::to_owned)
        }

        /// The serialised origin (`https://example.com:8443`), or `"null"` for an opaque one.
        #[get]
        pub fn origin(&self) -> String {
            self.inner.origin().ascii_serialization()
        }

        /// The whole URL as text.
        #[get]
        pub fn href(&self) -> String {
            self.inner.as_str().to_owned()
        }

        /// The canonical text.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.as_str().to_owned()
        }

        /// `true` when `other` is the same URL.
        pub fn equals(&self, other: Url) -> bool {
            self.inner == other.inner
        }

        /// Resolve `reference` against this URL by the RFC 3986 section 5 rules.
        ///
        /// ```luau
        /// local base = assert(url.parse("https://example.com/dir/page"))
        /// assert(tostring((assert(base:resolve("other")))) == "https://example.com/dir/other")
        /// ```
        pub fn resolve(&self, reference: String) -> Result<Url, UrlError> {
            join(&self.inner, &reference)
        }

        /// A copy with the scheme replaced. The spec refuses most swaps between special schemes (`https`, `ws`,
        /// ...) and the others.
        pub fn with_scheme(&self, scheme: String) -> Result<Url, UrlError> {
            let mut inner = self.inner.clone();
            inner.set_scheme(&scheme).map_err(|()| UrlError::InvalidScheme { scheme })?;
            Ok(Url { inner })
        }

        /// A copy with the host replaced.
        pub fn with_host(&self, host: String) -> Result<Url, UrlError> {
            let mut inner = self.inner.clone();
            inner.set_host(Some(&host)).map_err(|e| UrlError::InvalidHost { host, reason: e.to_string() })?;
            Ok(Url { inner })
        }

        /// A copy with the port replaced, or cleared when `nil`.
        pub fn with_port(&self, port: Option<u16>) -> Result<Url, UrlError> {
            let mut inner = self.inner.clone();
            inner.set_port(port).map_err(|()| UrlError::CannotSetPort)?;
            Ok(Url { inner })
        }

        /// A copy with the path replaced; it is re-encoded against the WHATWG path set.
        pub fn with_path(&self, path: String) -> Url {
            let mut inner = self.inner.clone();
            inner.set_path(&path);
            Url { inner }
        }

        /// A copy with the query string replaced, or cleared when `nil`.
        pub fn with_query(&self, query: Option<String>) -> Url {
            let mut inner = self.inner.clone();
            inner.set_query(query.as_deref());
            Url { inner }
        }

        /// A copy with the fragment replaced, or cleared when `nil`.
        pub fn with_fragment(&self, fragment: Option<String>) -> Url {
            let mut inner = self.inner.clone();
            inner.set_fragment(fragment.as_deref());
            Url { inner }
        }

        /// The query pairs in order, repeated keys included.
        pub fn query_pairs(&self) -> Vec<QueryPair> {
            self.inner
                .query_pairs()
                .map(|(key, value)| QueryPair { key: key.into_owned(), value: value.into_owned() })
                .collect()
        }

        /// The query as a map. A repeated key keeps its **last** value (`?a=1&a=2` is `{ a = "2" }`), like
        /// `Object.fromEntries(new URLSearchParams(...))`; use `query_pairs` to see every value.
        pub fn query_map(&self) -> BTreeMap<String, String> {
            self.inner.query_pairs().map(|(key, value)| (key.into_owned(), value.into_owned())).collect()
        }
    }

    /// Parse an absolute URL; the same as `Url.parse`.
    ///
    /// ```luau
    /// local u = assert(url.parse("https://example.com/a%20b?x=1"))
    /// assert(u.path == "/a%20b" and tostring(u) == "https://example.com/a%20b?x=1")
    /// ```
    pub fn parse(input: String) -> Result<Url, UrlError> {
        parse_text(&input)
    }

    /// Resolve a reference against `base` (a `Url` or text) by the RFC 3986 section 5 rules.
    ///
    /// ```luau
    /// assert(tostring((assert(url.resolve("https://example.com/dir/page", "?q=1")))) == "https://example.com/dir/page?q=1")
    /// ```
    pub fn resolve(base: Base, reference: String) -> Result<Url, UrlError> {
        let base = match base {
            Base::Url(url) => url,
            Base::Text(text) => parse_text(&text)?,
        };
        join(&base.inner, &reference)
    }

    /// Percent-encode `input` against `set` (`"component"` when not given).
    ///
    /// ```luau
    /// assert(url.encode("hello world!/?") == "hello%20world%21%2F%3F")
    /// assert(url.encode("a b?c", "path") == "a%20b%3Fc")
    /// ```
    pub fn encode(input: String, set: Option<Set>) -> String {
        let set = match set.unwrap_or_default() {
            Set::Component => COMPONENT,
            Set::Path => PATH,
            Set::Query => QUERY,
            Set::Fragment => FRAGMENT,
        };
        percent_encode(input.as_bytes(), set).to_string()
    }

    /// Percent-decode `input`. `+` stays `+` (it is a space only in a form-encoded query: see `decode_query`).
    /// The decoded bytes must be UTF-8.
    ///
    /// ```luau
    /// assert(assert(url.decode("hello%20world%21")) == "hello world!")
    /// ```
    pub fn decode(input: String) -> Result<String, UrlError> {
        percent_decode_str(&input)
            .decode_utf8()
            .map(std::borrow::Cow::into_owned)
            .map_err(|e| UrlError::InvalidUtf8 { reason: e.to_string() })
    }

    /// Form-encode query pairs into a query string, in order; a space is `+`.
    ///
    /// ```luau
    /// local q = url.encode_query({ { key = "name", value = "ada lovelace" }, { key = "role", value = "a/b" } })
    /// assert(q == "name=ada+lovelace&role=a%2Fb")
    /// ```
    pub fn encode_query(pairs: Vec<QueryPair>) -> String {
        let mut serializer = ::url::form_urlencoded::Serializer::new(String::new());
        for pair in &pairs {
            serializer.append_pair(&pair.key, &pair.value);
        }
        serializer.finish()
    }

    /// Parse an `application/x-www-form-urlencoded` payload into its pairs, in order; `+` is a space.
    ///
    /// ```luau
    /// local pairs = url.decode_query("name=ada+lovelace&role=a%2Fb")
    /// assert(pairs[1].value == "ada lovelace" and pairs[2].value == "a/b")
    /// ```
    pub fn decode_query(input: String) -> Vec<QueryPair> {
        ::url::form_urlencoded::parse(input.as_bytes())
            .map(|(key, value)| QueryPair { key: key.into_owned(), value: value.into_owned() })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:url")"#).eval().unwrap();
        lua.globals().set("url", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    #[test]
    fn parse_fields() {
        let lua = engine();
        lua.load(r#"u = assert(url.parse("https://ada:secret@example.com:8443/a/b?x=1&y=2#frag"))"#)
            .exec()
            .unwrap();
        let get = |field: &str| -> mlua::Value { lua.load(format!("return u.{field}")).eval().unwrap() };
        let text = |field: &str| -> String { lua.load(format!("return u.{field}")).eval().unwrap() };
        assert_eq!(text("scheme"), "https");
        assert_eq!(text("username"), "ada");
        assert_eq!(text("password"), "secret");
        assert_eq!(text("host"), "example.com");
        assert_eq!(get("port").as_integer(), Some(8443));
        assert_eq!(text("path"), "/a/b");
        assert_eq!(text("query"), "x=1&y=2");
        assert_eq!(text("fragment"), "frag");
        assert_eq!(text("origin"), "https://example.com:8443");
        assert_eq!(text("href"), "https://ada:secret@example.com:8443/a/b?x=1&y=2#frag");
    }

    #[test]
    fn absent_components_are_nil() {
        let (port, password, query, fragment): (mlua::Value, mlua::Value, mlua::Value, mlua::Value) = eval(
            r#"local u = assert(url.parse("https://example.com/")) return u.port, u.password, u.query, u.fragment"#,
        );
        assert!(port.is_nil() && password.is_nil() && query.is_nil() && fragment.is_nil());
        assert_eq!(eval::<String>(r#"return assert(url.parse("https://example.com/")).username"#), "");
    }

    #[test]
    fn parse_invalid_is_a_typed_error() {
        let (value, kind, op, input, text): (mlua::Value, String, String, String, String) =
            eval(r#"local v, e = url.parse("not a url") return v, e.kind, e.op, e.input, tostring(e)"#);
        assert!(value.is_nil());
        assert_eq!(
            (kind.as_str(), op.as_str(), input.as_str()),
            ("invalid_url", "lib:url.parse", "not a url")
        );
        assert!(text.starts_with("lib:url.parse: invalid_url:"), "{text}");
        assert_eq!(eval::<String>(r#"local _, e = url.Url.parse("x") return e.op"#), "lib:url.Url.parse");
    }

    #[test]
    fn tostring_is_canonical_and_equals_href() {
        let (s, same): (String, bool) = eval(
            r#"
            local u = assert(url.parse("https://example.com/a%20b?x=1#frag"))
            return tostring(u), tostring(u) == u.href
        "#,
        );
        assert_eq!(s, "https://example.com/a%20b?x=1#frag");
        assert!(same);
    }

    #[test]
    fn equals_replaces_the_eq_operator() {
        let (eq, ne): (bool, bool) = eval(
            r#"
            local a = assert(url.parse("https://example.com/a"))
            local b = assert(url.parse("https://example.com/a"))
            local c = assert(url.parse("https://example.com/b"))
            return a:equals(b), a:equals(c)
        "#,
        );
        assert!(eq && !ne);
    }

    #[test]
    fn with_methods_return_a_new_url() {
        let (orig, modified): (String, String) = eval(
            r#"
            local u = assert(url.parse("https://example.com/old"))
            local v = u:with_path("/new"):with_query("a=1"):with_fragment("here")
            return tostring(u), tostring(v)
        "#,
        );
        assert_eq!(orig, "https://example.com/old");
        assert_eq!(modified, "https://example.com/new?a=1#here");
        let cleared: String = eval(
            r#"return tostring(assert(url.parse("https://example.com/?a=1#f")):with_query(nil):with_fragment(nil))"#,
        );
        assert_eq!(cleared, "https://example.com/");
    }

    #[test]
    fn with_port_sets_and_clears() {
        assert_eq!(
            eval::<String>(
                r#"return tostring(assert(assert(url.parse("https://example.com/")):with_port(8443)))"#
            ),
            "https://example.com:8443/"
        );
        assert_eq!(
            eval::<String>(
                r#"return tostring(assert(assert(url.parse("https://example.com:9999/")):with_port(nil)))"#
            ),
            "https://example.com/"
        );
    }

    #[test]
    fn with_port_out_of_range_or_fractional_is_refused_as_an_argument() {
        for port in ["99999", "-1", "8080.5", "0/0"] {
            let err =
                refused(&format!(r#"return assert(url.parse("https://example.com/")):with_port({port})"#));
            assert!(err.contains("lib:url.Url:with_port: argument #1 'port'"), "{port}: {err}");
        }
    }

    #[test]
    fn with_port_on_a_url_without_a_host_is_a_typed_error() {
        let (kind, op): (String, String) = eval(
            r#"local _, e = assert(url.parse("mailto:ada@example.com")):with_port(25) return e.kind, e.op"#,
        );
        assert_eq!((kind.as_str(), op.as_str()), ("cannot_set_port", "lib:url.Url:with_port"));
    }

    #[test]
    fn with_scheme_and_host_errors_are_typed() {
        let (kind, scheme): (String, String) = eval(
            r#"local _, e = assert(url.parse("https://example.com/")):with_scheme("not a scheme") return e.kind, e.scheme"#,
        );
        assert_eq!((kind.as_str(), scheme.as_str()), ("invalid_scheme", "not a scheme"));
        let (kind, host): (String, String) = eval(
            r#"local _, e = assert(url.parse("https://example.com/")):with_host("exa mple.com") return e.kind, e.host"#,
        );
        assert_eq!((kind.as_str(), host.as_str()), ("invalid_host", "exa mple.com"));
        assert_eq!(
            eval::<String>(
                r#"return tostring(assert(assert(url.parse("https://example.com/")):with_host("other.org")))"#
            ),
            "https://other.org/"
        );
        assert_eq!(
            eval::<String>(
                r#"return tostring(assert(assert(url.parse("http://example.com/")):with_scheme("https")))"#
            ),
            "https://example.com/"
        );
    }

    #[test]
    fn resolve_method_and_flat_with_a_url_or_text() {
        let (a, b, c, d): (String, String, String, String) = eval(
            r#"
            local base = assert(url.parse("https://example.com/dir/page"))
            return tostring(assert(base:resolve("other"))),
                   tostring(assert(url.resolve(base, "/abs"))),
                   tostring(assert(url.resolve("https://example.com/dir/page", "?q=1"))),
                   tostring(assert(url.resolve("https://example.com/dir/page", "../up")))
        "#,
        );
        assert_eq!(a, "https://example.com/dir/other");
        assert_eq!(b, "https://example.com/abs");
        assert_eq!(c, "https://example.com/dir/page?q=1");
        assert_eq!(d, "https://example.com/up");
    }

    #[test]
    fn resolve_errors_are_typed() {
        let (kind, reference): (String, String) =
            eval(r#"local _, e = url.resolve("mailto:ada@example.com", "x") return e.kind, e.reference"#);
        assert_eq!((kind.as_str(), reference.as_str()), ("cannot_resolve", "x"));
        let kind: String = eval(r#"local _, e = url.resolve("not a url", "x") return e.kind"#);
        assert_eq!(kind, "invalid_url");
        let err = refused("return url.resolve(1, 'x')");
        assert!(err.contains("lib:url.resolve: argument #1 'base'"), "{err}");
    }

    #[test]
    fn query_pairs_preserve_order_and_repeats() {
        let flat: Vec<String> = eval(
            r#"
            local u = assert(url.parse("https://example.com/?a=1&b=2&a=3"))
            local out = {}
            for i, p in ipairs(u:query_pairs()) do out[i] = p.key .. "=" .. p.value end
            return out
        "#,
        );
        assert_eq!(flat, ["a=1", "b=2", "a=3"]);
    }

    #[test]
    fn query_map_keeps_the_last_value() {
        let (a, b): (String, String) = eval(
            r#"
            local m = assert(url.parse("https://example.com/?a=1&b=2&a=3")):query_map()
            return m.a, m.b
        "#,
        );
        assert_eq!((a.as_str(), b.as_str()), ("3", "2"));
    }

    #[test]
    fn encode_component_by_default() {
        // The RFC 3986 unreserved characters survive; everything else is encoded.
        assert_eq!(eval::<String>(r#"return url.encode("hello world!/?")"#), "hello%20world%21%2F%3F");
        assert_eq!(eval::<String>(r#"return url.encode("a-b_c.d~e")"#), "a-b_c.d~e");
    }

    #[test]
    fn encode_sets_differ() {
        let (q, p, f): (String, String, String) = eval(
            r#"
            return url.encode("a b\"c#d", "query"), url.encode("a b?c{d}", "path"), url.encode("a b<c>d", "fragment")
        "#,
        );
        assert_eq!(q, "a%20b%22c%23d");
        assert_eq!(p, "a%20b%3Fc%7Bd%7D");
        assert_eq!(f, "a%20b%3Cc%3Ed");
    }

    #[test]
    fn encode_set_is_a_declared_union() {
        let err = refused(r#"return url.encode("x", "bogus")"#);
        assert!(err.contains("lib:url.encode: argument #2 'set'"), "{err}");
    }

    #[test]
    fn decode_round_trip_and_plus() {
        assert_eq!(eval::<String>(r#"return (url.decode("hello%20world%21"))"#), "hello world!");
        assert_eq!(eval::<String>(r#"return (url.decode("a+b"))"#), "a+b");
    }

    #[test]
    fn decode_invalid_utf8_is_a_typed_error() {
        let (value, kind, op): (mlua::Value, String, String) =
            eval(r#"local v, e = url.decode("%FF%FE") return v, e.kind, e.op"#);
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), op.as_str()), ("invalid_utf8", "lib:url.decode"));
    }

    #[test]
    fn encode_and_decode_query() {
        assert_eq!(
            eval::<String>(
                r#"return url.encode_query({ { key = "name", value = "ada lovelace" }, { key = "role", value = "engineer/lead" } })"#
            ),
            "name=ada+lovelace&role=engineer%2Flead"
        );
        let flat: Vec<String> = eval(
            r#"
            local out = {}
            for i, p in ipairs(url.decode_query("name=ada+lovelace&role=engineer%2Flead")) do
                out[i] = p.key .. "=" .. p.value
            end
            return out
        "#,
        );
        assert_eq!(flat, ["name=ada lovelace", "role=engineer/lead"]);
    }

    #[test]
    fn query_round_trip() {
        assert!(eval::<bool>(
            r#"
            local pairs_in = { { key = "a", value = "1 2" }, { key = "b", value = "c&d" }, { key = "e", value = "f=g" } }
            local pairs_out = url.decode_query(url.encode_query(pairs_in))
            if #pairs_in ~= #pairs_out then return false end
            for i = 1, #pairs_in do
                if pairs_in[i].key ~= pairs_out[i].key or pairs_in[i].value ~= pairs_out[i].value then return false end
            end
            return true
        "#
        ));
    }

    #[test]
    fn encode_query_refuses_malformed_pairs() {
        let err = refused(r#"return url.encode_query({ { "a", "b" } })"#);
        assert!(err.contains("lib:url.encode_query: argument #1 'pairs'"), "{err}");
    }

    #[test]
    fn origin_of_the_special_schemes() {
        assert_eq!(
            eval::<String>(r#"return assert(url.parse("https://example.com:8443/a")).origin"#),
            "https://example.com:8443"
        );
    }

    #[test]
    fn build_from_pieces_via_with() {
        let s: String = eval(
            r#"
            local q = url.encode_query({ { key = "x", value = "1" }, { key = "y", value = "2" } })
            local u = assert(url.parse("https://example.com/")):with_path("/api/v1/items"):with_query(q):with_fragment("top")
            return tostring(u)
        "#,
        );
        assert_eq!(s, "https://example.com/api/v1/items?x=1&y=2#top");
    }

    /// RUSTSEC-2026-0592: `url` 2.5.8 and earlier recursed once per `blob:` prefix in `Url::origin()`, so a script
    /// reading `.origin` of a deeply nested `blob:` URL overflowed the stack and aborted the host (`catch_unwind`
    /// cannot catch it). Run on a thread with tokio's default 2 MiB stack, the smallest a host thread polling a VM
    /// may have, so the old recursion would abort this test binary instead of passing.
    #[test]
    fn origin_of_a_deeply_nested_blob_url_is_opaque_and_does_not_overflow() {
        let nested = format!("{}https://example.com/", "blob:".repeat(10_000));
        let origins = std::thread::Builder::new()
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
                            table.insert(out, assert(url.parse(s)).origin)
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
            origins,
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
