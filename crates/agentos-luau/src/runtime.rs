//! `AppRuntime` port implementation: a map of app id -> sandboxed VM.

use crate::{AppVm, Limits, VmError};
use agentos_application::{
    ports::{AppData, AppRuntime, LoadedApp, Notifier, PortResult},
    PortError,
};
use agentos_domain::{
    ui::{UiEvent, UiNode},
    AppId, Notification, ToolSpec,
};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub struct LuauRuntime {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    limits: Limits,
}

impl LuauRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, limits: Limits) -> Self {
        Self { vms: RwLock::default(), notifier, limits }
    }

    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.vms.read().get(app).cloned().ok_or_else(|| PortError::NotFound(app.to_string()))
    }

    /// `app.notify{}` calls made inside a VM are drained after every call and forwarded to the port.
    async fn flush_notifications(&self, app: &AppId, vm: &AppVm) -> PortResult<()> {
        for n in vm.take_notifications() {
            let title = n["title"].as_str().unwrap_or_default().to_owned();
            let body = n["body"].as_str().map(str::to_owned);
            self.notifier.notify(Notification { app: app.clone(), title, body }).await?;
        }
        Ok(())
    }
}

impl From<VmError> for PortError {
    fn from(e: VmError) -> Self {
        match e {
            VmError::NoSuchTool(t) => PortError::NotFound(t),
            other => PortError::Rejected(other.to_string()), // Lua errors, budget kills, bad data
        }
    }
}

#[async_trait]
impl AppRuntime for LuauRuntime {
    async fn load(&self, app: &AppId, main: Vec<u8>, data: Arc<dyn AppData>) -> PortResult<LoadedApp> {
        let vm = AppVm::load(app.as_str(), &main, data, self.limits.clone()).await?;
        let tools = vm
            .tools()
            .into_iter()
            .map(|t| ToolSpec { name: t.name, description: t.description, input_schema: t.input_schema })
            .collect();
        self.flush_notifications(app, &vm).await?;
        self.vms.write().insert(app.clone(), Arc::new(vm));
        Ok(LoadedApp { tools })
    }

    async fn unload(&self, app: &AppId) -> PortResult<()> {
        if let Some(vm) = self.vms.write().remove(app) {
            // last Arc => unload frees the VM; in-flight calls hold their own Arc until they finish
            if let Ok(vm) = Arc::try_unwrap(vm) {
                vm.unload();
            }
        }
        Ok(())
    }

    async fn call_tool(&self, app: &AppId, tool: &str, args: Value) -> PortResult<Value> {
        let vm = self.vm(app)?;
        let out = vm.call_tool(tool, args).await?;
        self.flush_notifications(app, &vm).await?;
        Ok(out)
    }

    async fn dispatch_event(&self, app: &AppId, event: UiEvent) -> PortResult<Option<UiNode>> {
        let vm = self.vm(app)?;
        let payload = serde_json::to_value(&event).map_err(|e| PortError::Internal(e.to_string()))?;
        let out = vm.emit("ui:event", payload).await?;
        self.flush_notifications(app, &vm).await?;
        if out.is_null() {
            return Ok(None);
        }
        serde_json::from_value(out)
            .map(Some)
            .map_err(|e| PortError::Rejected(format!("app returned an invalid view: {e}")))
    }
}
