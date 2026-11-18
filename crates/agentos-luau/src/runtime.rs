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
    ports::{
        AgentRoster, AppData, AppPackage, AppRuntime, CallContext, LoadedApp, Notifier, PortResult,
        RenderOutput, Shell, ViewEffect, ViewEventCall,
    },
    PortError,
};
use agentos_domain::{AppId, Capabilities};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::{Map, Value};
use std::{collections::HashMap, sync::Arc};

/// The generated constructors every app gets as `require("@agentos/ui")`.
const UI_MODULE: &str = include_str!("../../../types/generated/ui.luau");

pub struct LuauRuntime {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    shell: Arc<dyn Shell>,
    limits: Limits,
    host_modules: HashMap<String, Vec<u8>>,
    roster: Option<Arc<dyn AgentRoster>>,
}

impl LuauRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, shell: Arc<dyn Shell>, limits: Limits) -> Self {
        let host_modules = HashMap::from([("ui".to_owned(), UI_MODULE.as_bytes().to_vec())]);
        Self { vms: RwLock::default(), notifier, shell, limits, host_modules, roster: None }
    }

    /// The agent contacts apps with the `agents` permission can list.
    #[must_use]
    pub fn with_roster(mut self, roster: Arc<dyn AgentRoster>) -> Self {
        self.roster = Some(roster);
        self
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
                    let params = match params {
                        Value::Object(map) => map,
                        _ => Map::new(),
                    };
                    self.shell.open_window(app, &name, params).await;
                }
            }
        }
        Ok(())
    }
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
            roster: self.roster.clone().unwrap_or_else(|| VmConfig::default().roster),
        };
        let vm = AppVm::load(app, package, data, config).await.map_err(|e| port_error(app, e))?;
        let decl = vm.decl().ok_or_else(|| {
            PortError::Rejected(format!("{app}: the app must declare itself with app.manifest {{ ... }}"))
        })?;
        let loaded = LoadedApp { decl: Some(decl), tools: vm.tools(), sources: vm.sources() };
        self.flush_effects(app, &vm).await?;
        // only a VM that loaded completely replaces a running one (hot reload keeps the old one on failure)
        let replaced = self.vms.write().insert(app.clone(), Arc::new(vm));
        // the VM a reload replaces stops like an unloaded one: a call still running in it must not keep
        // writing to the database the new version now owns
        if let Some(old) = replaced {
            old.retire();
        }
        Ok(loaded)
    }

    async fn unload(&self, app: &AppId) -> PortResult<()> {
        let removed = self.vms.write().remove(app);
        if let Some(vm) = removed {
            // an in-flight call keeps its own Arc until it ends, and a runaway one never would: retire the
            // app first, so the call stops at its next interrupt or host call and the VM is freed then
            vm.retire();
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
        self.flush_effects(app, &vm).await?;
        out
    }

    async fn render_view(&self, app: &AppId, view: &str) -> PortResult<RenderOutput> {
        let vm = self.vm(app)?;
        let out = vm.render_view(view).await.map_err(|e| port_error(app, e));
        self.flush_effects(app, &vm).await?;
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
        self.flush_effects(app, &vm).await?;
        out
    }

    async fn set_client_values(&self, app: &AppId, view: &str, values: Map<String, Value>) -> PortResult<()> {
        self.vm(app)?.set_client_values(view, &values).map_err(|e| port_error(app, e))
    }

    async fn close_view(&self, app: &AppId, view: &str) -> PortResult<()> {
        let vm = self.vm(app)?;
        let out = vm.close_view(view).await.map_err(|e| port_error(app, e));
        self.flush_effects(app, &vm).await?;
        out
    }

    async fn dirty_views(&self, app: &AppId) -> PortResult<Vec<String>> {
        Ok(self.vm(app)?.dirty_views())
    }

    async fn take_view_effects(&self, app: &AppId) -> PortResult<Vec<(String, ViewEffect)>> {
        Ok(self.vm(app)?.take_view_effects())
    }
}
