//! Use cases. They depend only on ports and the domain.
//!
//! Lifecycle of an app: install, enable, disable (data kept, agent access blocked), delete data,
//! reload (dev: rebuild the VM, keep the DB), uninstall. Installs are recorded in the system database and
//! restored at start; an app that cannot load is marked failed and isolated, the others carry on.

use crate::{
    files::FileStore,
    ports::{
        AppData, AppPackage, AppRuntime, CallContext, Clock, DataStore, EventSink, Fetcher, InstalledApp,
        InstalledAppStore, ServerEvent, SqlMigration, ToolRegistry,
    },
    AppError, PortError,
};
use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use vos_domain::{
    api::{AppPreview, Catalog, InstalledInfo},
    protocol::codes,
    resolve_url, source_label, Actor, AgentIdentity, AppId, AppRef, Csp, DomainError, Manifest,
    PackageBudget, Permissions, QualifiedToolName, Registry, ToolSpec, WindowDecl,
};

/// Every collaborator of [`AppService`].
pub struct Ports {
    pub fetcher: Arc<dyn Fetcher>,
    pub data: Arc<dyn DataStore>,
    /// The app's managed files, removed together with its database.
    pub files: Arc<dyn FileStore>,
    pub runtime: Arc<dyn AppRuntime>,
    pub tools: Arc<dyn ToolRegistry>,
    pub events: Arc<dyn EventSink>,
    pub installed: Arc<dyn InstalledAppStore>,
    pub clock: Arc<dyn Clock>,
    /// The catalog Discover browses: registry files (`{"apps": [...]}`) or app refs, by URL.
    pub registries: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppStatus {
    Running,
    /// Installed, data kept, VM unloaded, tools hidden from agents.
    Disabled,
    /// Could not load (the message says why); the record stays so it can be fixed, reloaded or uninstalled.
    Failed(String),
}

impl AppStatus {
    fn to_wire(&self) -> vos_domain::api::AppState {
        use vos_domain::api::AppState;
        match self {
            Self::Running => AppState::Running,
            Self::Disabled => AppState::Disabled,
            Self::Failed(_) => AppState::Failed,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Running => "running".into(),
            Self::Disabled => "disabled".into(),
            Self::Failed(why) => format!("failed ({why})"),
        }
    }
}

/// How an app presents itself in the Dock and the library, from its package manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Display {
    pub icon: Option<String>,
    pub tint: Option<String>,
    pub description: Option<String>,
    /// The default window and its minimum size, when the manifest states one.
    pub window: Option<WindowDecl>,
    /// The browser features its frame asks for (`allow` of the iframe): `camera`, `clipboard-write`, ...
    pub permissions: Vec<&'static str>,
}

impl Display {
    fn of(manifest: &Manifest) -> Self {
        Self {
            icon: manifest.icon.clone(),
            tint: manifest.tint.clone(),
            description: manifest.description.clone(),
            window: manifest.window.clone(),
            permissions: manifest.ui.permissions.features(),
        }
    }
}

/// An app's page, as fetched and verified at install: its built static files, kept in memory by their
/// path under the app's `ui.root` (`assets/a.js`). The host serves them at `/apps/<id>/static/...`; whether they
/// are kept in memory or in a directory is this type's business, not the route's.
#[derive(Clone, Debug)]
pub struct StaticSite {
    files: HashMap<String, StaticFile>,
    entry: String,
    /// What the page may load from other origins, as the app declared it.
    pub csp: Csp,
    /// The browser features the app's frame asks for.
    pub permissions: Permissions,
}

/// One verified static file.
#[derive(Clone, Debug)]
pub struct StaticFile {
    pub bytes: Arc<[u8]>,
    /// SHA-256 (hex, lowercase) of `bytes`: the file's validator.
    pub sha256: String,
}

