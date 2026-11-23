//! Semantic version parsing, comparison, and range matching — `lib:semver`.
//!
//! Dual surface: `Version` / `VersionReq` UserData (parsed, reusable) plus flat
//! `parse` / `valid` / `coerce` / `compare` / `satisfies` / `increment` /
//! `format` convenience functions. Backed by the `semver` crate, which
//! implements **Cargo-compatible range syntax** (`^`, `~`, `>=`, `<`, `=`,
//! comma-AND). npm extensions (`||`, hyphen ranges, x-ranges) are out of
//! scope.

use std::cmp::Ordering;
use std::sync::OnceLock;

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};
use regex::Regex;
use semver::{BuildMetadata, Prerelease, Version, VersionReq};
use tracing::instrument;

use neoc_macros::lua_userdata;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:semver` surface through the facade.
///
/// The `Version` / `VersionReq` constructor sub-tables and every flat function
/// are set verbatim with [`crate::lua::facade::Module::set`]: the parse
/// constructors carry their own `#[instrument(name = "semver.Version.parse")]`
/// / `"semver.VersionReq.parse"` spans (which the facade's `type_` constructor
/// path could not reproduce), and each flat function carries its own
/// `#[instrument]` span plus a baked `"semver.<fn>: "` prefix built through
/// `err` / `ok`. The `VersionHandle` / `VersionReqHandle` userdata keep their
/// own `impl UserData` and are recorded here via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:semver")?;

    let version_sub = lua.create_table()?;
    version_sub.set("parse", lua.create_function(version_parse_method)?)?;
    m.set("Version", version_sub)?;

    let req_sub = lua.create_table()?;
    req_sub.set("parse", lua.create_function(req_parse_method)?)?;
    m.set("VersionReq", req_sub)?;

    m.set("parse", lua.create_function(flat_parse)?)?;
    m.record_fn(
        "parse",
        &[("s", "string")],
        "Result<Version, string>",
        "Alias for `Version.parse`. Returns `(nil, err)` on malformed input.",
    );

    m.set("valid", lua.create_function(flat_valid)?)?;
    m.record_fn(
        "valid",
        &[("s", "string")],
        "bool",
        "True when `s` parses as a strict SemVer 2.0.0 version. Never errors — the predicate form of `parse`.",
    );

    m.set("coerce", lua.create_function(flat_coerce)?)?;
    m.record_fn(
        "coerce",
        &[("s", "string")],
        "Result<Version, string>",
        "Loose extraction: strips a leading `v`/`V`, fills missing minor/patch with `0`, drops build metadata and fourth components.",
    );

    m.set("compare", lua.create_function(flat_compare)?)?;
    m.record_fn(
        "compare",
        &[("a", "LuaValue"), ("b", "LuaValue")],
        "Result<number, string>",
        "Three-way precedence compare returning `-1`, `0`, or `1`. Both args may be a `Version` or a string.",
    );

    m.set("satisfies", lua.create_function(flat_satisfies)?)?;
    m.record_fn(
        "satisfies",
        &[("version", "LuaValue"), ("range", "LuaValue")],
        "Result<bool, string>",
        "True when `version` is in `range`. Returns `(nil, err)` only on a malformed range or version, never on a clean no-match.",
    );

    m.set("increment", lua.create_function(flat_increment)?)?;
    m.record_fn(
        "increment",
        &[("version", "string"), ("level", "string")],
        "Result<string, string>",
        "Increment `version` by `level` (`major`/`minor`/`patch`/`prerelease`) and return a SemVer 2.0.0 string; build metadata is stripped.",
    );

    m.set("format", lua.create_function(flat_format)?)?;
    m.record_fn(
        "format",
        &[("v", "LuaValue")],
        "Result<string, string>",
        "Canonical SemVer 2.0.0 string for `v` (a `Version` or string). Equivalent to `tostring(v)`.",
    );

    m.userdata("Version", |v| {
        VersionHandle::lua(v);
        v.static_fn(
            "parse",
            &[("s", "string")],
            "Result<Version, string>",
            "Parse a strict SemVer 2.0.0 version. Returns `(nil, err)` on malformed input.",
        );
    });

    m.userdata("VersionReq", |r| {
        VersionReqHandle::lua(r);
        r.static_fn(
            "parse",
            &[("s", "string")],
            "Result<VersionReq, string>",
            "Parse a requirement such as `\">=1.2, <2\"`; `(nil, err)` when malformed.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Version UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VersionHandle(pub Version);

#[lua_userdata]
impl UserData for VersionHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        /// Major version.
        #[lua("number")]
        fields.add_field_method_get("major", |_, this| Ok(this.0.major as i64));
        /// Minor version.
        #[lua("number")]
        fields.add_field_method_get("minor", |_, this| Ok(this.0.minor as i64));
        /// Patch version.
        #[lua("number")]
        fields.add_field_method_get("patch", |_, this| Ok(this.0.patch as i64));
        /// Pre-release identifiers, or nil when there are none.
        #[lua("string?")]
        fields.add_field_method_get("pre", |_, this| {
            Ok(if this.0.pre.is_empty() {
                None
            } else {
                Some(this.0.pre.as_str().to_owned())
            })
        });
        /// Build metadata, or nil when there is none.
        #[lua("string?")]
        fields.add_field_method_get("build", |_, this| {
            Ok(if this.0.build.is_empty() {
                None
            } else {
                Some(this.0.build.as_str().to_owned())
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Three-way compare returning `-1`, `0`, or `1`. Accepts another `Version` or a string (raises if invalid).
        #[lua("number")]
        methods.add_method("compare", |_, this, other: Value| {
            let v = coerce_to_version(other, "semver.Version.compare")
                .map_err(mlua::Error::external)?;
            // `cmp_precedence` honours SemVer 2.0.0 §10 (build metadata ignored
            // in precedence). The default `cmp` includes build to stay
            // consistent with the derived `Eq` — that's the wrong choice for
            // our scripting surface.
            Ok(ordering_to_i64(this.0.cmp_precedence(&v)))
        });

        /// Return a new `Version` incremented per `level`. The input is not mutated.
        #[lua("Version")]
        methods.add_method("increment", |_, this, level: String| {
            let v = increment_version(&this.0, &level)
                .map_err(|m| mlua::Error::external(format!("semver.Version.increment: {m}")))?;
            Ok(VersionHandle(v))
        });

        // Comparison metamethods. We use `add_meta_function` (not
        // `add_meta_method`) because `table.sort` may pass the same userdata
        // as both args — `add_meta_method`'s implicit `&self` borrow would
        // overlap the inner `other.borrow()` and mlua rejects that. Plain
        // functions let each borrow live in its own statement.
        //
        // `cmp_precedence` honours SemVer 2.0.0 §10 (build metadata ignored
        // in precedence); the default `cmp` keeps build for `Eq`-consistency,
        // which is the wrong tradeoff for a scripting surface.
        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            compare_userdata(args).map(|o| o == Ordering::Less)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            compare_userdata(args).map(|o| o != Ordering::Greater)
        });
        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            compare_userdata(args).map(|o| o == Ordering::Equal)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(this.0.to_string())
        });
    }
}

#[instrument(name = "semver.Version.parse", target = "lua::lib::semver", skip_all)]
fn version_parse_method(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match parse_version(&s, "semver.Version.parse") {
        Ok(v) => ok(lua, VersionHandle(v)),
        Err(msg) => err(lua, msg),
    }
}

// ---------------------------------------------------------------------------
// VersionReq UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VersionReqHandle(pub VersionReq);

#[lua_userdata]
impl UserData for VersionReqHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// True when `version` is in the range. Accepts a `Version` or a string (raises if invalid).
        #[lua("bool")]
        methods.add_method("matches", |_, this, version: Value| {
            let v = coerce_to_version(version, "semver.VersionReq.matches")
                .map_err(mlua::Error::external)?;
            Ok(this.0.matches(&v))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(this.0.to_string())
        });
    }
}

