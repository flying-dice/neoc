//! `AppRuntime` port implementation: a map of app id -> sandboxed VM.
//!
//! An app that fails (it does not load, or a call raises) fails alone: its error is returned
//! to that caller with the app's file and line, and no other VM, and not the host, is touched.
//!
//! The apps also reach each other here ([`Hub`]): `RPC.send_request` runs a handler in another app's VM, and an app's
//! public notification (`RPC.send_notification` of a name without a leading `_`) is delivered to every other app that
//! listens to it (`RPC.on_notification`), after its open pages are told.

use crate::vm::{AppPeers, AppVm, Effect, VmConfig, VmError};
use async_trait::async_trait;
use neoc_vm::lifecycle::BoxFuture;
use neoc_vm::lifecycle::Life;
use parking_lot::RwLock;
use serde_json::{Map, Value};
use std::future::Future;
use std::{collections::HashMap, sync::Arc};
use vos_application::{
    panics,
    ports::{
        AgentRoster, AppBus, AppData, AppPackage, AppRuntime, AppTools, CallContext, Notifier, PortResult,
        Shell, ToolLeases,
    },
    FileStore, NoFiles, PortError,
};
use vos_domain::{Actor, AppId, QualifiedToolName, ToolSpec};

pub struct LuaRuntime {
    hub: Arc<Hub>,
    roster: Option<Arc<dyn AgentRoster>>,
    /// Where each app's `app.files` live (none unless the host gives a store).
    files: Arc<dyn FileStore>,
    /// Every running app's tools, for `app.tools.all` and `app.tools.call` (none until the host binds them).
    tools: Arc<LateTools>,
    /// The MCP tokens apps lease for agents of their own (`app.tools.lease`): an app's go when its backend stops.
    leases: Arc<dyn ToolLeases>,
}

/// The tools port, bound after the runtime is built: the use case that implements it is built on the runtime. Held
/// weakly, so the two do not keep each other alive; until it is bound (and in a runtime nothing binds) there are no
/// tools.
#[derive(Default)]
struct LateTools(std::sync::OnceLock<std::sync::Weak<dyn AppTools>>);

impl LateTools {
    fn bound(&self) -> Option<Arc<dyn AppTools>> {
        self.0.get().and_then(std::sync::Weak::upgrade)
    }
}

#[async_trait]
impl AppTools for LateTools {
    fn all(&self) -> Vec<(QualifiedToolName, ToolSpec)> {
        self.bound().map_or_else(Vec::new, |tools| tools.all())
    }

    async fn call(&self, caller: Actor, name: &QualifiedToolName, args: Value) -> PortResult<Value> {
        match self.bound() {
            Some(tools) => tools.call(caller, name, args).await,
            None => Err(PortError::NotFound(format!("tool `{name}`"))),
        }
    }
}

/// The RPC bus, bound after the runtime is built (the use case that implements it is built on the runtime), weakly. A
/// runtime nothing binds (an adapter's tests) is its own bus: requests reach the backends only, notifications the
/// shell and the backends.
#[derive(Default)]
struct LateBus(std::sync::OnceLock<std::sync::Weak<dyn AppBus>>);

impl LateBus {
    fn bound(&self) -> Option<Arc<dyn AppBus>> {
        self.0.get().and_then(std::sync::Weak::upgrade)
    }
}

/// The running apps, and where what they ask for goes: shared by the runtime and, weakly, by every app's VM (its
/// `RPC.send_request`), so the apps' backends reach the bus through it.
struct Hub {
    vms: RwLock<HashMap<AppId, Arc<AppVm>>>,
    notifier: Arc<dyn Notifier>,
    shell: Arc<dyn Shell>,
    bus: LateBus,
}

/// The time a call is made, for `ctx.now`.
fn now_ms() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

