//! `AppRuntime` port implementation: a map of app id -> sandboxed VM.
//!
//! An app that fails (it does not load, or a call raises) fails alone: its error is returned
//! to that caller with the app's file and line, and no other VM, and not the host, is touched.

use crate::vm::{AppVm, Effect, VmConfig, VmError};
use async_trait::async_trait;
use neoc_vm::lifecycle::Life;
use parking_lot::RwLock;
use serde_json::{Map, Value};
use std::future::Future;
use std::{collections::HashMap, sync::Arc};
use vos_application::{
    panics,
    ports::{
        AgentRoster, AppData, AppPackage, AppRuntime, CallContext, LoadedApp, Notifier, PortResult, Shell,
    },
    FileStore, NoFiles, PortError,
};
use vos_domain::AppId;

pub struct LuaRuntime {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    shell: Arc<dyn Shell>,
    roster: Option<Arc<dyn AgentRoster>>,
    /// Where each app's `app.files` live (none unless the host gives a store).
    files: Arc<dyn FileStore>,
}

impl LuaRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, shell: Arc<dyn Shell>) -> Self {
        Self { vms: RwLock::default(), notifier, shell, roster: None, files: Arc::new(NoFiles) }
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

    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.vms.read().get(app).cloned().ok_or_else(|| PortError::NotFound(app.to_string()))
    }
}

/// Run a call the host made into the app, and end it with an error when the app is unloaded or replaced while it runs. Stopping
/// an app aborts what it started (tasks, servers, workers); a call is not one of those, it is whoever asked, so without this a
/// handler waiting at `task.wait` (a long poll, a LAN service that is down) outlives its uninstall and the client that made the
/// call, and the host connection it came in on, keep waiting and keep the request's permit. Dropping the call is what a caller
/// that gives up does everywhere else (`AppVm::in_call`): the turn is handed on, a transaction it opened is rolled back.
async fn until_unloaded<T>(
    app: &AppId,
    vm: &AppVm,
    call: impl Future<Output = PortResult<T>>,
) -> PortResult<T> {
    tokio::select! {
        out = call => out,
        () = vm.life().closed() => {
            Err(PortError::Rejected(format!("`{app}` was unloaded while the call was running")))
        }
    }
}

/// Forward what the app asked for during a call: notifications, events and its window go to their ports.
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
        // one effect whose port panics costs only that effect: the rest of the batch is still delivered (see
        // `vos_application::panics`)
        let delivered = panics::caught(async {
            match effect {
                Effect::Notify(n) => notifier.notify(n).await?,
                Effect::Emit { name, data } => shell.emit_event(app, &name, data).await,
                Effect::OpenWindow { name, params } => {
                    let params = match params {
                        Value::Object(map) => map,
                        _ => Map::new(),
                    };
                    shell.open_window(app, &name, params).await;
                }
            }
            Ok::<(), PortError>(())
        })
        .await;
        match delivered {
            Ok(result) => result?,
            Err(message) => {
                tracing::error!(%app, %message, "delivering what the app asked for panicked; the rest is still delivered");
            }
        }
    }
    Ok(())
}

fn port_error(app: &AppId, e: VmError) -> PortError {
    match e {
        VmError::NoSuchTool(t) => PortError::NotFound(t),
        VmError::Script(ref script) | VmError::Load { error: ref script, .. } => {
            tracing::warn!(%app, error = %script, traceback = script.traceback.as_deref().unwrap_or(""), "app code failed");
            PortError::Rejected(e.to_string())
        }
        VmError::Rpc { code, message, data } => PortError::Rpc { code, message, data },
        // JSON-RPC's "method not found": the page called something the backend does not implement
        VmError::NoSuchHandler(name) => {
            PortError::Rpc { code: -32601, message: format!("Method not found: {name}"), data: None }
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
            roster: self.roster.clone().unwrap_or_else(|| VmConfig::default().roster),
            files: self.files.files(app),
            refused_modules: Vec::new(),
            test_hooks: false,
        };
        // An app that serves a port cannot be replaced by a version that serves the same one while the old
        // still holds it: stop what the old version started (listeners, tasks, workers) before the new one
        // starts. Its tools and handlers keep working until the new one has loaded, so a version that fails to
        // load leaves the old one answering, without its background work.
        let running = self.vms.read().get(app).cloned();
        if let Some(old) = running {
            // The old VM's background work stops before the new version is known to load, and a failed reload
            // leaves it answering without that work, with requests to any server it starts again running
            // read-only: stated in the `AppRuntime::load` port doc, accepted in AGOS-64.
            old.stop_background();
        }
        let vm = AppVm::load(app, package, data, config).await.map_err(|e| port_error(app, e))?;
        let loaded = LoadedApp { tools: vm.tools() };
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        // a call that comes from inside the VM (a request to the app's own server, a task) queues effects too:
        // forward them when it ends or pauses. The pump is one of the app's own tasks, so it goes with the app.
        let vm = Arc::new(vm);
        let (signal, weak) = (vm.effects_signal(), Arc::downgrade(&vm));
        let (notifier, shell, id) = (self.notifier.clone(), self.shell.clone(), app.clone());
        let life = vm.life().clone();
        life.clone().spawn(async move {
            loop {
                signal.notified().await;
                // what the VM has to say is taken first and the VM let go before anything is awaited: the pump
                // must not be what keeps an unloaded VM alive
                let Some(vm) = weak.upgrade() else { break };
                let effects = vm.take_effects();
                drop(vm);
                // a panic in a port while forwarding is logged and the pump waits for the next signal: the app's
                // later notifications and events still go out (see `vos_application::panics`)
                let forwarded = panics::caught(async {
                    if let Err(error) = deliver_effects(&*notifier, &*shell, &id, effects, &life).await {
                        tracing::warn!(app = %id, %error, "could not forward what the app asked for");
                    }
                })
                .await;
                if let Err(message) = forwarded {
                    tracing::error!(app = %id, %message, "forwarding what the app's background work asked for panicked");
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
        let out = until_unloaded(app, &vm, async {
            vm.call_tool(tool, args, &ctx, read_only).await.map_err(|e| port_error(app, e))
        })
        .await;
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }

    async fn call_handler(
        &self,
        app: &AppId,
        method: &str,
        args: Value,
        ctx: CallContext,
    ) -> PortResult<Value> {
        let vm = self.vm(app)?;
        let out = until_unloaded(app, &vm, async {
            vm.call_handler(method, args, &ctx).await.map_err(|e| port_error(app, e))
        })
        .await;
        forward_effects(&*self.notifier, &*self.shell, app, &vm).await?;
        out
    }
}
