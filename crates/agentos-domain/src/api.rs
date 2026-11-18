//! The host-management methods on `/rpc`: `app.list`, `app.available`, `app.preview`, `host.devices.list`.
//!
//! Next to the view protocol (`ui::protocol`) these are what the shell, not an app window, talks to the host
//! about. They are plain serde types, so the generated TypeScript (`types/generated/agentos.ts`) cannot drift
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
        /// The default window and its minimum size (only while the app runs: the app declares it).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        pub window: Option<AppWindow>,
        /// Where it was installed from, for people to read ("this computer", "apps.example.org").
        pub source: String,
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
        /// What the app asks to use: `data`, `notify`, `shell`.
        pub permissions: Vec<String>,
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
}