impl Hub {
    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.vms
            .read()
            .get(app)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("app `{app}` is not running")))
    }

    /// Forward what the app asked for during a call: notifications and its window go to their ports.
    async fn forward(&self, app: &AppId, vm: &AppVm) -> PortResult<()> {
        self.deliver(app, vm.take_effects(), vm.life()).await
    }

    /// Deliver effects already taken from the VM, so a caller need not keep the VM alive while the ports work. Boxed:
    /// a notification delivered to another app's listener can make that app ask for more.
    fn deliver<'a>(
        &'a self,
        app: &'a AppId,
        effects: Vec<Effect>,
        life: &'a Life,
    ) -> BoxFuture<'a, PortResult<()>> {
        Box::pin(async move {
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
                        Effect::Notify(n) => self.notifier.notify(n).await?,
                        Effect::Publish { name, params } => self.publish(app, &name, params).await,
                        Effect::HandlersChanged => {
                            if let Some(bus) = self.bus.bound() {
                                bus.handlers_changed(app).await;
                            }
                        }
                        Effect::OpenWindow { name, params } => {
                            let params = match params {
                                Value::Object(map) => map,
                                _ => Map::new(),
                            };
                            self.shell.open_window(app, &name, params).await;
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
        })
    }

    /// `RPC.send_notification`: onto the bus. A bus that refuses it (the app is still loading, say)
    /// leaves it to the runtime, which delivers it the same way.
    async fn publish(&self, from: &AppId, name: &str, params: Value) {
        if let Some(bus) = self.bus.bound() {
            match bus.publish(from, name, params.clone()).await {
                Ok(()) => return,
                // the contract refused it: it goes nowhere
                Err(error @ PortError::Rpc { .. }) => {
                    tracing::warn!(app = %from, notification = name, %error, "the app's contract refused a notification");
                    return;
                }
                // the bus could not take it (the app is still loading): the runtime delivers it the same way
                Err(error) => {
                    tracing::debug!(app = %from, notification = name, %error, "the bus refused a notification");
                }
            }
        }
        self.deliver_notification(from, name, params).await;
    }

    /// An app's notification, from its page or its backend: to the open pages (the shell hands it to the app's frames
    /// and, unless it is a `_name`, to the frames that subscribed to the app), then to every backend that listens
    /// (`RPC.on_notification`), one app after the other in id order, each told it came from `from`. A `_name` reaches
    /// only `from`'s own listeners. A listener that fails is logged; it costs the others nothing.
    async fn deliver_notification(&self, from: &AppId, name: &str, params: Value) {
        self.shell.emit_event(from, name, params.clone()).await;
        let private = name.starts_with('_');
        let mut listening: Vec<(AppId, Arc<AppVm>)> = self
            .vms
            .read()
            .iter()
            .filter(|(id, vm)| (!private || *id == from) && vm.listens(from, name))
            .map(|(id, vm)| (id.clone(), vm.clone()))
            .collect();
        listening.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, vm) in listening {
            let ctx = CallContext { actor: Actor::app(from.as_str()), now_ms: now_ms() };
            let heard = until_unloaded(&id, &vm, async {
                vm.deliver(from, name, params.clone(), &ctx).await.map_err(|e| port_error(&id, e))
            })
            .await;
            if let Err(error) = heard {
                tracing::warn!(app = %id, %from, notification = name, %error, "a listener of a notification failed");
            }
            if let Err(error) = self.forward(&id, &vm).await {
                tracing::warn!(app = %id, %error, "could not forward what a listener asked for");
            }
        }
    }

    /// `RPC.send_request`: onto the bus, as `from`. With no bus bound, the handler of `to`'s backend answers, if it has one.
    async fn request(&self, from: &AppId, to: &AppId, method: &str, params: Value) -> PortResult<Value> {
        if let Some(bus) = self.bus.bound() {
            return bus.request(Actor::app(from.as_str()), to, method, params).await;
        }
        let vm = self.vm(to)?;
        let ctx = CallContext { actor: Actor::app(from.as_str()), now_ms: now_ms() };
        let out = until_unloaded(to, &vm, async {
            vm.call_handler(method, params, &ctx).await.map_err(|e| port_error(to, e))
        })
        .await;
        self.forward(to, &vm).await?;
        out
    }
}

/// What an app's VM holds of the hub: weakly, so the VMs the hub holds do not keep it alive.
struct Peers(std::sync::Weak<Hub>);

#[async_trait]
impl AppPeers for Peers {
    async fn request(&self, from: &AppId, to: &AppId, method: &str, params: Value) -> PortResult<Value> {
        match self.0.upgrade() {
            Some(hub) => hub.request(from, to, method, params).await,
            None => Err(PortError::NotFound(format!("app `{to}` is not running"))),
        }
    }
}

impl LuaRuntime {
    pub fn new(notifier: Arc<dyn Notifier>, shell: Arc<dyn Shell>) -> Self {
        Self {
            hub: Arc::new(Hub { vms: RwLock::default(), notifier, shell, bus: LateBus::default() }),
            roster: None,
            files: Arc::new(NoFiles),
            tools: Arc::default(),
            leases: VmConfig::default().leases,
        }
    }

    /// Where every app leases MCP tokens for agents of its own (`app.tools.lease`, `app.tools.release`).
    #[must_use]
    pub fn with_leases(mut self, leases: Arc<dyn ToolLeases>) -> Self {
        self.leases = leases;
        self
    }

    /// The agent contacts `app.agents:list()` shows to every app.
    #[must_use]
    pub fn with_roster(mut self, roster: Arc<dyn AgentRoster>) -> Self {
        self.roster = Some(roster);
        self
    }

    /// Put every app's backend on the RPC bus (`RPC.send_request`, `RPC.send_notification`). Once: a second call is
    /// ignored.
    pub fn bind_bus(&self, bus: std::sync::Weak<dyn AppBus>) {
        let _ = self.hub.bus.0.set(bus);
    }

    /// Give every app the tools of every running app (`app.tools.all`, `app.tools.call`). Once: a second call is
    /// ignored.
    pub fn bind_tools(&self, tools: std::sync::Weak<dyn AppTools>) {
        let _ = self.tools.0.set(tools);
    }

