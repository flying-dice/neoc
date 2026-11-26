//! The host-management methods on `/rpc`: `app.list`, `app.available`, `app.preview`, `host.devices.list`,
//! `host.files.list`, and the `app.event` notification.
//!
//! Next to the handshake and the host's notifications ([`crate::protocol`]) these are what the shell talks to the
//! host about. They are plain serde types, so the generated TypeScript (`types/generated/vos.ts`) cannot drift
//! from what the host sends.

use serde::{Deserialize, Serialize};

/// Applies the standard derives, and the codegen ones behind the feature.
macro_rules! wire {
    ($($item:item)*) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
            #[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
            $item
        )*
    };
}

wire! {
    /// Where an installed app is in its life.
    #[serde(rename_all = "lowercase")]
    pub enum AppState {
        Running,
        /// Installed, data kept, VM unloaded, tools hidden from agents.
        Disabled,
        /// Could not load; the message says why.
        Failed,
    }

    /// An app's default window, in points.
    #[serde(rename_all = "camelCase")]
    pub struct AppWindow {
        pub w: u32,
        pub h: u32,
        pub min_w: u32,
        pub min_h: u32,
    }

    /// One installed app, as `app.list` and the `app.*` notifications describe it.
    #[serde(rename_all = "camelCase")]
    pub struct AppSummary {
        pub id: String,
        pub name: String,
        pub version: String,
        pub state: AppState,
        /// Why the app failed, when `state` is `failed`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub message: Option<String>,
        /// A symbol name for the Dock and the library.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub icon: Option<String>,
        /// A palette role for the icon.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub tint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub description: Option<String>,
        /// The default window and its minimum size, when the app's manifest states one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub window: Option<AppWindow>,
        /// Where it was installed from, for people to read ("this computer", "apps.example.org").
        pub source: String,
        /// The browser features the app's frame asks for (`camera`, `microphone`, `geolocation`,
        /// `clipboard-write`): the shell puts them in the iframe's `allow` attribute.
        #[serde(default)]
        pub permissions: Vec<String>,
    }

    /// The `app.event` notification: an app sent the notification `name` with `data` (`RPC.send_notification`), for its
    /// open clients.
    pub struct AppEvent {
        pub app: String,
        pub name: String,
        #[cfg_attr(feature = "codegen", ts(type = "JsonValue"))]
        pub data: serde_json::Value,
    }

    /// `app.list` result.
    pub struct AppList {
        pub apps: Vec<AppSummary>,
    }

    /// An installed copy of an app a registry or a URL offers.
    pub struct InstalledInfo {
        pub version: String,
        pub state: AppState,
    }

    /// An app ref seen without installing it: what `app.preview` returns and what Discover lists.
    #[serde(rename_all = "camelCase")]
    pub struct AppPreview {
        /// The app-ref URL to hand to `app.install`.
        pub url: String,
        /// SHA-256 (hex) of the app-ref document this preview was made from: hand it back to `app.install`
        /// so that what is installed is what was previewed.
        pub sha256: String,
        pub id: String,
        pub name: String,
        pub version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub icon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub tint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub description: Option<String>,
        /// The host the app ref comes from ("this computer" for a file).
        pub source: String,
        /// Set when this app id is already installed on this host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub installed: Option<InstalledInfo>,
    }

    /// `app.available` result: everything the configured registries offer.
    pub struct Catalog {
        pub apps: Vec<AppPreview>,
        /// A registry or app ref that could not be read, one line each.
        pub errors: Vec<String>,
    }

    /// One paired device in `host.devices.list`. Times are unix milliseconds.
    #[serde(rename_all = "camelCase")]
    pub struct DeviceRow {
        pub id: String,
        pub name: String,
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        pub created_at: u64,
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        pub last_seen_at: u64,
        /// This is the device asking.
        pub current: bool,
    }

    /// `host.devices.list` result.
    pub struct DeviceList {
        pub devices: Vec<DeviceRow>,
    }

    /// What an entry of a folder is.
    #[serde(rename_all = "lowercase")]
    pub enum FileEntryKind {
        File,
        Dir,
        /// A symbolic link that could not be followed: in an app's Documents no link is; on the computer one that
        /// leads nowhere or in a circle.
        Link,
        /// Something else (a device, a socket): shown, never opened.
        Other,
    }

    /// One entry of a directory in `host.files.list`: a name inside the directory that was listed.
    #[serde(rename_all = "camelCase")]
    pub struct FileEntry {
        pub name: String,
        pub kind: FileEntryKind,
        /// Bytes (whatever the platform reports for a directory).
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        pub size: u64,
        /// Unix milliseconds; absent where the filesystem does not record it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
        pub modified_at: Option<u64>,
        /// The entry is a symbolic link (on the computer `kind` is then what it leads to).
        pub link: bool,
        /// Why the entry cannot be opened, when the listing already knows ("the link leads nowhere").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub problem: Option<String>,
    }

    /// `host.files.list` result: the entries of one directory, sorted by name.
    pub struct FileList {
        pub entries: Vec<FileEntry>,
        /// The directory holds more than the listing's cap: these are the first of them, and there are others.
        pub truncated: bool,
    }

    /// `host.files.locations` result: what the Files page can browse besides the installed apps' Documents.
    #[serde(rename_all = "camelCase")]
    pub struct FileLocations {
        /// Whether this host lets the Files page browse its own file system ("This Computer").
        pub computer: bool,
        /// The home directory of the user running the host, as a path to list (`/Users/me`), when there is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub home: Option<String>,
        /// The largest file an upload may carry, in bytes.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        pub max_upload_bytes: u64,
    }

    /// Which apps an agent may call (`all` is the explicit "every app" option).
    pub struct AgentGrantsRow {
        pub all: bool,
        pub apps: Vec<String>,
    }

    /// One agent on the host's roster, as `agents.list` and the `agents.*` results describe it. Tokens never
    /// come with it.
    #[serde(rename_all = "camelCase")]
    pub struct AgentRow {
        pub id: String,
        pub name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub avatar: Option<String>,
        pub role: String,
        pub grants: AgentGrantsRow,
    }

    /// `agents.list` result.
    pub struct AgentList {
        pub agents: Vec<AgentRow>,
    }
}

impl From<&crate::Agent> for AgentRow {
    fn from(a: &crate::Agent) -> Self {
        Self {
            id: a.id.0.clone(),
            name: a.name.clone(),
            avatar: a.avatar.clone(),
            role: a.role.clone(),
            grants: AgentGrantsRow {
                all: a.grants.all,
                apps: a.grants.apps.iter().map(ToString::to_string).collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Agent, AgentId, AppId, Grants};
    use serde_json::json;

    #[test]
    fn an_agent_row_is_the_contact_and_its_grants() {
        let agent = Agent {
            id: AgentId("claude".into()),
            name: "Claude".into(),
            avatar: None,
            role: "Pair".into(),
            grants: Grants::apps([AppId::parse("notes").unwrap()]),
        };
        let v = serde_json::to_value(AgentRow::from(&agent)).unwrap();
        assert_eq!(
            v,
            json!({"id": "claude", "name": "Claude", "role": "Pair", "grants": {"all": false, "apps": ["notes"]}})
        );
    }
}
