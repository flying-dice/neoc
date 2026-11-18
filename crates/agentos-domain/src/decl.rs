//! What an app declares about itself in Luau with `app.manifest { ... }`, validated here (pure).
//!
//! This is distinct from the package [`crate::Manifest`] in the app ref (registry level: files, checksums,
//! version). The two must agree on the id; the declared permissions must be a subset of what the package
//! manifest grants.

use crate::{error::DomainError, AppId, Permission};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `DESIGN_LANGUAGE`: no window is smaller than this.
pub const MIN_WINDOW_W: u32 = 320;
pub const MIN_WINDOW_H: u32 = 240;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowDecl {
    pub w: u32,
    pub h: u32,
    #[serde(rename = "minW")]
    pub min_w: u32,
    #[serde(rename = "minH")]
    pub min_h: u32,
}

/// `kit = ">=1.2"`: the component-kit version the app needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KitRequirement {
    pub major: u32,
    pub minor: u32,
}

impl KitRequirement {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let bad = || DomainError::InvalidManifest(format!("kit `{s}` must look like `>=1.0`"));
        let rest = s.trim().strip_prefix(">=").ok_or_else(bad)?;
        let (major, minor) = rest.split_once('.').ok_or_else(bad)?;
        Ok(Self { major: major.parse().map_err(|_| bad())?, minor: minor.parse().map_err(|_| bad())? })
    }

    /// Is a client with kit `major.minor` new enough? A newer major is a breaking change, so it is not.
    #[must_use]
    pub fn satisfied_by(self, major: u32, minor: u32) -> bool {
        major == self.major && minor >= self.minor
    }
}

impl TryFrom<String> for KitRequirement {
    type Error = DomainError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<KitRequirement> for String {
    fn from(k: KitRequirement) -> Self {
        format!(">={}.{}", k.major, k.minor)
    }
}

/// The app's declared identity and window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDecl {
    pub id: AppId,
    pub name: String,
    pub icon: String,
    #[serde(default)]
    pub tint: Option<String>,
    #[serde(default)]
    pub window: Option<WindowDecl>,
    #[serde(default)]
    pub kit: Option<KitRequirement>,
    /// Capability names: `data`, `notify`, `shell`.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl AppDecl {
    pub fn validate(&self) -> Result<(), DomainError> {
        let invalid = |m: String| Err(DomainError::InvalidManifest(m));
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            return invalid("name must be 1 to 60 characters".into());
        }
        if self.icon.is_empty()
            || !self
                .icon
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        {
            return invalid(format!("icon `{}` must be a lowercase symbol name", self.icon));
        }
        if let Some(w) = &self.window {
            if w.min_w < MIN_WINDOW_W || w.min_h < MIN_WINDOW_H {
                return invalid(format!("window min size must be at least {MIN_WINDOW_W}x{MIN_WINDOW_H}"));
            }
            if w.w < w.min_w || w.h < w.min_h {
                return invalid("window size must not be below its minimum".into());
            }
        }
        for p in &self.permissions {
            if Permission::capability(p).is_none() {
                return invalid(format!("unknown permission `{p}` (use data, notify or shell)"));
            }
        }
        Ok(())
    }

    /// The declared permissions as domain grants.
    #[must_use]
    pub fn grants(&self) -> Vec<Permission> {
        self.permissions.iter().filter_map(|p| Permission::capability(p)).collect()
    }

    /// Check this declaration against the package manifest it came with.
    pub fn check_against(&self, package_id: &AppId, granted: &[Permission]) -> Result<(), DomainError> {
        self.validate()?;
        if &self.id != package_id {
            return Err(DomainError::InvalidManifest(format!(
                "app.manifest id `{}` does not match the package id `{package_id}`",
                self.id
            )));
        }
        let granted: BTreeSet<_> = granted.iter().filter_map(Permission::capability_name).collect();
        for g in self.grants() {
            let name = g.capability_name().unwrap_or_default();
            if !granted.contains(name) {
                return Err(DomainError::InvalidManifest(format!(
                    "app.manifest asks for `{name}` but the package does not grant it"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(v: serde_json::Value) -> Result<AppDecl, serde_json::Error> {
        serde_json::from_value(v)
    }

    fn notes() -> serde_json::Value {
        json!({"id":"notes","name":"Notes","icon":"note.text","tint":"yellow",
               "window":{"w":960,"h":640,"minW":480,"minH":360},"kit":">=1.0","permissions":["data","notify"]})
    }

    #[test]
    fn a_valid_declaration_parses_and_validates() {
        let d = decl(notes()).unwrap();
        d.validate().unwrap();
        assert_eq!(d.kit, Some(KitRequirement { major: 1, minor: 0 }));
        assert_eq!(d.grants(), vec![Permission::Data, Permission::Notify]);
    }

    #[test]
    fn unknown_keys_and_bad_ids_are_rejected_at_parse() {
        let mut v = notes();
        v["colour"] = json!("red");
        assert!(decl(v).is_err(), "typo in a key");
        let mut v = notes();
        v["id"] = json!("Not Valid");
        assert!(decl(v).is_err());
    }

    #[test]
    fn window_minimum_follows_the_design_language() {
        let mut d = decl(notes()).unwrap();
        d.window = Some(WindowDecl { w: 300, h: 300, min_w: 300, min_h: 300 });
        assert!(d.validate().is_err(), "below 320x240");
        d.window = Some(WindowDecl { w: 400, h: 300, min_w: 480, min_h: 300 });
        assert!(d.validate().is_err(), "default smaller than minimum");
    }

    #[test]
    fn name_icon_and_permissions_are_validated() {
        let mut d = decl(notes()).unwrap();
        d.name = "  ".into();
        assert!(d.validate().is_err());
        let mut d = decl(notes()).unwrap();
        d.icon = "Has Space".into();
        assert!(d.validate().is_err());
        let mut d = decl(notes()).unwrap();
        d.permissions = vec!["root".into()];
        assert!(d.validate().is_err());
    }

    #[test]
    fn kit_requirements_parse_and_compare() {
        let k = KitRequirement::parse(">=1.2").unwrap();
        assert!(k.satisfied_by(1, 2) && k.satisfied_by(1, 9));
        assert!(!k.satisfied_by(1, 1) && !k.satisfied_by(2, 0));
        assert!(KitRequirement::parse("1.2").is_err());
        assert!(KitRequirement::parse(">=x").is_err());
    }

    #[test]
    fn the_declaration_must_agree_with_the_package() {
        let d = decl(notes()).unwrap();
        let id = AppId::parse("notes").unwrap();
        d.check_against(&id, &[Permission::Data, Permission::Notify]).unwrap();
        assert!(d
            .check_against(&AppId::parse("other").unwrap(), &[Permission::Data, Permission::Notify])
            .is_err());
        assert!(d.check_against(&id, &[Permission::Data]).is_err(), "notify is not granted by the package");
    }
}