    /// Where every app's managed files (`app.files`) live: the same store the application purges them from.
    #[must_use]
    pub fn with_files(mut self, files: Arc<dyn FileStore>) -> Self {
        self.files = files;
        self
    }

    fn vm(&self, app: &AppId) -> PortResult<Arc<AppVm>> {
        self.hub.vms.read().get(app).cloned().ok_or_else(|| PortError::NotFound(app.to_string()))
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

fn port_error(app: &AppId, e: VmError) -> PortError {
    match e {
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
    async fn load(&self, app: &AppId, package: AppPackage, data: Arc<dyn AppData>) -> PortResult<()> {
        let config = VmConfig {
            roster: self.roster.clone().unwrap_or_else(|| VmConfig::default().roster),
            files: self.files.files(app),
            host_tools: self.tools.clone(),
            leases: self.leases.clone(),
            peers: Arc::new(Peers(Arc::downgrade(&self.hub))),
            refused_modules: Vec::new(),
            test_hooks: false,
        };
        // An app that serves a port cannot be replaced by a version that serves the same one while the old
        // still holds it: stop what the old version started (listeners, tasks, workers) before the new one
        // starts. Its handlers keep working until the new one has loaded, so a version that fails to
        // load leaves the old one answering, without its background work.
        let running = self.hub.vms.read().get(app).cloned();
        if let Some(old) = running {
            // The old VM's background work stops before the new version is known to load, and a failed reload
            // leaves it answering without that work, with requests to any server it starts again running
            // read-only: stated in the `AppRuntime::load` port doc, accepted in AGOS-64.
            old.stop_background();
            // the tokens it leased go with it: its connections to the agents it gave them to were background work
            self.leases.release_all(app).await;
        }
        let vm = AppVm::load(app, package, data, config).await.map_err(|e| port_error(app, e))?;
        self.hub.forward(app, &vm).await?;
        // a call that comes from inside the VM (a request to the app's own server, a task) queues effects too:
        // forward them when it ends or pauses. The pump is one of the app's own tasks, so it goes with the app.
        let vm = Arc::new(vm);
        let (signal, weak) = (vm.effects_signal(), Arc::downgrade(&vm));
        let (hub, id) = (Arc::downgrade(&self.hub), app.clone());
        let life = vm.life().clone();
        life.clone().spawn(async move {
            loop {
                signal.notified().await;
                // what the VM has to say is taken first and the VM let go before anything is awaited: the pump
                // must not be what keeps an unloaded VM alive
                let Some(vm) = weak.upgrade() else { break };
                let effects = vm.take_effects();
                drop(vm);
                let Some(hub) = hub.upgrade() else { break };
                // a panic in a port while forwarding is logged and the pump waits for the next signal: the app's
                // later notifications still go out (see `vos_application::panics`)
                let forwarded = panics::caught(async {
                    if let Err(error) = hub.deliver(&id, effects, &life).await {
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
        let replaced = self.hub.vms.write().insert(app.clone(), vm);
        // the VM a reload replaces stops like an unloaded one: what it started (tasks, servers, workers) is
        // aborted, so the new version does not share its port or its database with a ghost
        if let Some(old) = replaced {
            old.shutdown();
        }
        Ok(())
    }

    async fn unload(&self, app: &AppId) -> PortResult<()> {
        let removed = self.hub.vms.write().remove(app);
        if let Some(vm) = removed {
            // an in-flight call keeps its own Arc until it ends: stop everything the app started first (its
            // listeners are closed, its tasks and workers aborted), so nothing outlives the uninstall
            vm.shutdown();
            // and the tokens it leased for its agents stop working
            self.leases.release_all(app).await;
            // last Arc => dropped => the VM is freed
            if let Ok(vm) = Arc::try_unwrap(vm) {
                vm.unload();
            }
        }
        Ok(())
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
        self.hub.forward(app, &vm).await?;
        out
    }

    async fn call_handler_read_only(
        &self,
        app: &AppId,
        method: &str,
        args: Value,
        ctx: CallContext,
    ) -> PortResult<Value> {
        let vm = self.vm(app)?;
        let out = until_unloaded(app, &vm, async {
            vm.call_handler_with(method, args, &ctx, true).await.map_err(|e| port_error(app, e))
        })
        .await;
        self.hub.forward(app, &vm).await?;
        out
    }

    fn has_handler(&self, app: &AppId, method: &str) -> bool {
        self.vm(app).is_ok_and(|vm| vm.has_handler(method))
    }

    fn handlers(&self, app: &AppId) -> Vec<String> {
        self.vm(app).map_or_else(|_| Vec::new(), |vm| vm.handlers())
    }

    async fn deliver_notification(&self, from: &AppId, name: &str, params: Value) -> PortResult<()> {
        self.hub.deliver_notification(from, name, params).await;
        Ok(())
    }
}
