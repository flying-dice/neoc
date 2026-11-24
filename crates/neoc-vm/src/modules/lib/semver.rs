//! Semantic versions, their order, and ranges: `lib:semver` (the `semver` crate).
//!
//! `Version` is a parsed SemVer 2.0.0 version and `VersionReq` a parsed range; `valid`, `coerce`, `compare` and
//! `satisfies` answer about versions still in text. Ranges are Cargo's: `^1.2`, `~1.2`, `>=1.0, <2.0`, `=1.2.3`,
//! `1.*`, a comma for "and". npm's extensions (`||`, `1.2 - 1.4`, a space for "and") are not read.
//!
//! Order is SemVer's precedence: build metadata takes no part in it (`1.0.0+a` and `1.0.0+b` are equal), and a
//! pre-release comes before its release.
//!
//! The API's rules:
//! - There are no operators: `a:compare(b)` gives `"less" | "equal" | "greater"`, `a:equals(b)` a boolean. `table.sort`
//!   takes a comparator: `a:compare(b) == "less"`.
//! - Each argument has one type: the methods take objects (`v:compare(other)`, `req:matches(version)`), the module
//!   functions take text (`compare("1.2.3", "1.2.4")`, `satisfies("1.2.3", "^1")`).
//! - `increment` is a method and takes its level as a string enum (a level that does not exist is refused as an
//!   argument); a version is read with `Version.parse` and written with `tostring`.
//! - `coerce` gives `nil` when the text holds no version.
//! - A failure is a typed error: `ParseError` (`invalid_version`, `invalid_range`) and `IncrementError` (`overflow`).

use std::sync::OnceLock;

use ::semver::{BuildMetadata, Prerelease, Version};
use regex::Regex;

/// The pre-release after `pre`: its last identifier one higher when that is a number, `.0` appended when it is not
/// (`beta.1` is `beta.2`, `beta` is `beta.0`, `0.alpha` is `0.alpha.0`), and `0` after none. Only the last
/// identifier counts, as in node's `semver`. `None` when the number is `u64::MAX`.
fn bump_prerelease(pre: &str) -> Option<Prerelease> {
    if pre.is_empty() {
        return Prerelease::new("0").ok();
    }
    let (head, last) = match pre.rsplit_once('.') {
        Some((head, last)) => (Some(head), last),
        None => (None, pre),
    };
    let next = if !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        let bumped = last.parse::<u64>().ok()?.checked_add(1)?.to_string();
        match head {
            Some(head) => format!("{head}.{bumped}"),
            None => bumped,
        }
    } else {
        format!("{pre}.0")
    };
    Prerelease::new(&next).ok()
}

/// The first thing in `text` that reads as a version: a leading `v` is dropped, a missing minor or patch is 0,
/// build metadata and a fourth number are left out. A pre-release that is not valid (`-01`) is dropped, not the
/// version with it.
fn coerce_version(text: &str) -> Option<Version> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        // The start of the text or a non-digit comes before the major number, so a digit is never taken from the
        // middle of a longer run.
        Regex::new(r"(?:^|[^0-9])(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([0-9A-Za-z][0-9A-Za-z.-]*))?")
            .expect("the pattern is valid")
    });
    let text = text.trim_start();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let found = pattern.captures(text)?;
    let number = |group: usize| found.get(group).and_then(|m| m.as_str().parse::<u64>().ok());
    Some(Version {
        major: number(1)?,
        minor: number(2).unwrap_or(0),
        patch: number(3).unwrap_or(0),
        pre: found.get(4).and_then(|m| Prerelease::new(m.as_str()).ok()).unwrap_or(Prerelease::EMPTY),
        build: BuildMetadata::EMPTY,
    })
}

#[mlua_ir::module("lib:semver")]
pub mod semver {
    use ::semver::{BuildMetadata, Prerelease};
    use mlua_ir::{LuaError, Object, Shape};
    use serde::{Deserialize, Serialize};

    use super::{bump_prerelease, coerce_version};