impl StaticSite {
    /// The file at `path` (relative to the root), when the app has it. An exact match: there is no fallback page.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&StaticFile> {
        self.files.get(path)
    }

    /// The path of the page the frame loads.
    #[must_use]
    pub fn entry(&self) -> &str {
        &self.entry
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub id: AppId,
    pub name: String,
    pub version: String,
    pub source_url: String,
    pub status: AppStatus,
    pub display: Display,
}

/// The calls of one app's handlers that may run at once, whoever makes them and over however many connections: more are refused
/// at once (`-32000`, "busy") instead of queuing behind handlers that may never return. The shell's own queue per app is a courtesy
/// that stays under it; this is what holds against a client that ignores the courtesy.
pub const MAX_CALLS_PER_APP: usize = 8;

/// The places of one load of an app. A reload or an uninstall drops it and the next load starts with all of them: what the old load
/// still runs ends with it (the runtime ends a call whose app is unloaded) and gives back to a counter nobody asks any more.
#[derive(Default)]
struct CallSlots(Arc<AtomicUsize>);

/// One place held by a call that is running; given back when the call ends or is dropped (a cancelled request's task is aborted).
struct CallSlot(Arc<AtomicUsize>);

impl CallSlots {
    fn take(&self) -> Option<CallSlot> {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_CALLS_PER_APP).then_some(n + 1))
            .ok()
            .map(|_| CallSlot(self.0.clone()))
    }
}

impl Drop for CallSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A loaded app: its database, its tools and its page.
struct Active {
    calls: CallSlots,
    data: Arc<dyn AppData>,
    name: String,
    display: Display,
    tools: Vec<ToolSpec>,
    site: Arc<StaticSite>,
}

struct AppEntry {
    record: InstalledApp,
    name: String,
    display: Display,
    status: AppStatus,
    active: Option<Active>,
}

pub struct AppService {
    p: Ports,
    apps: Mutex<HashMap<AppId, AppEntry>>,
}

/// An app ref as fetched: the parsed document and the SHA-256 (hex) of the bytes it came from, which is what
/// a preview shows and an install must match.
struct FetchedRef {
    app_ref: AppRef,
    sha256: String,
}

/// What an install record keeps of a load: the version and the digest of the app ref.
// TODO: clean-code - 0.4 - NAMING: no approval is left in it, only the version and digest a load records; rename (`LoadedRevision`).
struct Approval {
    version: String,
    ref_sha256: String,
}

impl Approval {
    fn of(fetched: &FetchedRef) -> Self {
        let m = &fetched.app_ref.manifest;
        Self { version: m.version.clone(), ref_sha256: fetched.sha256.clone() }
    }

    /// Record this approval; whether the record changed.
    fn apply(self, record: &mut InstalledApp) -> bool {
        let changed = record.version != self.version || record.ref_sha256 != self.ref_sha256;
        record.version = self.version;
        record.ref_sha256 = self.ref_sha256;
        changed
    }
}

/// A reloaded app ref must still be the app that was installed: the id is what names the app's database.
fn check_against_record(record: &InstalledApp, manifest: &Manifest) -> Result<(), DomainError> {
    if manifest.id != record.id {
        return Err(DomainError::AppIdChanged {
            installed: record.id.to_string(),
            served: manifest.id.to_string(),
        });
    }
    Ok(())
}

impl AppService {
    #[must_use]
    pub fn new(ports: Ports) -> Self {
        Self { p: ports, apps: Mutex::default() }
    }

    /// The app's database handle.
    pub fn data_of(&self, app: &AppId) -> Result<Arc<dyn AppData>, AppError> {
        self.with_active(app, |a| a.data.clone())
    }

    // ---- install / uninstall ------------------------------------------------------------------------

    /// Install from an app-ref URL (file://, http://, https://: the Fetcher adapter decides how).
    pub async fn install_from_url(&self, app_ref_url: &str) -> Result<AppId, AppError> {
        let fetched = self.fetch_app_ref(app_ref_url).await?;
        self.install(fetched, app_ref_url).await
    }

