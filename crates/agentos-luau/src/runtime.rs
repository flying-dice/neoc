//! `AppRuntime` port implementation: a map of app id -> sandboxed VM.
//!
//! An app that fails (it does not load, or a call blows its budget) fails alone: its error is returned
//! to that caller with the app's file and line, and no other VM, and not the host, is touched.

use crate::{
    vm::{AppVm, Effect, ShellEffect, VmConfig, VmError},
    Limits,
};
use agentos_application::{
    live::SourceId,
    ports::{AppData, AppPackage, AppRuntime, CallContext, LoadedApp, Notifier, PortResult, Shell},
    PortError,
};
use agentos_domain::{
    ui::{UiEvent, UiNode},
    AppId, Capabilities,
};
use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub struct LuauRuntime {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    shell: Arc<dyn Shell>,
    limits: Limits,
    host_modules: HashMap<String, Vec<u8>>,
    /// `app:openWindow(...)` requests, until the view layer takes them.
    open_requests: Mutex<Vec<(AppId, String, Value)>>,
}

impl LuauRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, shell: Arc<dyn Shell>, limits: Limits) -> Self {
        Self {
            vms: RwLock::default(),
            notifier,
            shell,
            limits,
            host_modules: HashMap::new(),
            open_requests: Mutex::default(),
        }
    }

    /// Make `require("@agentos/<name>")` load this Luau source in every app's VM (the generated `ui` module).
    #[must_use]
    pub fn with_host_module(mut self, name: &str, source: impl Into<Vec<u8>>) -> Self {
        self.host_modules.insert(name.to_owned(), source.into());
        self
    }

    /// The `app:openWindow(name, params)` requests made since the last call.
    pub fn take_open_requests(&self) -> Vec<(AppId, String, Value)> {
        std::mem::take(&mut *self.open_requests.lock())
    }

    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.vms.read().get(app).cloned().ok_or_else(|| PortError::NotFound(app.to_string()))
    }

    /// Forward what the app asked for during a call: notifications and shell declarations go to their ports.
    async fn flush_effects(&self, app: &AppId, vm: &AppVm) -> PortResult<()> {
        for effect in vm.take_effects() {
            match effect {
                Effect::Notify(n) => self.notifier.notify(n).await?,
                Effect::Shell(ShellEffect::Badge(b)) => self.shell.set_badge(app, b).await,
                Effect::Shell(ShellEffect::Commands(c)) => self.shell.set_menu_commands(app, c).await,
                Effect::Shell(ShellEffect::DeepLinks(l)) => self.shell.set_deep_links(app, l).await,
                Effect::OpenWindow { name, params } => {
                    self.open_requests.lock().push((app.clone(), name, params));
                }
            }
        }
        Ok(())
    }
}

fn port_error(app: &AppId, e: VmError) -> PortError {
    match e {
        VmError::NoSuchTool(t) => PortError::NotFound(t),
        VmError::NoSuchWriter(s) => PortError::NotFound(s),
        VmError::Script(ref script) | VmError::Load { error: ref script, .. } => {
            tracing::warn!(%app, error = %script, traceback = script.traceback.as_deref().unwrap_or(""), "app code failed");
            PortError::Rejected(e.to_string())
        }
        other => PortError::Rejected(other.to_string()), // ctx:fail, invalid input, bad data
    }
}

#[async_trait]
impl AppRuntime for LuauRuntime {
    async fn load(
        &self,
        app: &AppId,
        package: AppPackage,
        data: Arc<dyn AppData>,
        capabilities: Capabilities,
    ) -> PortResult<LoadedApp> {
        let config = VmConfig {
            capabilities,
            limits: self.limits.clone(),
            host_modules: Arc::new(self.host_modules.clone()),
        };
        let vm = AppVm::load(app, package, data, config).await.map_err(|e| port_error(app, e))?;
        let decl = vm.decl().ok_or_else(|| {
            PortError::Rejected(format!("{app}: the app must declare itself with app.manifest {{ ... }}"))
        })?;
        let loaded = LoadedApp { decl: Some(decl), tools: vm.tools(), sources: vm.sources() };
        self.flush_effects(app, &vm).await?;
        // only a VM that loaded completely replaces a running one (hot reload keeps the old one on failure)
        self.vms.write().insert(app.clone(), Arc::new(vm));
        Ok(loaded)
    }

    async fn unload(&self, app: &AppId) -> PortResult<()> {
        if let Some(vm) = self.vms.write().remove(app) {
            // last Arc => dropped => the VM is freed; an in-flight call keeps its own Arc until it finishes
            if let Ok(vm) = Arc::try_unwrap(vm) {
                vm.unload();
            }
        }
        Ok(())
    }

    async fn call_tool(&self, app: &AppId, tool: &str, args: Value, ctx: CallContext) -> PortResult<Value> {
        let vm = self.vm(app)?;
        let out = vm.call_tool(tool, args, &ctx).await.map_err(|e| port_error(app, e));
        self.flush_effects(app, &vm).await?;
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
        self.flush_effects(app, &vm).await?;
        out
    }

    async fn dispatch_event(&self, app: &AppId, event: UiEvent) -> PortResult<Option<UiNode>> {
        let vm = self.vm(app)?;
        let payload = serde_json::to_value(&event).map_err(|e| PortError::Internal(e.to_string()))?;
        let out = vm.emit("ui:event", payload).await.map_err(|e| port_error(app, e))?;
        self.flush_effects(app, &vm).await?;
        if out.is_null() {
            return Ok(None);
        }
        serde_json::from_value(out)
            .map(Some)
            .map_err(|e| PortError::Rejected(format!("app returned an invalid view: {e}")))
    }
}