#[instrument(
    name = "semver.VersionReq.parse",
    target = "lua::lib::semver",
    skip_all
)]
fn req_parse_method(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match parse_req(&s, "semver.VersionReq.parse") {
        Ok(r) => ok(lua, VersionReqHandle(r)),
        Err(msg) => err(lua, msg),
    }
}

// ---------------------------------------------------------------------------
// Flat functions
// ---------------------------------------------------------------------------

#[instrument(name = "semver.parse", target = "lua::lib::semver", skip_all)]
fn flat_parse(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match parse_version(&s, "semver.parse") {
        Ok(v) => ok(lua, VersionHandle(v)),
        Err(msg) => err(lua, msg),
    }
}

#[instrument(name = "semver.valid", target = "lua::lib::semver", skip_all)]
fn flat_valid(_: &Lua, s: String) -> mlua::Result<bool> {
    Ok(Version::parse(&s).is_ok())
}

#[instrument(name = "semver.coerce", target = "lua::lib::semver", skip_all)]
fn flat_coerce(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match coerce_version(&s) {
        Some(v) => ok(lua, VersionHandle(v)),
        None => err(lua, format!("semver.coerce: no version found in {s:?}")),
    }
}

#[instrument(name = "semver.compare", target = "lua::lib::semver", skip_all)]
fn flat_compare(lua: &Lua, (a, b): (Value, Value)) -> mlua::Result<(Value, Value)> {
    let av = match coerce_to_version(a, "semver.compare") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let bv = match coerce_to_version(b, "semver.compare") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    ok(lua, ordering_to_i64(av.cmp_precedence(&bv)))
}

