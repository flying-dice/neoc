//! `AppRuntime` port implementation: a map of app id -> sandboxed VM.
//!
//! An app that fails (it does not load, or a call raises) fails alone: its error is returned
//! to that caller with the app's file and line, and no other VM, and not the host, is touched.

use crate::vm::{AppVm, Effect, ShellEffect, VmConfig, VmError};
use async_trait::async_trait;
use neoc_vm::lua::lifecycle::Life;
use parking_lot::RwLock;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock, Weak},
};
use vos_application::{
    live::SourceId,
    ports::{
        AgentRoster, AppActivity, AppData, AppPackage, AppRuntime, CallContext, LoadedApp, Notifier,
        PortResult, RenderOutput, Shell, ViewEffect, ViewEventCall,
    },
    FileStore, NoFiles, PortError,
};
use vos_domain::AppId;

/// The generated constructors (`types/generated/ui.lua`) every app gets as `require("vos:ui")`. Luau source of
/// `vos:ui` (hosts and tests that build a [`VmConfig`] by hand register it as `ui`).
pub const UI_MODULE: &str = include_str!("../../../types/generated/ui.lua");

pub struct LuaRuntime {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    shell: Arc<dyn Shell>,
    host_modules: HashMap<String, Vec<u8>>,
    roster: Option<Arc<dyn AgentRoster>>,
    /// Where each app's `app.files` live (none unless the host gives a store).
    files: Arc<dyn FileStore>,
    /// Who hears that a background call settled; bound once the application exists (see [`Self::bind_activity`]).
    activity: Arc<OnceLock<Weak<dyn AppActivity>>>,
}

impl LuaRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, shell: Arc<dyn Shell>) -> Self {
        let host_modules = HashMap::from([("ui".to_owned(), UI_MODULE.as_bytes().to_vec())]);
        Self {
            vms: RwLock::default(),
            notifier,
            shell,
            host_modules,
            roster: None,
            files: Arc::new(NoFiles),
            activity: Arc::default(),
        }
    }

    /// The agent contacts `app.agents:list()` shows to every app.
    #[must_use]
    pub fn with_roster(mut self, roster: Arc<dyn AgentRoster>) -> Self {
        self.roster = Some(roster);
        self
    }

    /// Where every app's managed files (`app.files`) live: the same store the application purges them from.
    #[must_use]
    pub fn with_files(mut self, files: Arc<dyn FileStore>) -> Self {
        self.files = files;
        self
    }

    /// Tell this runtime who to inform when an app's background work (a task, a request to its own server)
    /// changed something. The runtime is built before the application that listens, so this is bound later and
    /// holds the listener weakly. The first binding wins.
    pub fn bind_activity(&self, activity: Weak<dyn AppActivity>) {
        let _ = self.activity.set(activity);
    }

    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.vms.read().get(app).cloned().ok_or_else(|| PortError::NotFound(app.to_string()))
    }
}

/// Forward what the app asked for during a call: notifications and shell declarations go to their ports.
async fn forward_effects(
    notifier: &dyn Notifier,
    shell: &dyn Shell,
    app: &AppId,
    vm: &AppVm,
) -> PortResult<()> {
    deliver_effects(notifier, shell, app, vm.take_effects(), vm.life()).await
}

/// Deliver effects already taken from the VM, so a caller need not keep the VM alive while the ports work.
async fn deliver_effects(
    notifier: &dyn Notifier,
    shell: &dyn Shell,
    app: &AppId,
    effects: Vec<Effect>,
    life: &Life,
) -> PortResult<()> {
    for effect in effects {
        // An app that was unloaded (or replaced) while a call was in flight has nothing more to say: what it
        // queued is dropped, not delivered to a shell and a notification list it no longer belongs to.
        if life.is_closed() {
            break;
        }
        match effect {
            Effect::Notify(n) => notifier.notify(n).await?,
            Effect::Shell(ShellEffect::DeepLinks(l)) => shell.set_deep_links(app, l).await,
            Effect::OpenWindow { name, params } => {
                let params = match params {
                    Value::Object(map) => map,
                    _ => Map::new(),
                };
                shell.open_window(app, &name, params).await;
            }
        }
    }
    Ok(())
}

fn port_error(app: &AppId, e: VmError) -> PortError {
    match e {
        VmError::NoSuchTool(t)
        | VmError::NoSuchWriter(t)
        | VmError::NoSuchWindow(t)
        | VmError::NoSuchView(t) => PortError::NotFound(t),
        VmError::Script(ref script) | VmError::Load { error: ref script, .. } => {
            tracing::warn!(%app, error = %script, traceback = script.traceback.as_deref().unwrap_or(""), "app code failed");
            PortError::Rejected(e.to_string())
        }
        // a bug in Rust code, not the app's: the caller is told, the app stays loaded, and the host's log says so
        VmError::Panicked(ref message) => {
            tracing::error!(%app, %message, "a host function panicked in a call into the app");
            PortError::Internal(e.to_string())
        }
        other => PortError::Rejected(other.to_string()), // ctx:fail, invalid input, bad data
    }
}

