use crate::{error::DomainError, limits::MAX_PACKAGE_FILES, KitRequirement, Permission};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Validated app identifier; the only way to get one is `AppId::parse`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[cfg_attr(feature = "codegen", ts(type = "string"))]
#[cfg_attr(feature = "codegen", schemars(with = "String"))]
pub struct AppId(String);

impl AppId {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let ok = (3..=40).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !s.starts_with('-');
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(DomainError::InvalidAppId(s.to_owned()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
// Hand-written (not `#[serde(try_from, into)]`) so the codegen derive of `ts-rs` has no serde attribute
// it cannot parse: the wire form is a plain string that is validated on the way in.
impl Serialize for AppId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for AppId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}
impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationSpec {
    pub version: u32,
    pub name: String,
    /// path of the .sql file inside the app ref
    pub file: String,
}

/// `manifest.json` of an app package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: AppId,
    pub name: String,
    pub version: String,
    /// The entry file, a path in the app ref: the app built into one Lua 5.1 file (`app.lua`).
    pub main: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub migrations: Vec<MigrationSpec>,
    /// What a registry or the "add by URL" preview shows without running the app: a symbol name for the icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A palette role (`orange`, `blue`, ...): the icon's tint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    /// One or two sentences about the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What the host must provide for the app to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<Requires>,
}

/// The host API version this build serves (`major.minor`). Packages state the range they need in
/// [`Requires::host`]; a newer major is a breaking change, so it never satisfies an older requirement.
pub const HOST_API: (u32, u32) = (1, 0);

/// `requires` in the package manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    /// `">=1.0"`: the host API range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl Manifest {
    /// Self-consistency of the package manifest (before anything is fetched).
    pub fn validate(&self) -> Result<(), DomainError> {
        if std::path::Path::new(&self.main).extension().is_none_or(|e| e != "lua") {
            return Err(DomainError::InvalidManifest(format!(
                "main must be the built Lua 5.1 file (ending in `.lua`), not `{}`; build the app with `bun run build:apps`",
                self.main
            )));
        }
        let in_order = self.migrations.iter().enumerate().all(|(i, m)| m.version as usize == i + 1);
        if !in_order {
            return Err(DomainError::MigrationOrder);
        }
        if let Some(host) = self.requires.as_ref().and_then(|r| r.host.as_deref()) {
            let need = KitRequirement::parse(host)?;
            if !need.satisfied_by(HOST_API.0, HOST_API.1) {
                return Err(DomainError::UnsupportedHost {
                    needs: host.to_owned(),
                    has: format!("{}.{}", HOST_API.0, HOST_API.1),
                });
            }
        }
        Ok(())
    }
}

/// Where an app ref came from, for people to read: the host of an `http(s)` URL, `this computer` for a
/// `file://` one.
#[must_use]
pub fn source_label(url: &str) -> String {
    match url.split_once("://") {
        Some(("file", _)) => "this computer".to_owned(),
        Some((_, rest)) => rest.split(['/', '?', '#']).next().unwrap_or(rest).to_owned(),
        None => url.to_owned(),
    }
}

/// `file` resolved against the URL of the document that names it: absolute URLs stay as they are, a
/// relative one is looked up next to the document (`dir/app-ref.json` + `app.lua` = `dir/app.lua`).
///
/// For the entries of a registry, which may point anywhere. A package's files go through
/// [`resolve_package_url`] instead.
#[must_use]
pub fn resolve_url(base: &str, file: &str) -> String {
    if file.contains("://") {
        return file.to_owned();
    }
    let dir = base.rsplit_once('/').map_or(base, |(dir, _)| dir);
    format!("{dir}/{}", file.trim_start_matches("./"))
}

/// `scheme://authority` of a URL, lowercased (`None` when it has no `://`).
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    Some(format!("{scheme}://{authority}").to_ascii_lowercase())
}

/// Whether a path segment climbs out of its directory (`..`, also percent-encoded or with a backslash).
fn climbs(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| segment.to_ascii_lowercase().replace("%2e", ".") == "..")
}