#[instrument(name = "semver.satisfies", target = "lua::lib::semver", skip_all)]
fn flat_satisfies(lua: &Lua, (version, range): (Value, Value)) -> mlua::Result<(Value, Value)> {
    let v = match coerce_to_version(version, "semver.satisfies") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let r = match coerce_to_req(range, "semver.satisfies") {
        Ok(r) => r,
        Err(msg) => return err(lua, msg),
    };
    ok(lua, r.matches(&v))
}

#[instrument(name = "semver.increment", target = "lua::lib::semver", skip_all)]
fn flat_increment(lua: &Lua, (version, level): (String, String)) -> mlua::Result<(Value, Value)> {
    let v = match parse_version(&version, "semver.increment") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    match increment_version(&v, &level) {
        Ok(v2) => ok(lua, v2.to_string()),
        Err(msg) => err(lua, format!("semver.increment: {msg}")),
    }
}

#[instrument(name = "semver.format", target = "lua::lib::semver", skip_all)]
fn flat_format(lua: &Lua, version: Value) -> mlua::Result<(Value, Value)> {
    match coerce_to_version(version, "semver.format") {
        Ok(v) => ok(lua, v.to_string()),
        Err(msg) => err(lua, msg),
    }
}

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

fn parse_version(s: &str, prefix: &str) -> Result<Version, String> {
    Version::parse(s).map_err(|e| format!("{prefix}: invalid version {s:?}: {e}"))
}

fn parse_req(s: &str, prefix: &str) -> Result<VersionReq, String> {
    VersionReq::parse(s).map_err(|e| format!("{prefix}: invalid range {s:?}: {e}"))
}