#[async_trait]
impl AppRuntime for LuaRuntime {
    async fn load(&self, app: &AppId, package: AppPackage, data: Arc<dyn AppData>) -> PortResult<LoadedApp> {
        let config = VmConfig {
            host_modules: Arc::new(self.host_modules.clone()),
            roster: self.roster.clone().unwrap_or_else(|| VmConfig::default().roster),
            files: self.files.files(app),
            refused_modules: Vec::new(),
            test_hooks: false,
        };
        // An app that serves a port cannot be replaced by a version that serves the same one while the old
        // still holds it: stop what the old version started (listeners, tasks, workers) before the new one
        // starts. Its tools and windows keep working until the new one has loaded, so a version that fails to
        // load leaves the old one answering, without its background work.
        let running = self.vms.read().get(app).cloned();
        if let Some(old) = running {
            // The old VM's background work stops before the new version is known to load, and a failed reload
            // leaves it answering without that work, with requests to any server it starts again running
            // read-only: stated in the `AppRuntime::load` port doc, accepted in AGOS-64.
            old.stop_background();
        }
        let vm = AppVm::load(app, package, data, config).await.map_err(|e| port_error(app, e))?;
        let decl = vm.decl().ok_or_else(|| {
            PortError::Rejected(format!("{app}: the app must declare itself with app.manifest {{ ... }}"))
        })?;
        let loaded = LoadedApp { decl: Some(decl), tools: vm.tools(), sources: vm.sources() };
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        // a call that comes from inside the VM (a request to the app's own server, a task) queues effects too:
        // forward them when it ends. The pump is one of the app's own tasks, so it goes with the app.
        let vm = Arc::new(vm);
        let (signal, weak) = (vm.effects_signal(), Arc::downgrade(&vm));
        let (notifier, shell, id) = (self.notifier.clone(), self.shell.clone(), app.clone());
        let activity = self.activity.clone();
        let life = vm.life().clone();
        life.clone().spawn(async move {
            loop {
                signal.notified().await;
                // what the VM has to say is taken first and the VM let go before anything is awaited: the pump
                // must not be what keeps an unloaded VM alive
                let Some(vm) = weak.upgrade() else { break };
                let effects = vm.take_effects();
                // open windows follow what the background work changed; a pause that changed no state or
                // window request has nothing to follow (data changes are pushed by the live sources)
                let follow = !life.is_closed() && vm.has_unsettled_views();
                drop(vm);
                if let Err(error) = deliver_effects(&*notifier, &*shell, &id, effects, &life).await {
                    tracing::warn!(app = %id, %error, "could not forward what the app asked for");
                }
                if !follow || life.is_closed() {
                    continue;
                }
                // The application is held for the settle only. A settle that hangs (the VM's gate is held by a
                // runaway call) holds it until the app is unloaded, which aborts this task: apps have no budgets.
                let listener = activity.get().and_then(Weak::upgrade);
                if let Some(listener) = listener {
                    listener.background_settled(&id).await;
                }
            }
        });
        // only a VM that loaded completely replaces a running one (hot reload keeps the old one on failure)
        let replaced = self.vms.write().insert(app.clone(), vm);
        // the VM a reload replaces stops like an unloaded one: what it started (tasks, servers, workers) is
        // aborted, so the new version does not share its port or its database with a ghost
        if let Some(old) = replaced {
            old.shutdown();
        }
        Ok(loaded)
    }

    async fn unload(&self, app: &AppId) -> PortResult<()> {
        let removed = self.vms.write().remove(app);
        if let Some(vm) = removed {
            // an in-flight call keeps its own Arc until it ends: stop everything the app started first (its
            // listeners are closed, its tasks and workers aborted), so nothing outlives the uninstall
            vm.shutdown();
            // last Arc => dropped => the VM is freed
            if let Ok(vm) = Arc::try_unwrap(vm) {
                vm.unload();
            }
        }
        Ok(())
    }

    async fn call_tool(
        &self,
        app: &AppId,
        tool: &str,
        args: Value,
        ctx: CallContext,
        read_only: bool,
    ) -> PortResult<Value> {
        let vm = self.vm(app)?;
        let out = vm.call_tool(tool, args, &ctx, read_only).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn write_row(
        &self,
        app: &AppId,
        source: &SourceId,
        key: Value,
        field: &str,
        value: Value,
        ctx: CallContext,
    ) -> PortResult<()> {
        let vm = self.vm(app)?;
        let out = vm.write_row(source, &key, field, &value, &ctx).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn windows(&self, app: &AppId) -> PortResult<Vec<String>> {
        Ok(self.vm(app)?.windows())
    }

    async fn open_view(
        &self,
        app: &AppId,
        view: &str,
        window: &str,
        params: Map<String, Value>,
        restored: Map<String, Value>,
    ) -> PortResult<RenderOutput> {
        let vm = self.vm(app)?;
        let out = vm.open_view(view, window, params, restored).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn render_view(&self, app: &AppId, view: &str) -> PortResult<RenderOutput> {
        let vm = self.vm(app)?;
        let out = vm.render_view(view).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn dispatch_view_event(
        &self,
        app: &AppId,
        view: &str,
        call: ViewEventCall,
        ctx: CallContext,
    ) -> PortResult<bool> {
        let vm = self.vm(app)?;
        let out = vm.dispatch_view_event(view, call, &ctx).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn set_client_values(&self, app: &AppId, view: &str, values: Map<String, Value>) -> PortResult<()> {
        self.vm(app)?.set_client_values(view, &values).await.map_err(|e| port_error(app, e))
    }

    async fn close_view(&self, app: &AppId, view: &str) -> PortResult<()> {
        let vm = self.vm(app)?;
        let out = vm.close_view(view).await.map_err(|e| port_error(app, e));
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn dirty_views(&self, app: &AppId) -> PortResult<Vec<String>> {
        Ok(self.vm(app)?.dirty_views())
    }

    async fn take_view_effects(&self, app: &AppId) -> PortResult<Vec<(String, ViewEffect)>> {
        Ok(self.vm(app)?.take_view_effects())
    }
}
