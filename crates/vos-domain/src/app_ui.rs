//! What an app's client declares about its page: where its built files are, and what the page needs from the
//! browser. The names follow MCP Apps' `_meta.ui` (`csp`, `permissions`) so an app's descriptor reads like one.
//!
//! This is a declaration the host applies to the page's content security policy and the frame's `allow` attribute.
//! It is not a permission system: nothing here asks the person, grants anything or can be revoked.

use crate::error::DomainError;
use serde::{Deserialize, Serialize};

/// `ui` in the package manifest: an app with a client of its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppUi {
    /// The directory of the app ref that holds the built client (what `vite build` wrote): every file of the ref
    /// under it is served at `/apps/<id>/static/<path under the root>`.
    pub root: String,
    /// The page the frame loads, a path under `root` (`index.html`).
    #[serde(default = "default_entry")]
    pub entry: String,
    /// What the page may load from other origins.
    #[serde(default, skip_serializing_if = "Csp::is_empty")]
    pub csp: Csp,
    /// What browser features the frame asks for.
    #[serde(default, skip_serializing_if = "Permissions::is_empty")]
    pub permissions: Permissions,
}

fn default_entry() -> String {
    "index.html".to_owned()
}

/// `csp` of MCP Apps' `_meta.ui`. `resource_domains` (images, media, fonts and styles) is applied; the other
/// three are parsed and **not applied**: an app's calls go through the host and its backend can make web requests.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Csp {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connect_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_uri_domains: Vec<String>,
}

impl Csp {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Csp::default()
    }
}

/// `{}`: the value of a permission in MCP Apps (`"camera": {}`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Granted {}

/// `permissions` of MCP Apps' `_meta.ui`: the browser features the frame asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Permissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<Granted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microphone: Option<Granted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geolocation: Option<Granted>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard_write: Option<Granted>,
}

impl Permissions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Permissions::default()
    }

    /// The Permissions Policy features these name, for the iframe's `allow` attribute.
    #[must_use]
    pub fn features(&self) -> Vec<&'static str> {
        [
            (self.camera.is_some(), "camera"),
            (self.microphone.is_some(), "microphone"),
            (self.geolocation.is_some(), "geolocation"),
            (self.clipboard_write.is_some(), "clipboard-write"),
        ]
        .into_iter()
        .filter_map(|(asked, feature)| asked.then_some(feature))
        .collect()
    }
}

impl AppUi {
    /// Self-consistency of the declaration (before anything is fetched).
    pub fn validate(&self) -> Result<(), DomainError> {
        let bad = |what: &str| DomainError::InvalidManifest(format!("ui.{what}"));
        if !is_relative_path(&self.root) {
            return Err(bad(&format!("root must be a directory inside the app ref, not `{}`", self.root)));
        }
        if !is_relative_path(&self.entry)
            || !std::path::Path::new(&self.entry).extension().is_some_and(|e| e.eq_ignore_ascii_case("html"))
        {
            return Err(bad(&format!("entry must be an .html file under the root, not `{}`", self.entry)));
        }
        let lists = [
            ("connectDomains", &self.csp.connect_domains),
            ("resourceDomains", &self.csp.resource_domains),
            ("frameDomains", &self.csp.frame_domains),
            ("baseUriDomains", &self.csp.base_uri_domains),
        ];
        for (name, list) in lists {
            for origin in list {
                if !is_origin(origin) {
                    return Err(bad(&format!(
                        "csp.{name} must hold origins such as https://cdn.example.com or https://*.example.com \
(no bare scheme, no `*`), not `{origin}`"
                    )));
                }
            }
        }
        Ok(())
    }

    /// The path of an app ref file inside the root (`static/assets/a.js` is `assets/a.js`), when it is one.
    #[must_use]
    pub fn under_root<'a>(&self, path: &'a str) -> Option<&'a str> {
        path.strip_prefix(self.root.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
            .filter(|r| !r.is_empty())
    }
}

/// A relative path of plain segments: no `..`, `.`, empty or hidden segment, no backslash, no control character.
#[must_use]
pub fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|segment| !segment.is_empty() && !segment.starts_with('.'))
}

