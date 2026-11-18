use crate::{error::DomainError, KitRequirement, Permission};
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
    /// Luau entry file, a path in the app ref.
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
/// relative one is looked up next to the document (`dir/app-ref.json` + `app.luau` = `dir/app.luau`).
#[must_use]
pub fn resolve_url(base: &str, file: &str) -> String {
    if file.contains("://") {
        return file.to_owned();
    }
    let dir = base.rsplit_once('/').map_or(base, |(dir, _)| dir);
    format!("{dir}/{}", file.trim_start_matches("./"))
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
    #[must_use]
    pub fn resolved(mut self, base: &str) -> Self {
        for file in &mut self.files {
            file.url = resolve_url(base, &file.url);
        }
        self
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
                main: "main.luau".into(),
                permissions: vec![],
                migrations: vec![],
                icon: None,
                tint: None,
                description: None,
                requires: None,
            },
            files: vec![FileRef {
                path: "main.luau".into(),
                url: "file:///x".into(),
                // sha256("abc")
                sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            }],
        };
        assert!(r.verify("main.luau", b"abc").is_ok());
        assert!(matches!(r.verify("main.luau", b"abd"), Err(DomainError::ChecksumMismatch { .. })));
        assert!(matches!(r.verify("nope", b""), Err(DomainError::MissingFile(_))));
    }

    #[test]
    fn host_requirements_are_checked_against_the_host_api() {
        let mut m = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "m".into(),
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
        assert_eq!(resolve_url("file:///r/calc/app-ref.json", "app.luau"), "file:///r/calc/app.luau");
        assert_eq!(resolve_url("https://x.org/a/ref.json", "./b/c.luau"), "https://x.org/a/b/c.luau");
        assert_eq!(resolve_url("file:///r/ref.json", "https://y.org/z"), "https://y.org/z");
        assert_eq!(source_label("https://apps.example.org:8443/x/ref.json"), "apps.example.org:8443");
        assert_eq!(source_label("file:///r/ref.json"), "this computer");
    }

    #[test]
    fn migration_order_is_validated() {
        let mut m = Manifest {
            id: AppId::parse("notes").unwrap(),
            name: "n".into(),
            version: "1".into(),
            main: "m".into(),
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
}