/// A package file's URL resolved against the app ref that lists it. Unlike [`resolve_url`] it never leaves
/// the ref: an absolute URL must have the ref's scheme and authority (an `https` ref cannot list
/// `file:///etc/passwd` or another host), and no path may climb with `..`.
pub fn resolve_package_url(base: &str, file: &str) -> Result<String, DomainError> {
    let refuse =
        |reason: &str| DomainError::UnsafeFileUrl { url: file.to_owned(), reason: reason.to_owned() };
    if file.chars().any(char::is_control) {
        return Err(refuse("it contains a control character"));
    }
    if climbs(file) {
        return Err(refuse("it climbs out of the app's directory with `..`"));
    }
    if file.contains("://") {
        let (Some(wanted), Some(own)) = (origin_of(file), origin_of(base)) else {
            return Err(refuse("it is not a URL of the app ref's origin"));
        };
        if wanted != own {
            return Err(refuse(&format!("it is on {wanted}, the app ref is on {own}")));
        }
        return Ok(file.to_owned());
    }
    if file.starts_with(['/', '\\']) {
        return Err(refuse("a package file is relative to its app ref"));
    }
    Ok(resolve_url(base, file))
}

/// A registry file: the app refs it offers, by URL (relative URLs resolve against the registry's own).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub apps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    pub url: String,
    pub sha256: String,
}

/// An "app ref": the manifest plus file URLs with checksums. Loaded identically over file/http/https;
/// a registry is just a list of app-ref URLs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRef {
    pub manifest: Manifest,
    pub files: Vec<FileRef>,
}

impl AppRef {
    /// The same app ref with every relative file URL resolved against `base` (the URL it was loaded from).
    /// Refuses a ref that lists too many files or a file outside the ref's own origin and directory.
    pub fn resolved(mut self, base: &str) -> Result<Self, DomainError> {
        if self.files.len() > MAX_PACKAGE_FILES {
            return Err(DomainError::PackageTooLarge(format!(
                "the app ref lists {} files, at most {MAX_PACKAGE_FILES} are allowed",
                self.files.len()
            )));
        }
        for file in &mut self.files {
            file.url = resolve_package_url(base, &file.url)?;
        }
        Ok(self)
    }

    pub fn file(&self, path: &str) -> Result<&FileRef, DomainError> {
        self.files.iter().find(|f| f.path == path).ok_or_else(|| DomainError::MissingFile(path.to_owned()))
    }

    /// Pure integrity check; the fetcher adapter does the I/O, the domain decides what is acceptable.
    pub fn verify(&self, path: &str, bytes: &[u8]) -> Result<(), DomainError> {
        let expected = &self.file(path)?.sha256;
        let actual = format!("{:x}", Sha256::digest(bytes));
        if expected.eq_ignore_ascii_case(&actual) {
            Ok(())
        } else {
            Err(DomainError::ChecksumMismatch { path: path.to_owned(), expected: expected.clone(), actual })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_rules() {
        assert!(AppId::parse("notes").is_ok());
        assert!(AppId::parse("Notes").is_err());
        assert!(AppId::parse("a").is_err());
        assert!(AppId::parse("-bad").is_err());
        assert!(serde_json::from_str::<AppId>("\"../etc\"").is_err());
    }

    #[test]
    fn verify_detects_tampering() {
        let r = AppRef {
            manifest: Manifest {
                id: AppId::parse("notes").unwrap(),
                name: "Notes".into(),
                version: "1.0.0".into(),
                main: "main.lua".into(),
                permissions: vec![],
                migrations: vec![],
                icon: None,
                tint: None,
                description: None,
                requires: None,
            },
            files: vec![FileRef {
                path: "main.lua".into(),
                url: "file:///x".into(),
                // sha256("abc")
                sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            }],
        };
        assert!(r.verify("main.lua", b"abc").is_ok());
        assert!(matches!(r.verify("main.lua", b"abd"), Err(DomainError::ChecksumMismatch { .. })));
        assert!(matches!(r.verify("nope", b""), Err(DomainError::MissingFile(_))));
    }

    #[test]
    fn host_requirements_are_checked_against_the_host_api() {
        let mut m = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "m.lua".into(),
            permissions: vec![],
            migrations: vec![],
            icon: None,
            tint: None,
            description: None,
            requires: Some(Requires { host: Some(">=1.0".into()) }),
        };
        assert!(m.validate().is_ok());
        m.requires = Some(Requires { host: Some(">=2.0".into()) });
        assert!(matches!(m.validate(), Err(DomainError::UnsupportedHost { .. })));
        m.requires = Some(Requires { host: Some("soon".into()) });
        assert!(m.validate().is_err(), "unparseable");
    }

    #[test]
    fn urls_resolve_next_to_the_document_that_names_them() {
        assert_eq!(resolve_url("file:///r/calc/app-ref.json", "app.lua"), "file:///r/calc/app.lua");
        assert_eq!(resolve_url("https://x.org/a/ref.json", "./b/c.lua"), "https://x.org/a/b/c.lua");
        assert_eq!(resolve_url("file:///r/ref.json", "https://y.org/z"), "https://y.org/z");
        assert_eq!(source_label("https://apps.example.org:8443/x/ref.json"), "apps.example.org:8443");
        assert_eq!(source_label("file:///r/ref.json"), "this computer");
    }

    fn ref_listing(urls: &[&str]) -> AppRef {
        let files = urls
            .iter()
            .enumerate()
            .map(|(i, u)| FileRef { path: format!("f{i}.lua"), url: (*u).into(), sha256: String::new() })
            .collect();
        let manifest = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "f0.lua".into(),
            permissions: vec![],
            migrations: vec![],
            icon: None,
            tint: None,
            description: None,
            requires: None,
        };
        AppRef { manifest, files }
    }