    /// How two versions order, by SemVer's precedence.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Order {
        /// The first comes before the second.
        Less,
        /// Neither comes before the other (build metadata takes no part).
        Equal,
        /// The first comes after the second.
        Greater,
    }

    impl From<std::cmp::Ordering> for Order {
        fn from(ordering: std::cmp::Ordering) -> Self {
            match ordering {
                std::cmp::Ordering::Less => Order::Less,
                std::cmp::Ordering::Equal => Order::Equal,
                std::cmp::Ordering::Greater => Order::Greater,
            }
        }
    }

    /// Which part of a version `increment` raises.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Level {
        /// The next major version: `1.2.3` is `2.0.0`.
        Major,
        /// The next minor version: `1.2.3` is `1.3.0`.
        Minor,
        /// The next patch: `1.2.3` is `1.2.4`, and a pre-release is its release (`1.2.3-rc.1` is `1.2.3`).
        Patch,
        /// The next pre-release: `1.2.3` is `1.2.4-0`, `1.2.3-beta` is `1.2.3-beta.0`, `1.2.3-beta.1` is
        /// `1.2.3-beta.2`.
        Prerelease,
    }

    impl Level {
        fn name(self) -> &'static str {
            match self {
                Level::Major => "major",
                Level::Minor => "minor",
                Level::Patch => "patch",
                Level::Prerelease => "prerelease",
            }
        }
    }

    /// Why a text is not a version or a range.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ParseError {
        /// The text is not a SemVer 2.0.0 version (`1.2`, `v1.2.3`, `1.2.3-01`).
        #[error("invalid version {text:?}: {reason}")]
        InvalidVersion {
            /// The text as given.
            text: String,
            /// What is wrong.
            reason: String,
        },
        /// The text is not a range.
        #[error("invalid range {text:?}: {reason}")]
        InvalidRange {
            /// The text as given.
            text: String,
            /// What is wrong.
            reason: String,
        },
    }

    /// Why a version has no next one.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum IncrementError {
        /// The number to raise is already the largest a version holds.
        #[error("the {} number is the largest a version holds", .level.name())]
        Overflow {
            /// The part that was to be raised.
            level: Level,
        },
    }

    fn version(text: &str) -> Result<::semver::Version, ParseError> {
        ::semver::Version::parse(text)
            .map_err(|error| ParseError::InvalidVersion { text: text.to_owned(), reason: error.to_string() })
    }

    fn range(text: &str) -> Result<::semver::VersionReq, ParseError> {
        ::semver::VersionReq::parse(text)
            .map_err(|error| ParseError::InvalidRange { text: text.to_owned(), reason: error.to_string() })
    }

    /// A SemVer 2.0.0 version. `tostring` gives its canonical text.
    #[derive(Debug, Clone, Object)]
    pub struct Version {
        inner: ::semver::Version,
    }

    #[mlua_ir::object]
    impl Version {
        /// Read a version: `major.minor.patch`, then `-pre.release` and `+build` when it has them.
        ///
        /// ```luau
        /// local v = assert(semver.Version.parse("1.2.3-beta.1+build.42"))
        /// assert(v.major == 1 and v.pre == "beta.1" and v.build == "build.42")
        /// local _, err = semver.Version.parse("1.2")
        /// assert(err ~= nil and err.kind == "invalid_version")
        /// ```
        pub fn parse(text: String) -> Result<Version, ParseError> {
            version(&text).map(|inner| Version { inner })
        }

        /// The major number.
        #[get]
        pub fn major(&self) -> u64 {
            self.inner.major
        }

        /// The minor number.
        #[get]
        pub fn minor(&self) -> u64 {
            self.inner.minor
        }

        /// The patch number.
        #[get]
        pub fn patch(&self) -> u64 {
            self.inner.patch
        }

        /// The pre-release identifiers (`beta.1`); `nil` for a release.
        #[get]
        pub fn pre(&self) -> Option<String> {
            (!self.inner.pre.is_empty()).then(|| self.inner.pre.as_str().to_owned())
        }

        /// The build metadata (`build.42`); `nil` when there is none.
        #[get]
        pub fn build(&self) -> Option<String> {
            (!self.inner.build.is_empty()).then(|| self.inner.build.as_str().to_owned())
        }

        /// How this orders against `other`, by precedence: build metadata takes no part.
        ///
        /// ```luau
        /// local parse = semver.Version.parse
        /// local list = { assert(parse("2.0.0")), assert(parse("1.0.1")), assert(parse("1.0.1-rc.1")) }
        /// table.sort(list, function(a, b)
        ///     return a:compare(b) == "less"
        /// end)
        /// assert(tostring(list[1]) == "1.0.1-rc.1" and tostring(list[3]) == "2.0.0")
        /// ```
        pub fn compare(&self, other: Version) -> Order {
            self.inner.cmp_precedence(&other.inner).into()
        }

        /// Whether this and `other` have the same precedence: `1.0.0+a` equals `1.0.0+b`. (`==` on two versions
        /// asks whether they are the same object.)
        pub fn equals(&self, other: Version) -> bool {
            self.inner.cmp_precedence(&other.inner).is_eq()
        }

        /// The next version at `level`, without build metadata. This version is not changed.
        ///
        /// ```luau
        /// local v = assert(semver.Version.parse("1.2.3"))
        /// assert(tostring(assert(v:increment("minor"))) == "1.3.0" and tostring(v) == "1.2.3")
        /// assert(tostring(assert(v:increment("prerelease"))) == "1.2.4-0")
        /// ```
        pub fn increment(&self, level: Level) -> Result<Version, IncrementError> {
            let overflow = || IncrementError::Overflow { level };
            let mut next = self.inner.clone();
            next.build = BuildMetadata::EMPTY;
            match level {
                Level::Major => {
                    next.major = next.major.checked_add(1).ok_or_else(overflow)?;
                    next.minor = 0;
                    next.patch = 0;
                    next.pre = Prerelease::EMPTY;
                }
                Level::Minor => {
                    next.minor = next.minor.checked_add(1).ok_or_else(overflow)?;
                    next.patch = 0;
                    next.pre = Prerelease::EMPTY;
                }
                // SemVer 2.0.0 §11: a pre-release comes before its release, so dropping it is the next patch.
                Level::Patch if !next.pre.is_empty() => next.pre = Prerelease::EMPTY,
                Level::Patch => next.patch = next.patch.checked_add(1).ok_or_else(overflow)?,
                Level::Prerelease if next.pre.is_empty() => {
                    next.patch = next.patch.checked_add(1).ok_or_else(overflow)?;
                    next.pre = bump_prerelease("").ok_or_else(overflow)?;
                }
                Level::Prerelease => next.pre = bump_prerelease(next.pre.as_str()).ok_or_else(overflow)?,
            }
            Ok(Version { inner: next })
        }

        /// The canonical text: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// A range of versions, in Cargo's syntax: `^1.2`, `~1.2.3`, `>=1.0.0, <2.0.0`, `=1.2.3`, `1.*`. `tostring` gives
    /// its canonical text.
    #[derive(Debug, Clone, Object)]
    pub struct VersionReq {
        inner: ::semver::VersionReq,
    }

    #[mlua_ir::object]
    impl VersionReq {
        /// Read a range.
        ///
        /// ```luau
        /// local req = assert(semver.VersionReq.parse(">=1.0.0, <2.0.0"))
        /// assert(req:matches((assert(semver.Version.parse("1.5.0")))))
        /// local _, err = semver.VersionReq.parse("not-a-range")
        /// assert(err ~= nil and err.kind == "invalid_range")
        /// ```
        pub fn parse(text: String) -> Result<VersionReq, ParseError> {
            range(&text).map(|inner| VersionReq { inner })
        }

        /// Whether `version` is in the range. A pre-release is in a range only when the range names a pre-release
        /// of the same `major.minor.patch`.
        pub fn matches(&self, version: Version) -> bool {
            self.inner.matches(&version.inner)
        }

        /// The canonical text: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// Whether `text` is a SemVer 2.0.0 version.
    pub fn valid(text: String) -> bool {
        ::semver::Version::parse(&text).is_ok()
    }

    /// The version a loose text means: a leading `v` is dropped, a missing minor or patch is 0, build metadata and
    /// a fourth number are left out. `nil` when the text holds no number.
    ///
    /// ```luau
    /// assert(tostring(semver.coerce("v1.2")) == "1.2.0")
    /// assert(tostring(semver.coerce("release-1.2.3.4+build")) == "1.2.3")
    /// assert(semver.coerce("latest") == nil)
    /// ```
    pub fn coerce(text: String) -> Option<Version> {
        coerce_version(&text).map(|inner| Version { inner })
    }

    /// How version `a` orders against version `b`, both as text, by precedence.
    ///
    /// ```luau
    /// assert(semver.compare("1.0.0-rc.1", "1.0.0") == "less")
    /// ```
    pub fn compare(a: String, b: String) -> Result<Order, ParseError> {
        Ok(version(&a)?.cmp_precedence(&version(&b)?).into())
    }

    /// Whether `version` is in `range`, both as text. An error is a text that does not parse, never a version
    /// outside the range.
    ///
    /// ```luau
    /// assert(semver.satisfies("1.2.3", "^1.0.0") == true)
    /// assert(semver.satisfies("2.0.0", "^1.0.0") == false)
    /// ```
    pub fn satisfies(version: String, range: String) -> Result<bool, ParseError> {
        let (version, range) = (self::version(&version)?, self::range(&range)?);
        Ok(range.matches(&version))
    }
}

#[cfg(test)]
mod tests {
    use super::{bump_prerelease, coerce_version};

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:semver")"#).eval().unwrap();
        lua.globals().set("semver", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn the_next_prerelease_raises_the_last_number_or_appends_one() {
        let next = |pre: &str| bump_prerelease(pre).map(|p| p.as_str().to_owned());
        assert_eq!(next("").as_deref(), Some("0"));
        assert_eq!(next("beta").as_deref(), Some("beta.0"));
        assert_eq!(next("beta.1").as_deref(), Some("beta.2"));
        assert_eq!(next("9").as_deref(), Some("10"));
        assert_eq!(next("0.alpha").as_deref(), Some("0.alpha.0"), "only the last identifier counts");
        assert_eq!(next("rc.1.beta").as_deref(), Some("rc.1.beta.0"));
        assert_eq!(next("rc-1").as_deref(), Some("rc-1.0"));
        assert_eq!(next("18446744073709551615"), None);
    }

    #[test]
    fn coerce_reads_loose_text() {
        for (text, version) in [
            ("v1.2.3", "1.2.3"),
            ("V1.2.3", "1.2.3"),
            ("  v1.2.3", "1.2.3"),
            ("1.2", "1.2.0"),
            ("1", "1.0.0"),
            ("release-1.2.3", "1.2.3"),
            ("v1.2.3-rc1", "1.2.3-rc1"),
            ("1.2.3.4", "1.2.3"),
            ("1.2.3+build", "1.2.3"),
            // a pre-release that SemVer §9 refuses (a number with a leading zero) is dropped, not the version
            ("1.2.3-01", "1.2.3"),
        ] {
            assert_eq!(
                coerce_version(text).map(|v| v.to_string()).as_deref(),
                Some(version),
                "coerce({text:?})"
            );
        }
        for text in ["abc", "", "  ", "v", "99999999999999999999999"] {
            assert_eq!(coerce_version(text), None, "coerce({text:?})");
        }
    }

    #[test]
    fn a_version_has_its_parts() {
        let (major, minor, patch, pre, build): (u64, u64, u64, Option<String>, Option<String>) = eval(
            r#"
            local v = assert(semver.Version.parse("1.2.3-beta.1+build.42"))
            return v.major, v.minor, v.patch, v.pre, v.build
            "#,
        );
        assert_eq!((major, minor, patch), (1, 2, 3));
        assert_eq!((pre.as_deref(), build.as_deref()), (Some("beta.1"), Some("build.42")));

        let (pre, build): (Option<String>, Option<String>) =
            eval(r#"local v = assert(semver.Version.parse("1.2.3")); return v.pre, v.build"#);
        assert_eq!((pre, build), (None, None));
    }

    #[test]
    fn tostring_is_canonical() {
        let (version, range): (String, String) = eval(
            r#"
            return tostring(assert(semver.Version.parse("1.2.3-rc.1+meta"))),
                tostring(assert(semver.VersionReq.parse(">=1.0.0, <2.0.0")))
            "#,
        );
        assert_eq!(version, "1.2.3-rc.1+meta");
        assert_eq!(range, ">=1.0.0, <2.0.0");
    }

    #[test]
    fn what_does_not_parse_is_a_typed_error() {
        let (version, range): (Vec<String>, Vec<String>) = eval(
            r#"
            local none, bad = semver.Version.parse("1.2")
            assert(none == nil and bad.op == "lib:semver.Version.parse" and type(bad.reason) == "string")
            local also, range = semver.VersionReq.parse("not-a-range")
            assert(also == nil and range.op == "lib:semver.VersionReq.parse")
            return { bad.kind, bad.text, tostring(bad) }, { range.kind, range.text, tostring(range) }
            "#,
        );
        assert_eq!(version[..2], ["invalid_version", "1.2"]);
        assert!(
            version[2].starts_with("lib:semver.Version.parse: invalid_version: invalid version \"1.2\": "),
            "{}",
            version[2]
        );
        assert_eq!(range[..2], ["invalid_range", "not-a-range"]);
        assert!(
            range[2]
                .starts_with("lib:semver.VersionReq.parse: invalid_range: invalid range \"not-a-range\": "),
            "{}",
            range[2]
        );
    }

    #[test]
    fn versions_compare_by_precedence() {
        let seen: Vec<String> = eval(
            r#"
            local parse = function(text) return (assert(semver.Version.parse(text))) end
            local v = parse("1.2.3")
            return { v:compare(parse("1.2.3")), v:compare(parse("1.2.4")), v:compare(parse("1.0.0")), v:compare(v) }
            "#,
        );
        assert_eq!(seen, ["equal", "less", "greater", "equal"]);

        let (equals, differs, same_object): (bool, bool, bool) = eval(
            r#"
            local a, b, c = semver.Version.parse("1.0.0"), semver.Version.parse("2.0.0"), semver.Version.parse("1.0.0")
            return a:equals(c), a:equals(b), a == c
            "#,
        );
        assert!(equals && !differs);
        assert!(!same_object, "`==` is identity: there is no operator");
    }

    #[test]
    fn a_list_sorts_with_a_comparator() {
        let ordered: Vec<String> = eval(
            r#"
            local list = {}
            for _, text in { "2.0.0", "1.0.0", "1.0.1-rc.1", "1.0.1" } do
                table.insert(list, (assert(semver.Version.parse(text))))
            end
            table.sort(list, function(a, b) return a:compare(b) == "less" end)
            local out = {}
            for i, v in list do out[i] = tostring(v) end
            return out
            "#,
        );
        assert_eq!(ordered, ["1.0.0", "1.0.1-rc.1", "1.0.1", "2.0.0"]);
    }

    #[test]
    fn compare_orders_texts() {
        let seen: Vec<String> = eval(
            r#"
            return {
                semver.compare("1.2.3", "1.2.3"), semver.compare("1.2.3", "1.2.4"), semver.compare("1.2.4", "1.2.3"),
                semver.compare("10.20.30+build", "10.20.30+build"), semver.compare("1.0.0-rc.1", "1.0.0"),
                semver.compare("1.0.0", "1.0.0-rc.1"), semver.compare("2.0.0", "10.0.0"),
            }
            "#,
        );
        assert_eq!(seen, ["equal", "less", "greater", "equal", "less", "greater", "less"]);

        let (kind, text, op): (String, String, String) = eval(
            r#"
            local order, err = semver.compare("1.2.3", "not.a.ver")
            assert(order == nil)
            return err.kind, err.text, err.op
            "#,
        );
        assert_eq!(
            (kind.as_str(), text.as_str(), op.as_str()),
            ("invalid_version", "not.a.ver", "lib:semver.compare")
        );
    }

    /// SemVer 2.0.0 §11: the whole chain, each before the next.
    #[test]
    fn prereleases_order_as_the_spec_says() {
        let ordered: bool = eval(
            r#"
            local chain = {
                "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta",
                "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0",
            }
            for i = 1, #chain - 1 do
                if semver.compare(chain[i], chain[i + 1]) ~= "less" then return false end
                if semver.compare(chain[i + 1], chain[i]) ~= "greater" then return false end
            end
            return true
            "#,
        );
        assert!(ordered);
    }

    /// SemVer 2.0.0 §10: build metadata takes no part in precedence.
    #[test]
    fn build_metadata_is_ignored_in_precedence() {
        let (equals, order, flat): (bool, String, String) = eval(
            r#"
            local a, b = assert(semver.Version.parse("1.0.0+a")), assert(semver.Version.parse("1.0.0+b"))
            return a:equals(b), a:compare(b), semver.compare("1.0.0+a", "1.0.0+b")
            "#,
        );
        assert!(equals);
        assert_eq!((order.as_str(), flat.as_str()), ("equal", "equal"));
    }

    #[test]
    fn satisfies_reads_cargo_ranges() {
        let seen: Vec<bool> = eval(
            r#"
            return {
                semver.satisfies("1.2.3", "^1.0.0"), semver.satisfies("2.0.0", "^1.0.0"),
                semver.satisfies("1.2.3", "~1.2.0"), semver.satisfies("1.3.0", "~1.2.0"),
                -- a pre-release is not in a plain range
                semver.satisfies("1.0.0-alpha", "^1.0.0"),
                -- under 0.1, a caret is exact
                semver.satisfies("0.0.1", "^0.0.1"), semver.satisfies("0.0.2", "^0.0.1"),
                semver.satisfies("1.5.0", ">=1.0.0, <2.0.0"), semver.satisfies("1.5.0", "1.*"),
            }
            "#,
        );
        assert_eq!(seen, [true, false, true, false, false, true, false, true, true]);
    }

    #[test]
    fn satisfies_fails_for_a_text_that_does_not_parse_never_for_a_miss() {
        let (range_kind, range_text, version_kind, npm): (String, String, String, String) = eval(
            r#"
            local miss, none = semver.satisfies("2.0.0", "^1.0.0")
            assert(miss == false and none == nil)
            local answer, range = semver.satisfies("1.0.0", "<<bad>>")
            assert(answer == nil and range.op == "lib:semver.satisfies")
            local _, version = semver.satisfies("nope", "^1")
            local _, npm = semver.satisfies("1.0.0", "^1 || ^2")
            return range.kind, range.text, version.kind, npm.kind
            "#,
        );
        assert_eq!((range_kind.as_str(), range_text.as_str()), ("invalid_range", "<<bad>>"));
        assert_eq!(version_kind, "invalid_version");
        assert_eq!(npm, "invalid_range", "npm's `||` is not read");
    }

    #[test]
    fn a_range_matches_versions() {
        let seen: Vec<bool> = eval(
            r#"
            local parse = function(text) return (assert(semver.Version.parse(text))) end
            local req = assert(semver.VersionReq.parse(">=1.0.0, <2.0.0"))
            local caret = assert(semver.VersionReq.parse("^1.0.0"))
            return {
                req:matches(parse("1.0.0")), req:matches(parse("1.5.0")), req:matches(parse("2.0.0")),
                req:matches(parse("0.9.9")), caret:matches(parse("1.4.2")),
            }
            "#,
        );
        assert_eq!(seen, [true, true, false, false, true]);
    }

    #[test]
    fn valid_is_a_predicate() {
        let seen: Vec<bool> = eval(
            r#"return { semver.valid("1.2.3"), semver.valid("1.2.3-rc.1+b"), semver.valid("not.a.ver"), semver.valid("1.2"), semver.valid("v1.2.3"), semver.valid("") }"#,
        );
        assert_eq!(seen, [true, true, false, false, false, false]);
    }

    #[test]
    fn coerce_gives_a_version_or_nil() {
        let (coerced, major, none): (String, u64, bool) = eval(
            r#"
            local v = semver.coerce("release-1.2")
            return tostring(v), v.major, semver.coerce("abc") == nil and semver.coerce("") == nil and semver.coerce("  ") == nil
            "#,
        );
        assert_eq!((coerced.as_str(), major), ("1.2.0", 1));
        assert!(none);
    }

    #[test]
    fn increment_gives_the_next_version() {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let next: mlua::Function = lua
            .load(
                r#"
                local semver = require("lib:semver")
                return function(text, level)
                    return tostring(assert(assert(semver.Version.parse(text)):increment(level)))
                end
                "#,
            )
            .eval()
            .unwrap();
        for (text, level, expected) in [
            ("0.0.0", "major", "1.0.0"),
            ("1.2.3-rc.1", "major", "2.0.0"),
            ("1.2.3", "minor", "1.3.0"),
            ("1.2.3", "patch", "1.2.4"),
            ("1.2.3-beta.1", "patch", "1.2.3"),
            ("1.2.3", "prerelease", "1.2.4-0"),
            ("1.2.3-beta", "prerelease", "1.2.3-beta.0"),
            ("1.2.3-beta.1", "prerelease", "1.2.3-beta.2"),
            // only the last identifier counts, as in node's semver
            ("1.2.3-0.alpha", "prerelease", "1.2.3-0.alpha.0"),
            ("1.2.3-rc.1.beta", "prerelease", "1.2.3-rc.1.beta.0"),
            // build metadata is left out
            ("1.2.3+build", "minor", "1.3.0"),
            ("1.2.3+meta", "patch", "1.2.4"),
        ] {
            let got: String =
                next.call((text, level)).unwrap_or_else(|e| panic!("increment({text:?}, {level:?}): {e}"));
            assert_eq!(got, expected, "increment({text:?}, {level:?})");
        }
    }

    #[test]
    fn increment_leaves_the_version_alone() {
        let (before, after): (String, String) = eval(
            r#"
            local v = assert(semver.Version.parse("1.2.3"))
            local n = assert(v:increment("minor"))
            return tostring(v), tostring(n)
            "#,
        );
        assert_eq!((before.as_str(), after.as_str()), ("1.2.3", "1.3.0"));
    }

    #[test]
    fn the_largest_number_has_no_next() {
        let seen: Vec<String> = eval(
            r#"
            local out = {}
            for _, case in {
                { "18446744073709551615.0.0", "major" },
                { "1.18446744073709551615.0", "minor" },
                { "1.0.18446744073709551615", "patch" },
                { "1.0.18446744073709551615", "prerelease" },
                { "1.0.0-rc.18446744073709551615", "prerelease" },
            } do
                local next, err = assert(semver.Version.parse(case[1])):increment(case[2])
                assert(next == nil and err.kind == "overflow" and err.op == "lib:semver.Version:increment")
                assert(err.message == "the " .. case[2] .. " number is the largest a version holds")
                table.insert(out, err.level)
            end
            return out
            "#,
        );
        assert_eq!(seen, ["major", "minor", "patch", "prerelease", "prerelease"]);
    }

    #[test]
    fn arguments_are_checked_by_their_types() {
        let (level, text, object): (String, String, String) = eval(
            r#"
            local v = assert(semver.Version.parse("1.2.3"))
            local req = assert(semver.VersionReq.parse("^1"))
            local _, level = pcall(v.increment, v, "bogus")
            local _, text = pcall(v.compare, v, "1.2.3")
            local _, object = pcall(semver.compare, v, "1.2.3")
            assert(not pcall(req.matches, req, "1.2.3") and not pcall(req.matches, req, req))
            assert(not pcall(semver.satisfies, v, req) and not pcall(semver.valid, 1) and not pcall(semver.coerce, nil))
            assert(semver.parse == nil and semver.format == nil and semver.increment == nil)
            return tostring(level), tostring(text), tostring(object)
            "#,
        );
        assert!(level.contains("lib:semver.Version:increment: argument #1 'level'"), "got: {level}");
        assert!(text.contains("lib:semver.Version:compare: argument #1 'other'"), "got: {text}");
        assert!(object.contains("lib:semver.compare: argument #1 'a'"), "got: {object}");
    }
}