/// Accept either a Lua string (parsed via `parser`) or a UserData of type `H`
/// (cloned out via `unwrap`). Any other Lua value is a type error.
/// `noun` names what the string is ("version", "range"); `type_name` names
/// the userdata type ("Version", "VersionReq"). Both surface verbatim in
/// error messages.
fn coerce_with<T, H, P, U>(
    v: Value,
    prefix: &str,
    noun: &str,
    type_name: &str,
    parser: P,
    unwrap: U,
) -> Result<T, String>
where
    H: UserData + 'static,
    P: FnOnce(&str) -> Result<T, semver::Error>,
    U: FnOnce(&H) -> T,
{
    match v {
        Value::String(s) => {
            let s = s
                .to_str()
                .map_err(|e| format!("{prefix}: {e}"))?
                .to_string();
            parser(&s).map_err(|e| format!("{prefix}: invalid {noun} {s:?}: {e}"))
        }
        Value::UserData(ud) => match ud.borrow::<H>() {
            Ok(h) => Ok(unwrap(&h)),
            Err(_) => Err(format!("{prefix}: expected {type_name} or string")),
        },
        other => Err(format!(
            "{prefix}: expected {type_name} or string, got {}",
            other.type_name()
        )),
    }
}

fn coerce_to_version(v: Value, prefix: &str) -> Result<Version, String> {
    coerce_with::<_, VersionHandle, _, _>(v, prefix, "version", "Version", Version::parse, |h| {
        h.0.clone()
    })
}

fn coerce_to_req(v: Value, prefix: &str) -> Result<VersionReq, String> {
    coerce_with::<_, VersionReqHandle, _, _>(
        v,
        prefix,
        "range",
        "VersionReq",
        VersionReq::parse,
        |h| h.0.clone(),
    )
}

/// Borrow both UserData operands of a comparison metamethod, clone the inner
/// `Version`, and return the precedence-aware ordering. Centralises the
/// dance the three `Lt`/`Le`/`Eq` callbacks share.
fn compare_userdata(args: (AnyUserData, AnyUserData)) -> mlua::Result<Ordering> {
    let av = args.0.borrow::<VersionHandle>()?.0.clone();
    let bv = args.1.borrow::<VersionHandle>()?.0.clone();
    Ok(av.cmp_precedence(&bv))
}

fn ordering_to_i64(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Increment `v` by `level` ("major" | "minor" | "patch" | "prerelease").
/// Always returns a new `Version`; build metadata is stripped per spec.
fn increment_version(v: &Version, level: &str) -> Result<Version, String> {
    let mut out = v.clone();
    out.build = BuildMetadata::EMPTY;
    match level {
        "major" => {
            out.major = bump_u64(out.major, "major")?;
            out.minor = 0;
            out.patch = 0;
            out.pre = Prerelease::EMPTY;
        }
        "minor" => {
            out.minor = bump_u64(out.minor, "minor")?;
            out.patch = 0;
            out.pre = Prerelease::EMPTY;
        }
        "patch" => inc_patch(&mut out)?,
        "prerelease" => inc_prerelease(&mut out)?,
        other => {
            return Err(format!(
                "invalid level {other:?}: must be one of \"major\", \"minor\", \"patch\", \"prerelease\""
            ));
        }
    }
    Ok(out)
}

fn bump_u64(n: u64, name: &str) -> Result<u64, String> {
    n.checked_add(1).ok_or_else(|| format!("{name} overflow"))
}

/// Patch increment, SemVer 2.0.0 §11. Dropping a pre-release on `1.2.3-X`
/// already lands on the next patch (`1.2.3`) — no numeric bump needed.
fn inc_patch(v: &mut Version) -> Result<(), String> {
    if v.pre.is_empty() {
        v.patch = bump_u64(v.patch, "patch")?;
    } else {
        v.pre = Prerelease::EMPTY;
    }
    Ok(())
}

/// Pre-release increment. Empty pre starts a fresh `-0` on the next patch;
/// otherwise we bump the trailing numeric identifier (or append `.0` if the
/// pre has no numeric tail).
fn inc_prerelease(v: &mut Version) -> Result<(), String> {
    if v.pre.is_empty() {
        v.patch = bump_u64(v.patch, "patch")?;
        v.pre = Prerelease::new("0").map_err(|e| format!("prerelease build failed: {e}"))?;
    } else {
        v.pre = bump_prerelease(v.pre.as_str())?;
    }
    Ok(())
}

/// Bump the trailing dotted identifier of `pre`: if it is numeric, increment
/// it; otherwise append `.0`. Matches Node's `semver` behaviour — only the
/// last identifier is inspected, so `1.2.3-0.alpha` → `1.2.3-0.alpha.0`
/// (alpha tail triggers append, not the leading numeric).
fn bump_prerelease(pre: &str) -> Result<Prerelease, String> {
    // `str::split` always yields at least one element, and `inc_prerelease`
    // only calls us when `pre` is non-empty.
    let mut parts: Vec<String> = pre.split('.').map(|s| s.to_string()).collect();
    let last_idx = parts.len() - 1;
    let last = &parts[last_idx];
    let new_pre = if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
        let n: u64 = last
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let bumped = n
            .checked_add(1)
            .ok_or_else(|| "prerelease overflow".to_string())?;
        parts[last_idx] = bumped.to_string();
        parts.join(".")
    } else {
        format!("{pre}.0")
    };
    Prerelease::new(&new_pre).map_err(|e| e.to_string())
}

