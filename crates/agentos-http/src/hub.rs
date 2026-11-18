//! The `/rpc` application protocol: maps JSON-RPC methods onto `AppService` use cases (driving side)
//! and fans `ServerEvent`s / notifications out to every connected client (driven side: it is the
//! `EventSink` and `Notifier` adapter).

use crate::rpc::{RpcHandler, RpcPeer};
use agentos_application::{
    ports::{EventSink, Notifier, PortResult, ServerEvent},
    AppError, AppService, AppStatus, DeviceService, PortError,
};
use agentos_domain::{
    protocol::{DispatchUiEvent, RpcError, UiTree},
    AppId, DeviceId, Notification,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

/// A client that revokes its own device gets its answer before the socket is closed.
const SELF_REVOKE_GRACE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct RpcHub {
    app: OnceLock<Arc<AppService>>, // set once after construction (hub <-> service reference cycle)
    devices: OnceLock<Arc<DeviceService>>,
    peers: Mutex<Vec<RpcPeer>>,
}

impl RpcHub {
    pub fn bind(&self, app: Arc<AppService>, devices: Arc<DeviceService>) {
        let _ = self.app.set(app);
        let _ = self.devices.set(devices);
    }

    /// Hang up every socket of a device (it was revoked); the caller's own socket gets a short grace
    /// period so the revoke reply still arrives.
    fn disconnect_device(&self, device: &DeviceId, caller: &RpcPeer) {
        let doomed: Vec<RpcPeer> = self
            .peers
            .lock()
            .iter()
            .filter(|p| p.device().is_some_and(|d| d.id == *device))
            .cloned()
            .collect();
        for peer in doomed {
            if peer.is_same_connection(caller) {
                tokio::spawn(async move {
                    tokio::time::sleep(SELF_REVOKE_GRACE).await;
                    peer.close();
                });
            } else {
                peer.close();
            }
        }
    }

    async fn host_request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(devices) = self.devices.get() else { return Err(not_ready()) };
        match method {
            "host.devices.list" => {
                let current = peer.device().map(|d| d.id.clone());
                let devices = devices.list().await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                let rows: Vec<Value> = devices
                    .iter()
                    .map(|d| {
                        let mut row = serde_json::to_value(d).unwrap_or(Value::Null);
                        row["current"] = json!(current.as_ref() == Some(&d.id));
                        row
                    })
                    .collect();
                Ok(json!({ "devices": rows }))
            }
            "host.devices.revoke" => {
                let p: RevokeParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                devices.revoke(&p.id).await.map_err(|e| rpc_err(&AppError::Port(e)))?;
                self.disconnect_device(&p.id, peer);
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }

    fn broadcast(&self, method: &str, params: &Value) {
        // drop peers whose socket is gone
        self.peers.lock().retain(|p| p.notify(method, params.clone()).is_ok());
    }
}

fn rpc_err(e: &AppError) -> RpcError {
    let code = match e {
        AppError::Domain(_) => -32001,
        AppError::Port(PortError::NotFound(_)) | AppError::NotInstalled(_) => -32002,
        AppError::Port(_) => -32003,
        AppError::AlreadyInstalled(_) => -32004,
        AppError::NotRunning { .. } => -32005,
    };
    RpcError { code, message: e.to_string(), data: None }
}

fn not_ready() -> RpcError {
    RpcError { code: -32603, message: "host not ready".into(), data: None }
}

fn method_not_found(method: &str) -> RpcError {
    RpcError { code: -32601, message: format!("method `{method}` not found"), data: None }
}

fn bad_params(e: &serde_json::Error) -> RpcError {
    RpcError { code: -32602, message: format!("invalid params: {e}"), data: None }
}

#[derive(Deserialize)]
struct InstallParams {
    url: String,
}
#[derive(Deserialize)]
struct RevokeParams {
    id: DeviceId,
}
#[derive(Deserialize)]
struct AppParams {
    app: AppId,
}
#[derive(Deserialize)]
struct UninstallParams {
    app: AppId,
    #[serde(default)]
    purge: bool,
}

#[async_trait]
impl RpcHandler for RpcHub {
    async fn connected(&self, peer: RpcPeer) {
        self.peers.lock().push(peer);
    }

    async fn request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        if method.starts_with("host.") {
            return self.host_request(peer, method, params).await;
        }
        let Some(app) = self.app.get() else { return Err(not_ready()) };
        match method {
            "app.install" => {
                let p: InstallParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                let id = app.install_from_url(&p.url).await.map_err(|e| rpc_err(&e))?;
                Ok(json!({ "app": id }))
            }
            "app.uninstall" => {
                let p: UninstallParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                app.uninstall(&p.app, p.purge).await.map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            "app.list" => {
                let apps: Vec<Value> = app
                    .apps()
                    .into_iter()
                    .map(|a| {
                        let (state, message) = match &a.status {
                            AppStatus::Running => ("running", None),
                            AppStatus::Disabled => ("disabled", None),
                            AppStatus::Failed(m) => ("failed", Some(m.clone())),
                        };
                        json!({ "id": a.id, "name": a.name, "version": a.version, "state": state, "message": message })
                    })
                    .collect();
                Ok(json!({ "apps": apps }))
            }
            "app.enable" | "app.disable" | "app.deleteData" | "app.reload" => {
                let p: AppParams = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                match method {
                    "app.enable" => app.enable(&p.app).await,
                    "app.disable" => app.disable(&p.app).await,
                    "app.deleteData" => app.delete_data(&p.app).await,
                    _ => app.reload(&p.app).await,
                }
                .map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            "ui.event" => {
                let p: DispatchUiEvent = serde_json::from_value(params).map_err(|e| bad_params(&e))?;
                app.dispatch_ui_event(&p.app, p.event).await.map_err(|e| rpc_err(&e))?;
                Ok(Value::Null)
            }
            _ => Err(method_not_found(method)),
        }
    }
}

#[async_trait]
impl EventSink for RpcHub {
    async fn emit(&self, event: ServerEvent) {
        match event {
            ServerEvent::AppInstalled(app) => self.broadcast("app.installed", &json!({ "app": app })),
            ServerEvent::AppUninstalled(app) => self.broadcast("app.uninstalled", &json!({ "app": app })),
            ServerEvent::AppEnabled(app) => self.broadcast("app.enabled", &json!({ "app": app })),
            ServerEvent::AppDisabled(app) => self.broadcast("app.disabled", &json!({ "app": app })),
            ServerEvent::AppFailed { app, message } => {
                self.broadcast("app.failed", &json!({ "app": app, "message": message }));
            }
            ServerEvent::UiTree { app, root } => {
                if let Ok(v) = serde_json::to_value(UiTree { app, root }) {
                    self.broadcast("ui.tree", &v);
                }
            }
            ServerEvent::Notification(n) => {
                if let Ok(v) = serde_json::to_value(n) {
                    self.broadcast("notify", &v);
                }
            }
        }
    }
}

#[async_trait]
impl Notifier for RpcHub {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        self.emit(ServerEvent::Notification(n)).await;
        Ok(())
    }
}