/// An origin an app may name: `http(s)://host[:port]`, or `https://*.domain.tld` for the subdomains of one domain.
/// Never a bare scheme, a `*`, a path or a wildcard over a whole top-level domain.
#[must_use]
pub fn is_origin(origin: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else { return false };
    if scheme != "https" && scheme != "http" {
        return false;
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    if let Some(port) = port {
        if port.is_empty()
            || port.len() > 5
            || !port.bytes().all(|b| b.is_ascii_digit())
            || port.parse::<u32>().is_ok_and(|p| p == 0 || p > 65_535)
        {
            return false;
        }
    }
    let (wildcard, name) = match host.strip_prefix("*.") {
        Some(name) => (true, name),
        None => (false, host),
    };
    let labels: Vec<&str> = name.split('.').collect();
    let plain = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !labels.iter().all(plain) {
        return false;
    }
    let numeric = labels.len() == 4 && labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_digit()));
    if wildcard {
        // `*.example.com`, not `*.com` and not an address
        return labels.len() >= 2
            && !numeric
            && labels
                .last()
                .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
    }
    labels.len() >= 2
        && (numeric
            || labels
                .last()
                .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())))
        || name.eq_ignore_ascii_case("localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui(root: &str, entry: &str) -> AppUi {
        AppUi {
            root: root.into(),
            entry: entry.into(),
            csp: Csp::default(),
            permissions: Permissions::default(),
        }
    }

    #[test]
    fn origins_are_real_hosts_and_never_open_everything() {
        for ok in [
            "https://cdn.example.com",
            "https://*.example.com",
            "https://cdn.example.com:8443",
            "http://192.168.1.20:8080",
            "http://localhost:3000",
            "https://*.images.example.org",
        ] {
            assert!(is_origin(ok), "{ok}");
        }
        for bad in [
            "*",
            "https:",
            "https://",
            "https://*",
            "https://*.com",
            "https://*.*.com",
            "https://example.com/path",
            "https://example.com/",
            "ftp://example.com",
            "data:",
            "example.com",
            "'unsafe-inline'",
            "https://exa mple.com",
            "https://example.com;script-src *",
            "https://example.com:0",
            "https://example.com:99999",
            "https://*.192.168.1.20",
            "https://-bad.example.com",
        ] {
            assert!(!is_origin(bad), "{bad}");
        }
    }

    #[test]
    fn the_root_and_entry_are_plain_paths_inside_the_ref() {
        assert!(ui("static", "index.html").validate().is_ok());
        assert!(ui("client/dist", "app/index.html").validate().is_ok());
        for bad in ["", "/etc", "../x", "a//b", "a/./b", ".hidden", "a\\b", "a/../b"] {
            assert!(ui(bad, "index.html").validate().is_err(), "root {bad}");
        }
        for bad in ["", "index.js", "../index.html", "/index.html", ".index.html"] {
            assert!(ui("static", bad).validate().is_err(), "entry {bad}");
        }
        let mut with_csp = ui("static", "index.html");
        with_csp.csp.resource_domains = vec!["*".into()];
        assert!(with_csp.validate().unwrap_err().to_string().contains("resourceDomains"));
    }

    #[test]
    fn files_under_the_root_are_named_by_their_path_in_it() {
        let u = ui("static", "index.html");
        assert_eq!(u.under_root("static/assets/a.js"), Some("assets/a.js"));
        assert_eq!(u.under_root("static/index.html"), Some("index.html"));
        assert_eq!(u.under_root("static"), None);
        assert_eq!(u.under_root("static/"), None);
        assert_eq!(u.under_root("staticfoo/a.js"), None);
        assert_eq!(u.under_root("app.luau"), None);
    }

    #[test]
    fn permissions_name_their_policy_features_and_unknown_ones_are_refused() {
        let p: Permissions = serde_json::from_str(r#"{"camera":{},"clipboardWrite":{}}"#).unwrap();
        assert_eq!(p.features(), ["camera", "clipboard-write"]);
        assert!(serde_json::from_str::<Permissions>(r#"{"nfc":{}}"#).is_err());
        assert!(serde_json::from_str::<Permissions>(r#"{"camera":{"x":1}}"#).is_err());
        let ui: AppUi = serde_json::from_str(r#"{"root":"static"}"#).unwrap();
        assert_eq!(ui.entry, "index.html");
    }
}