/// Loose version extraction. See module docs for the contract; pre-release is
/// preserved when present, build metadata is dropped, extra version components
/// (`1.2.3.4` → `1.2.3`) are truncated.
fn coerce_version(input: &str) -> Option<Version> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Boundary char (start-of-string or non-digit) before `major` so we
        // don't grab a digit from the middle of an alphanumeric run; `pre`
        // identifier chars are SemVer 2.0.0 §9 (no leading dot, dot-separated).
        Regex::new(r"(?:^|[^0-9])(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([0-9A-Za-z][0-9A-Za-z.-]*))?")
            .expect("static coerce regex")
    });
    let stripped = input.trim_start();
    let stripped = stripped
        .strip_prefix('v')
        .or_else(|| stripped.strip_prefix('V'))
        .unwrap_or(stripped);
    let caps = re.captures(stripped)?;
    let major: u64 = caps.get(1)?.as_str().parse().ok()?;
    let minor: u64 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    let patch: u64 = caps
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    // Loose coerce: a malformed pre-release identifier (e.g. leading zero —
    // strict per SemVer 2.0.0 §9) drops the pre rather than failing the whole
    // function, matching the "truncate fourth component" behaviour. The base
    // major.minor.patch is what callers reach for `coerce` to rescue.
    let pre = caps
        .get(4)
        .and_then(|m| Prerelease::new(m.as_str()).ok())
        .unwrap_or(Prerelease::EMPTY);
    Some(Version {
        major,
        minor,
        patch,
        pre,
        build: BuildMetadata::EMPTY,
    })
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use std::hint::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("semver");

    // Pure underlying crate path.
    g.bench_function("library/parse_version", |b| {
        b.iter(|| {
            let _ = Version::parse(black_box("1.2.3-beta.1+build.42")).unwrap();
        });
    });

    g.bench_function("library/satisfies", |b| {
        let v = Version::parse("1.2.3").unwrap();
        let r = VersionReq::parse(">=1.0.0, <2.0.0").unwrap();
        b.iter(|| {
            let _ = r.matches(black_box(&v));
        });
    });

    // End-to-end via Lua.
    let parse: mlua::Function = lua
        .load(r#"return function(s) return (require("lib:semver")).parse(s) end"#)
        .eval()
        .expect("parse chunk");
    g.bench_function("through_lua/parse", |b| {
        b.iter(|| {
            let _: mlua::Value = parse.call(black_box("1.2.3-beta.1+build.42")).unwrap();
        });
    });

    let satisfies: mlua::Function = lua
        .load(r#"return function(v, r) return (require("lib:semver")).satisfies(v, r) end"#)
        .eval()
        .expect("satisfies chunk");
    g.bench_function("through_lua/satisfies", |b| {
        b.iter(|| {
            let _: bool = satisfies
                .call((black_box("1.2.3"), black_box(">=1.0.0, <2.0.0")))
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
        let m: Table = lua.load(r#"return require("lib:semver")"#).eval().unwrap();
        lua.globals().set("semver", m).unwrap();
        lua
    }

    #[test]
    fn version_parse_fields() {
        let lua = engine();
        let (major, minor, patch, pre, build): (i64, i64, i64, Option<String>, Option<String>) =
            lua.load(
                r#"
                local v = semver.Version.parse("1.2.3-beta.1+build.42")
                return v.major, v.minor, v.patch, v.pre, v.build
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!((major, minor, patch), (1, 2, 3));
        assert_eq!(pre.as_deref(), Some("beta.1"));
        assert_eq!(build.as_deref(), Some("build.42"));
    }

    #[test]
    fn version_parse_no_pre_no_build_returns_nil() {
        let lua = engine();
        let (pre, build): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local v = semver.Version.parse("1.2.3")
                return v.pre, v.build
            "#,
            )
            .eval()
            .unwrap();
        assert!(pre.is_none());
        assert!(build.is_none());
    }

    #[test]
    fn version_parse_invalid_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return semver.Version.parse("1.2")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("semver.Version.parse:"), "got: {e}");
    }

    #[test]
    fn metamethods_lt_le_eq() {
        let lua = engine();
        let (lt, le, eq): (bool, bool, bool) = lua
            .load(
                r#"
                local a = semver.Version.parse("1.0.0")
                local b = semver.Version.parse("2.0.0")
                local c = semver.Version.parse("1.0.0")
                return a < b, a <= c, a == c
            "#,
            )
            .eval()
            .unwrap();
        assert!(lt && le && eq);
    }

    #[test]
    fn table_sort_uses_lt() {
        let lua = engine();
        let ordered: Vec<String> = lua
            .load(
                r#"
                local list = {
                    semver.Version.parse("2.0.0"),
                    semver.Version.parse("1.0.0"),
                    semver.Version.parse("1.0.1-rc.1"),
                    semver.Version.parse("1.0.1"),
                }
                table.sort(list)
                local out = {}
                for i, v in ipairs(list) do out[i] = tostring(v) end
                return out
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(ordered, vec!["1.0.0", "1.0.1-rc.1", "1.0.1", "2.0.0"]);
    }

    #[test]
    fn tostring_canonical() {
        let lua = engine();
        let s: String = lua
            .load(r#"return tostring(semver.Version.parse("1.2.3-beta.1+m"))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "1.2.3-beta.1+m");
    }

    #[test]
    fn compare_method_accepts_string_or_version() {
        let lua = engine();
        let (a, b, c): (i64, i64, i64) = lua
            .load(
                r#"
                local v = semver.Version.parse("1.2.3")
                return
                    v:compare("1.2.3"),
                    v:compare("1.2.4"),
                    v:compare(semver.Version.parse("1.0.0"))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!((a, b, c), (0, -1, 1));
    }

    #[test]
    fn flat_compare_symmetry_and_identity() {
        let lua = engine();
        let (id, sym): (bool, bool) = lua
            .load(
                r#"
                local va, vb = semver.Version.parse("1.2.3"), semver.Version.parse("2.0.0")
                return semver.compare(va, va) == 0,
                       semver.compare(va, vb) == -semver.compare(vb, va)
            "#,
            )
            .eval()
            .unwrap();
        assert!(id && sym);
    }

    #[test]
    fn flat_compare_mixed_input() {
        let lua = engine();
        let (a, b, c, d): (i64, i64, i64, i64) = lua
            .load(
                r#"
                local v = semver.Version.parse("1.2.3")
                return
                    semver.compare("1.2.3", "1.2.4"),
                    semver.compare("1.2.4", "1.2.3"),
                    semver.compare(v, "1.2.3"),
                    semver.compare("1.2.3", v)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!((a, b, c, d), (-1, 1, 0, 0));
    }

    #[test]
    fn prerelease_ordering_chain() {
        let lua = engine();
        let chain_ordered: bool = lua
            .load(
                r#"
                local list = {
                    "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta",
                    "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11",
                    "1.0.0-rc.1", "1.0.0",
                }
                for i = 1, #list - 1 do
                    if semver.compare(list[i], list[i + 1]) ~= -1 then return false end
                end
                return true
            "#,
            )
            .eval()
            .unwrap();
        assert!(chain_ordered);
    }

    #[test]
    fn build_metadata_ignored_in_precedence() {
        let lua = engine();
        let (eq, cmp): (bool, i64) = lua
            .load(
                r#"
                local a = semver.Version.parse("1.0.0+a")
                local b = semver.Version.parse("1.0.0+b")
                return a == b, semver.compare(a, b)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(cmp, 0, "compare must ignore build per SemVer 2.0.0 §10");
        assert!(eq, "__eq must mirror precedence — got false");
    }

    #[test]
    fn satisfies_pre_release_does_not_match_plain_range() {
        let lua = engine();
        let ok: bool = lua
            .load(r#"return semver.satisfies("1.0.0-alpha", "^1.0.0") == false"#)
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn satisfies_zero_major_caret_is_exact() {
        let lua = engine();
        let (matches_exact, matches_other): (bool, bool) = lua
            .load(
                r#"
                return
                    semver.satisfies("0.0.1", "^0.0.1"),
                    semver.satisfies("0.0.2", "^0.0.1")
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches_exact);
        assert!(!matches_other);
    }

    #[test]
    fn satisfies_invalid_range_returns_nil_err_not_false() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return semver.satisfies("1.0.0", "<<bad>>")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("semver.satisfies:"), "got: {e}");
    }

    #[test]
    fn satisfies_accepts_versionreq_userdata() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local r = semver.VersionReq.parse(">=1.0.0, <2.0.0")
                return semver.satisfies(semver.Version.parse("1.5.0"), r)
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn versionreq_userdata_matches() {
        let lua = engine();
        let (yes, no): (bool, bool) = lua
            .load(
                r#"
                local r = semver.VersionReq.parse(">=1.0.0, <2.0.0")
                return r:matches("1.5.0"), r:matches("2.0.0")
            "#,
            )
            .eval()
            .unwrap();
        assert!(yes);
        assert!(!no);
    }

    #[test]
    fn versionreq_invalid_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return semver.VersionReq.parse("not-a-range")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("semver.VersionReq.parse:"), "got: {e}");
    }

    #[test]
    fn valid_returns_bool_no_error_path() {
        let lua = engine();
        let (yes, no): (bool, bool) = lua
            .load(r#"return semver.valid("1.2.3"), semver.valid("not.a.ver")"#)
            .eval()
            .unwrap();
        assert!(yes);
        assert!(!no);
    }

    #[test]
    fn coerce_vectors() {
        let lua = engine();
        let cases = [
            ("v1.2.3", "1.2.3"),
            ("1.2", "1.2.0"),
            ("1", "1.0.0"),
            ("release-1.2.3", "1.2.3"),
            ("v1.2.3-rc1", "1.2.3-rc1"),
            ("1.2.3.4", "1.2.3"),
            ("1.2.3+build", "1.2.3"),
            // Loose recovery: a pre-release that fails strict SemVer §9
            // (leading zero on a numeric identifier) drops the pre and
            // returns the base, matching the truncation rule for `.4`.
            ("1.2.3-01", "1.2.3"),
        ];
        for (input, expected) in cases {
            lua.globals().set("inp", input).unwrap();
            let s: String = lua
                .load("return tostring(semver.coerce(inp))")
                .eval()
                .unwrap_or_else(|e| panic!("coerce({input:?}) failed: {e}"));
            assert_eq!(s, expected, "coerce({input:?})");
        }
    }

    #[test]
    fn coerce_uncoercible() {
        let lua = engine();
        for bad in ["abc", "", "  "] {
            lua.globals().set("inp", bad).unwrap();
            let (val, e): (Value, Option<String>) =
                lua.load(r#"return semver.coerce(inp)"#).eval().unwrap();
            assert!(matches!(val, Value::Nil), "coerce({bad:?}) should be nil");
            let e = e.unwrap();
            assert!(e.starts_with("semver.coerce:"), "got: {e}");
        }
    }

    #[test]
    fn increment_levels() {
        let lua = engine();
        let cases: Vec<(&str, &str, &str)> = vec![
            ("0.0.0", "major", "1.0.0"),
            ("1.2.3", "minor", "1.3.0"),
            ("1.2.3", "patch", "1.2.4"),
            ("1.2.3-beta.1", "patch", "1.2.3"),
            ("1.2.3", "prerelease", "1.2.4-0"),
            ("1.2.3-beta", "prerelease", "1.2.3-beta.0"),
            ("1.2.3-beta.1", "prerelease", "1.2.3-beta.2"),
            // Node parity — only the trailing identifier matters. A
            // non-numeric tail triggers append, regardless of any earlier
            // numeric segments in the dotted pre.
            ("1.2.3-0.alpha", "prerelease", "1.2.3-0.alpha.0"),
            ("1.2.3-rc.1.beta", "prerelease", "1.2.3-rc.1.beta.0"),
            ("1.2.3+build", "minor", "1.3.0"),
        ];
        for (input, level, expected) in cases {
            lua.globals().set("inp", input).unwrap();
            lua.globals().set("lev", level).unwrap();
            let got: String = lua
                .load("return semver.increment(inp, lev)")
                .eval()
                .unwrap_or_else(|e| panic!("increment({input:?}, {level:?}) failed: {e}"));
            assert_eq!(got, expected, "increment({input:?}, {level:?})");
        }
    }

    #[test]
    fn increment_invalid_level_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return semver.increment("1.2.3", "bogus")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("semver.increment:"), "got: {e}");
        assert!(e.contains("invalid level"), "got: {e}");
    }

    #[test]
    fn version_increment_method_returns_new_object() {
        let lua = engine();
        let (orig_str, new_str, same_obj): (String, String, bool) = lua
            .load(
                r#"
                local v = semver.Version.parse("1.2.3")
                local n = v:increment("minor")
                return tostring(v), tostring(n), v == n
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(orig_str, "1.2.3");
        assert_eq!(new_str, "1.3.0");
        assert!(!same_obj);
    }

    #[test]
    fn format_equivalent_to_tostring() {
        let lua = engine();
        let (a, b): (String, String) = lua
            .load(
                r#"
                local v = semver.Version.parse("1.2.3-rc.1+meta")
                return semver.format(v), tostring(v)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "1.2.3-rc.1+meta");
    }

    #[test]
    fn format_accepts_string() {
        let lua = engine();
        let s: String = lua
            .load(r#"return semver.format("1.2.3-rc.1+meta")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "1.2.3-rc.1+meta");
    }

    #[test]
    fn format_invalid_returns_nil_err_not_raise() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return semver.format("not.a.ver")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("semver.format:"), "got: {e}");
    }

    #[test]
    fn versionreq_tostring_canonical() {
        let lua = engine();
        let s: String = lua
            .load(r#"return tostring(semver.VersionReq.parse(">=1.0.0, <2.0.0"))"#)
            .eval()
            .unwrap();
        assert_eq!(s, ">=1.0.0, <2.0.0");
    }
}