    /// Install what [`AppService::preview`] showed: the app ref at `app_ref_url` must still have the
    /// `sha256` the preview reported, else nothing is installed (the person approved that one, not this).
    pub async fn install_previewed(&self, app_ref_url: &str, sha256: &str) -> Result<AppId, AppError> {
        let fetched = self.fetch_app_ref(app_ref_url).await?;
        if !fetched.sha256.eq_ignore_ascii_case(sha256) {
            return Err(DomainError::RefChanged {
                expected: sha256.to_ascii_lowercase(),
                actual: fetched.sha256,
            }
            .into());
        }
        self.install(fetched, app_ref_url).await
    }

    /// Install an app ref obtained from `source_url`, recording what was approved, and remember it across
    /// restarts.
    async fn install(&self, fetched: FetchedRef, source_url: &str) -> Result<AppId, AppError> {
        let app_ref = &fetched.app_ref;
        let m = &app_ref.manifest;
        m.validate()?;
        if let Some(existing) = self.apps.lock().get(&m.id) {
            if !matches!(existing.status, AppStatus::Failed(_)) {
                return Err(AppError::AlreadyInstalled(m.id.to_string()));
            }
        }
        // fetch + verify everything and load it before any state is recorded: a bad app leaves nothing behind
        let active = self.activate(app_ref, None).await?;
        let record = InstalledApp {
            id: m.id.clone(),
            source_url: source_url.to_owned(),
            version: m.version.clone(),
            enabled: true,
            installed_at_ms: self.p.clock.now_unix_ms(),
            ref_sha256: fetched.sha256.clone(),
        };
        if let Err(e) = self.p.installed.save(&record).await {
            self.deactivate(&m.id, active).await;
            return Err(e.into());
        }
        let (name, display) = (active.name.clone(), active.display.clone());
        self.apps.lock().insert(
            m.id.clone(),
            AppEntry { record, name, display, status: AppStatus::Running, active: Some(active) },
        );
        self.p.events.emit(ServerEvent::AppInstalled(m.id.clone())).await;
        Ok(m.id.clone())
    }