    #[test]
    fn a_package_file_must_stay_on_the_refs_scheme_and_origin() {
        let base = "https://apps.example.org/notes/app-ref.json";
        for ok in
            ["app.lua", "./lib/x.lua", "https://apps.example.org/notes/app.lua", "HTTPS://Apps.Example.org/a"]
        {
            assert!(resolve_package_url(base, ok).is_ok(), "{ok}");
        }
        for bad in [
            "file:///dev/zero",
            "file:///etc/passwd",
            "http://apps.example.org/a.lua",
            "https://evil.example/a.lua",
            "https://apps.example.org@evil.example/a.lua",
            "https://apps.example.org:8443/a.lua",
            "ftp://apps.example.org/a",
        ] {
            assert!(
                matches!(resolve_package_url(base, bad), Err(DomainError::UnsafeFileUrl { .. })),
                "{bad} must be refused"
            );
        }
        // a file ref may list file urls, and only file urls
        assert!(resolve_package_url("file:///r/ref.json", "file:///r/app.lua").is_ok());
        assert!(resolve_package_url("file:///r/ref.json", "https://evil.example/a").is_err());
        assert!(resolve_package_url("mem://notes/ref.json", "mem://notes/a").is_ok());
        assert!(resolve_package_url("mem://notes/ref.json", "mem://other/a").is_err());
    }

    #[test]
    fn a_package_file_cannot_climb_out_with_dot_dot_or_start_at_the_root() {
        let base = "file:///r/notes/app-ref.json";
        for bad in [
            "../secrets.lua",
            "lib/../../x.lua",
            "lib/%2e%2E/x.lua",
            "lib\\..\\x.lua",
            "file:///r/notes/../x.lua",
            "/etc/passwd",
            "\\evil",
            "a\0b.lua",
        ] {
            assert!(resolve_package_url(base, bad).is_err(), "{bad:?} must be refused");
        }
        assert_eq!(resolve_package_url(base, "lib/a..b.lua").unwrap(), "file:///r/notes/lib/a..b.lua");
    }

    #[test]
    fn resolving_a_ref_refuses_one_that_lists_a_foreign_file_or_too_many() {
        let base = "https://apps.example.org/notes/app-ref.json";
        let ok = ref_listing(&["a.lua", "b/c.lua"]).resolved(base).unwrap();
        assert_eq!(ok.files[1].url, "https://apps.example.org/notes/b/c.lua");
        assert!(matches!(
            ref_listing(&["a.lua", "file:///dev/zero"]).resolved(base),
            Err(DomainError::UnsafeFileUrl { .. })
        ));
        let many = vec!["a.lua"; MAX_PACKAGE_FILES + 1];
        assert!(matches!(ref_listing(&many).resolved(base), Err(DomainError::PackageTooLarge(_))));
        assert!(ref_listing(&many[..MAX_PACKAGE_FILES]).resolved(base).is_ok());
    }

    #[test]
    fn migration_order_is_validated() {
        let mut m = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "m.lua".into(),
            permissions: vec![],
            migrations: vec![MigrationSpec { version: 2, name: "x".into(), file: "x".into() }],
            icon: None,
            tint: None,
            description: None,
            requires: None,
        };
        assert_eq!(m.validate(), Err(DomainError::MigrationOrder));
        m.migrations[0].version = 1;
        assert!(m.validate().is_ok());
    }

    #[test]
    fn the_entry_must_be_the_built_lua_file() {
        let mut m = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "app.luau".into(),
            permissions: vec![],
            migrations: vec![],
            icon: None,
            tint: None,
            description: None,
            requires: None,
        };
        let err = m.validate().unwrap_err().to_string();
        assert!(err.contains("app.luau") && err.contains("bun run build:apps"), "{err}");
        m.main = "app.lua".into();
        assert!(m.validate().is_ok());
    }
}