    pub async fn uninstall(&self, app: &AppId, purge_data: bool) -> Result<(), AppError> {
        let entry = self.apps.lock().remove(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
        if let Some(active) = entry.active {
            self.deactivate(app, active).await;
        }
        self.p.installed.remove(app).await?;
        if purge_data {
            self.purge_data(app).await?;
        }
        self.p.events.emit(ServerEvent::AppUninstalled(app.clone())).await;
        Ok(())
    }

    // ---- start-up -----------------------------------------------------------------------------------

    /// Load every app the system database remembers. A disabled app stays unloaded; one that cannot load
    /// is marked failed (and announced) while the rest start normally.
    pub async fn restore_installed(&self) -> Result<Vec<AppInfo>, AppError> {
        for mut record in self.p.installed.list().await? {
            let id = record.id.clone();
            if self.apps.lock().contains_key(&id) {
                continue;
            }
            let (status, active, name, display) = if record.enabled {
                match self.load_from_record(&record, None).await {
                    Ok((active, approval)) => {
                        if approval.apply(&mut record) {
                            if let Err(e) = self.p.installed.save(&record).await {
                                tracing::warn!(app = %id, error = %e, "could not record the reloaded app ref");
                            }
                        }
                        let (name, display) = (active.name.clone(), active.display.clone());
                        (AppStatus::Running, Some(active), name, display)
                    }
                    Err(e) => {
                        tracing::warn!(app = %id, error = %e, "installed app failed to load");
                        let (name, display) = self.package_display(&record).await;
                        (AppStatus::Failed(e.to_string()), None, name, display)
                    }
                }
            } else {
                let (name, display) = self.package_display(&record).await;
                (AppStatus::Disabled, None, name, display)
            };
            if let AppStatus::Failed(message) = &status {
                self.p
                    .events
                    .emit(ServerEvent::AppFailed { app: id.clone(), message: message.clone() })
                    .await;
            }
            self.apps.lock().insert(id, AppEntry { record, name, display, status, active });
        }
        Ok(self.apps())
    }

    /// Make sure every one of these app-ref URLs is installed: install the missing ones, retry the failed ones.
    /// A failure is logged and skipped so one broken URL cannot keep the host from starting.
    pub async fn ensure_installed(&self, urls: &[String]) {
        for url in urls {
            let known = self
                .apps
                .lock()
                .iter()
                .find(|(_, e)| e.record.source_url == *url)
                .map(|(id, e)| (id.clone(), e.status.clone()));
            let outcome = match known {
                Some((id, AppStatus::Failed(_))) => self.reload(&id).await.map(|()| id),
                Some(_) => continue,
                None => self.install_from_url(url).await,
            };
            match outcome {
                Ok(app) => tracing::info!(%app, %url, "app ready"),
                Err(e) => tracing::warn!(%url, error = %e, "could not install app from registry"),
            }
        }
    }

    // ---- enable / disable / delete data / reload ------------------------------------------------------

    /// Hide the app from agents and unload its VM. Its data is kept.
    pub async fn disable(&self, app: &AppId) -> Result<(), AppError> {
        let active = {
            let mut apps = self.apps.lock();
            let entry = apps.get_mut(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
            if entry.status == AppStatus::Disabled {
                return Ok(());
            }
            entry.status = AppStatus::Disabled;
            entry.record.enabled = false;
            entry.active.take()
        };
        if let Some(active) = active {
            self.deactivate(app, active).await;
        }
        self.p.installed.set_enabled(app, false).await?;
        self.p.events.emit(ServerEvent::AppDisabled(app.clone())).await;
        Ok(())
    }

    /// Load a disabled (or failed) app again from where it was installed.
    pub async fn enable(&self, app: &AppId) -> Result<(), AppError> {
        let record = {
            let apps = self.apps.lock();
            let entry = apps.get(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
            if entry.status == AppStatus::Running {
                return Ok(());
            }
            entry.record.clone()
        };
        match self.load_from_record(&record, None).await {
            Ok((active, approval)) => {
                self.p.installed.set_enabled(app, true).await?;
                self.remember(app, approval).await;
                if let Some(entry) = self.apps.lock().get_mut(app) {
                    entry.name.clone_from(&active.name);
                    entry.display = active.display.clone();
                    entry.active = Some(active);
                    entry.status = AppStatus::Running;
                    entry.record.enabled = true;
                }
                self.p.events.emit(ServerEvent::AppEnabled(app.clone())).await;
                Ok(())
            }
            Err(e) => {
                self.fail(app, &e).await;
                Err(e)
            }
        }
    }

    /// The app's database and its managed files. Removing the files is best effort, as removing the database is
    /// (the SQLite adapter ignores what it cannot delete): a failure is logged and the uninstall or "delete data"
    /// still finishes its state changes and events, rather than stopping half way with the app gone or without a VM.
    async fn purge_data(&self, app: &AppId) -> Result<(), AppError> {
        self.p.data.purge(app).await?;
        if let Err(e) = self.p.files.purge(app).await {
            tracing::warn!(%app, error = %e, "removing the app's files failed: what could not be removed stays on disk");
        }
        Ok(())
    }

    /// Erase the app's data. A running app is reloaded on an empty database (its migrations run again).
    pub async fn delete_data(&self, app: &AppId) -> Result<(), AppError> {
        let (was_running, record, active) = {
            let mut apps = self.apps.lock();
            let entry = apps.get_mut(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
            (entry.status == AppStatus::Running, entry.record.clone(), entry.active.take())
        };
        if let Some(active) = active {
            self.deactivate(app, active).await;
        }
        self.purge_data(app).await?;
        if !was_running {
            return Ok(());
        }
        match self.load_from_record(&record, None).await {
            Ok((active, approval)) => {
                self.remember(app, approval).await;
                if let Some(entry) = self.apps.lock().get_mut(app) {
                    entry.active = Some(active);
                }
                Ok(())
            }
            Err(e) => {
                self.fail(app, &e).await;
                Err(e)
            }
        }
    }

    /// Dev hot reload: rebuild the app from its source, keeping its database. If the new code does not
    /// load, the running version keeps serving and the error is returned.
    pub async fn reload(&self, app: &AppId) -> Result<(), AppError> {
        let (record, data, running) = {
            let apps = self.apps.lock();
            let entry = apps.get(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
            if entry.status == AppStatus::Disabled {
                return Err(AppError::NotRunning { app: app.to_string(), state: entry.status.describe() });
            }
            (
                entry.record.clone(),
                entry.active.as_ref().map(|a| a.data.clone()),
                entry.status == AppStatus::Running,
            )
        };
        match self.load_from_record(&record, data).await {
            Ok((active, approval)) => {
                self.remember(app, approval).await;
                let mut apps = self.apps.lock();
                if let Some(entry) = apps.get_mut(app) {
                    entry.name.clone_from(&active.name);
                    entry.display = active.display.clone();
                    entry.active = Some(active);
                    entry.status = AppStatus::Running;
                }
                Ok(())
            }
            Err(e) => {
                if !running {
                    self.fail(app, &e).await;
                }
                Err(e)
            }
        }
    }

    // ---- running apps -------------------------------------------------------------------------------

    /// MCP `tools/call`: authorize the calling agent, then run the tool in the app's VM as that agent.
    pub async fn call_tool(
        &self,
        caller: &AgentIdentity,
        name: &QualifiedToolName,
        args: Value,
    ) -> Result<Value, AppError> {
        caller.authorize_tool_call(name)?;
        self.require_running(name.app())?;
        let ctx = CallContext { actor: Actor::from(caller), now_ms: self.p.clock.now_unix_ms() };
        // the runtime enforces `readOnly` too, but the guarantee must not rest on the adapter alone: it is
        // handed the requirement for every tool the app declared `readOnly`
        let read_only = self.tool_is_read_only(name).unwrap_or(false);
        Ok(self.p.runtime.call_tool(name.app(), name.tool(), args, ctx, read_only).await?)
    }

    /// An app page's `rpc(method, args)`: run the app's handler of that name in the app's VM. The call is the
    /// person's own (the shell sends it from the app's frame, naming the app that frame belongs to), so it may
    /// write; an app that is not running answers with an error.
    pub async fn call_handler(&self, app: &AppId, method: &str, args: Value) -> Result<Value, AppError> {
        self.require_running(app)?;
        let _slot = self.with_active(app, |a| a.calls.take())?.ok_or_else(|| {
            PortError::Rpc {
                code: codes::BUSY,
                message: format!(
                    "`{app}` already has {MAX_CALLS_PER_APP} calls running: let one finish, or end one, before sending another"
                ),
                data: None,
            }
        })?;
        let ctx = CallContext { actor: Actor::user("You"), now_ms: self.p.clock.now_unix_ms() };
        Ok(self.p.runtime.call_handler(app, method, args, ctx).await?)
    }

    /// The static files of a running app's page, for its frame.
    pub fn static_site(&self, app: &AppId) -> Result<Arc<StaticSite>, AppError> {
        self.with_active(app, |a| a.site.clone())
    }

    /// Whether the app is installed, in any state (a disabled or failed app still has its files).
    #[must_use]
    pub fn is_installed(&self, app: &AppId) -> bool {
        self.apps.lock().contains_key(app)
    }

    /// Whether the app's tool is declared `readOnly`; `None` when the app is not running or has no such tool.
    #[must_use]
    pub fn tool_is_read_only(&self, tool: &QualifiedToolName) -> Option<bool> {
        self.with_active(tool.app(), |a| a.tools.iter().find(|t| t.name == tool.tool()).map(|t| t.read_only))
            .ok()
            .flatten()
    }

    /// The tools the app currently publishes.
    pub fn tools(&self, app: &AppId) -> Result<Vec<ToolSpec>, AppError> {
        self.with_active(app, |a| a.tools.clone())
    }

    // ---- inspection ---------------------------------------------------------------------------------

    /// Every installed app, whatever its status, sorted by id.
    pub fn apps(&self) -> Vec<AppInfo> {
        let mut v: Vec<AppInfo> = self
            .apps
            .lock()
            .values()
            .map(|e| AppInfo {
                id: e.record.id.clone(),
                name: e.name.clone(),
                version: e.record.version.clone(),
                source_url: e.record.source_url.clone(),
                status: e.status.clone(),
                display: e.display.clone(),
            })
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Ids of the installed apps, sorted.
    pub fn installed_apps(&self) -> Vec<AppId> {
        self.apps().into_iter().map(|a| a.id).collect()
    }

    pub fn status(&self, app: &AppId) -> Option<AppStatus> {
        self.apps.lock().get(app).map(|e| e.status.clone())
    }

    // ---- shutdown -----------------------------------------------------------------------------------

    /// Host shutdown: retract every app's tools and unload its VM (data is kept). Nothing is purged or forgotten and clients are not told the apps were uninstalled: they
    /// are coming back with the host.
    pub async fn shutdown(&self) {
        let entries: Vec<(AppId, Option<Active>)> =
            self.apps.lock().drain().map(|(id, e)| (id, e.active)).collect();
        for (app, active) in entries {
            if let Some(active) = active {
                self.deactivate(&app, active).await;
            }
        }
    }

    // ---- internals ----------------------------------------------------------------------------------

    fn require_running(&self, app: &AppId) -> Result<(), AppError> {
        match self.apps.lock().get(app) {
            None => Err(AppError::NotInstalled(app.to_string())),
            Some(e) if e.status == AppStatus::Running => Ok(()),
            Some(e) => Err(AppError::NotRunning { app: app.to_string(), state: e.status.describe() }),
        }
    }

    fn with_active<T>(&self, app: &AppId, f: impl FnOnce(&Active) -> T) -> Result<T, AppError> {
        let apps = self.apps.lock();
        let entry = apps.get(app).ok_or_else(|| AppError::NotInstalled(app.to_string()))?;
        match &entry.active {
            Some(active) => Ok(f(active)),
            None => Err(AppError::NotRunning { app: app.to_string(), state: entry.status.describe() }),
        }
    }

    async fn fail(&self, app: &AppId, error: &AppError) {
        if let Some(entry) = self.apps.lock().get_mut(app) {
            entry.status = AppStatus::Failed(error.to_string());
            entry.active = None;
        }
        self.p.events.emit(ServerEvent::AppFailed { app: app.clone(), message: error.to_string() }).await;
    }

    /// Fetch `url` and refuse a document over the domain's file size cap.
    async fn fetch_capped(&self, url: &str) -> Result<Vec<u8>, AppError> {
        let bytes = self.p.fetcher.fetch(url).await?;
        PackageBudget::default().charge(url, bytes.len())?;
        Ok(bytes)
    }

    async fn fetch_app_ref(&self, url: &str) -> Result<FetchedRef, AppError> {
        let bytes = self.fetch_capped(url).await?;
        let app_ref: AppRef = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::Port(PortError::Rejected(format!("bad app ref at {url}: {e}"))))?;
        Ok(FetchedRef { app_ref: app_ref.resolved(url)?, sha256: format!("{:x}", Sha256::digest(&bytes)) })
    }

    /// Keep the version and digest of the app ref that just loaded in the app's install record.
    async fn remember(&self, app: &AppId, approval: Approval) {
        let updated = {
            let mut apps = self.apps.lock();
            apps.get_mut(app).and_then(|e| approval.apply(&mut e.record).then(|| e.record.clone()))
        };
        if let Some(record) = updated {
            if let Err(e) = self.p.installed.save(&record).await {
                tracing::warn!(%app, error = %e, "could not record the reloaded app ref");
            }
        }
    }

    /// The name and presentation of an app that is not running, from its package manifest (best effort: an
    /// unreachable source leaves the defaults).
    async fn package_display(&self, record: &InstalledApp) -> (String, Display) {
        match self.fetch_app_ref(&record.source_url).await {
            Ok(f) => (f.app_ref.manifest.name.clone(), Display::of(&f.app_ref.manifest)),
            Err(_) => (record.id.to_string(), Display::default()),
        }
    }

    // ---- preview and catalog --------------------------------------------------------------------------

    /// What an app ref is, without installing it or running anything: its manifest, where it comes from and
    /// whether this host already has the app. Nothing but the app ref itself is fetched.
    pub async fn preview(&self, app_ref_url: &str) -> Result<AppPreview, AppError> {
        let fetched = self.fetch_app_ref(app_ref_url).await?;
        fetched.app_ref.manifest.validate()?;
        Ok(self.preview_of(&fetched, app_ref_url))
    }

    fn preview_of(&self, fetched: &FetchedRef, url: &str) -> AppPreview {
        let m = &fetched.app_ref.manifest;
        let installed = self
            .apps
            .lock()
            .get(&m.id)
            .map(|e| InstalledInfo { version: e.record.version.clone(), state: e.status.to_wire() });
        AppPreview {
            url: url.to_owned(),
            sha256: fetched.sha256.clone(),
            id: m.id.to_string(),
            name: m.name.clone(),
            version: m.version.clone(),
            icon: m.icon.clone(),
            tint: m.tint.clone(),
            description: m.description.clone(),
            source: source_label(url),
            installed,
        }
    }

    /// Everything the configured registries offer. A registry (or one of its apps) that cannot be read is
    /// reported in `errors` and skipped: the rest still list.
    pub async fn catalog(&self) -> Catalog {
        let mut catalog = Catalog { apps: vec![], errors: vec![] };
        let mut urls = Vec::new();
        for registry in &self.p.registries {
            match self.registry_entries(registry).await {
                Ok(found) => urls.extend(found),
                Err(e) => catalog.errors.push(format!("{registry}: {e}")),
            }
        }
        for url in urls {
            match self.fetch_app_ref(&url).await {
                Ok(fetched) if fetched.app_ref.manifest.validate().is_ok() => {
                    if !catalog.apps.iter().any(|a| a.id == fetched.app_ref.manifest.id.as_str()) {
                        catalog.apps.push(self.preview_of(&fetched, &url));
                    }
                }
                Ok(fetched) => {
                    catalog
                        .errors
                        .push(format!("{url}: {} needs a newer host", fetched.app_ref.manifest.name));
                }
                Err(e) => catalog.errors.push(format!("{url}: {e}")),
            }
        }
        catalog
    }

    /// The app-ref URLs a registry URL stands for: the apps of a registry file, or the URL itself when it is
    /// an app ref.
    async fn registry_entries(&self, url: &str) -> Result<Vec<String>, AppError> {
        let bytes = self.fetch_capped(url).await?;
        if let Ok(registry) = serde_json::from_slice::<Registry>(&bytes) {
            return Ok(registry.apps.iter().map(|a| resolve_url(url, a)).collect());
        }
        if serde_json::from_slice::<AppRef>(&bytes).is_ok() {
            return Ok(vec![url.to_owned()]);
        }
        Err(PortError::Rejected("not a registry file or an app ref".into()).into())
    }

    /// Load an installed app again from where it was installed. What the source serves now must still be
    /// that app; the returned approval is what the record keeps of this load.
    async fn load_from_record(
        &self,
        record: &InstalledApp,
        reuse: Option<Arc<dyn AppData>>,
    ) -> Result<(Active, Approval), AppError> {
        let fetched = self.fetch_app_ref(&record.source_url).await?;
        check_against_record(record, &fetched.app_ref.manifest)?;
        let active = self.activate(&fetched.app_ref, reuse).await?;
        Ok((active, Approval::of(&fetched)))
    }

    /// Fetch and verify the package (the backend, the page's files, the migrations), open the database, run the
    /// backend and publish its tools. Nothing is left behind on failure.
    async fn activate(&self, app_ref: &AppRef, reuse: Option<Arc<dyn AppData>>) -> Result<Active, AppError> {
        let m = &app_ref.manifest;
        m.validate()?;
        let mut budget = PackageBudget::default();
        let mut package = AppPackage { entry: m.main.clone(), ..AppPackage::default() };
        // an app's backend is built into ONE Luau file: the entry is the only source the runtime is handed
        package.files.insert(m.main.clone(), self.fetch_verified(app_ref, &m.main, &mut budget).await?);
        // an app's page is its built static files, verified like the rest and kept for the frame to load
        let ui = &m.ui;
        let mut files = HashMap::new();
        for file in &app_ref.files {
            let Some(inside) = ui.under_root(&file.path) else { continue };
            let bytes = self.fetch_verified(app_ref, &file.path, &mut budget).await?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            files.insert(inside.to_owned(), StaticFile { bytes: bytes.into(), sha256 });
        }
        if !files.contains_key(&ui.entry) {
            return Err(DomainError::InvalidManifest(format!(
                "ui.entry `{}` is not among the app ref's files under `{}`",
                ui.entry, ui.root
            ))
            .into());
        }
        let site = Arc::new(StaticSite {
            files,
            entry: ui.entry.clone(),
            csp: ui.csp.clone(),
            permissions: ui.permissions.clone(),
        });
        let mut migrations = Vec::new();
        for spec in &m.migrations {
            let sql = String::from_utf8(self.fetch_verified(app_ref, &spec.file, &mut budget).await?)
                .map_err(|_| PortError::Rejected(format!("{} is not UTF-8", spec.file)))?;
            migrations.push(SqlMigration { version: spec.version, name: spec.name.clone(), sql });
        }

        let data = match reuse {
            Some(d) => d,
            None => self.p.data.open(&m.id, migrations).await?,
        };
        // a failed load leaves the data in place (it is the user's) and the previous VM, if any, untouched
        let loaded = self.p.runtime.load(&m.id, package, data.clone()).await?;
        if let Err(e) = self.p.tools.publish(&m.id, loaded.tools.clone()).await {
            self.p.runtime.unload(&m.id).await.ok();
            return Err(e.into());
        }
        Ok(Active {
            calls: CallSlots::default(),
            data,
            name: m.name.clone(),
            display: Display::of(m),
            tools: loaded.tools,
            site,
        })
    }

    /// Retract tools, stop serving and unload the VM. Failures are logged, not raised: teardown must finish.
    async fn deactivate(&self, app: &AppId, active: Active) {
        if let Err(e) = self.p.tools.retract(app).await {
            tracing::warn!(%app, error = %e, "retracting tools failed");
        }
        drop(active);
        if let Err(e) = self.p.runtime.unload(app).await {
            tracing::warn!(%app, error = %e, "unloading app failed");
        }
    }

    async fn fetch_verified(
        &self,
        app_ref: &AppRef,
        path: &str,
        budget: &mut PackageBudget,
    ) -> Result<Vec<u8>, AppError> {
        let url = &app_ref.file(path)?.url;
        let bytes = self.p.fetcher.fetch(url).await?;
        budget.charge(path, bytes.len())?;
        app_ref.verify(path, &bytes)?;
        Ok(bytes)
    }
}
