//! Use cases. They depend only on ports and the domain.

use crate::{
    ports::{
        AgentConnector, AppRuntime, DataStore, EventSink, Fetcher, RunId, ServerEvent, SqlMigration,
        ToolRegistry,
    },
    AppError, PortError,
};
use agentos_domain::{ui::UiEvent, AgentId, AgentIdentity, AppId, AppRef, QualifiedToolName};
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

pub struct AppService {
    pub fetcher: Arc<dyn Fetcher>,
    pub data: Arc<dyn DataStore>,
    pub runtime: Arc<dyn AppRuntime>,
    pub tools: Arc<dyn ToolRegistry>,
    pub events: Arc<dyn EventSink>,
    pub agents: Arc<dyn AgentConnector>,
    installed: Mutex<HashSet<AppId>>,
}

impl AppService {
    pub fn new(
        fetcher: Arc<dyn Fetcher>,
        data: Arc<dyn DataStore>,
        runtime: Arc<dyn AppRuntime>,
        tools: Arc<dyn ToolRegistry>,
        events: Arc<dyn EventSink>,
        agents: Arc<dyn AgentConnector>,
    ) -> Self {
        Self { fetcher, data, runtime, tools, events, agents, installed: Mutex::default() }
    }

    /// Install from an app-ref URL (file://, http://, https://: the Fetcher adapter decides how).
    pub async fn install_from_url(&self, app_ref_url: &str) -> Result<AppId, AppError> {
        let bytes = self.fetcher.fetch(app_ref_url).await?;
        let app_ref: AppRef = serde_json::from_slice(&bytes)
            .map_err(|e| PortError::Rejected(format!("bad app ref at {app_ref_url}: {e}")))?;
        self.install(&app_ref).await
    }

    pub async fn install(&self, app_ref: &AppRef) -> Result<AppId, AppError> {
        let m = &app_ref.manifest;
        m.validate()?;
        if self.installed.lock().contains(&m.id) {
            return Err(AppError::AlreadyInstalled(m.id.to_string()));
        }
        // fetch + verify every file the manifest points at, before touching any state
        let main = self.fetch_verified(app_ref, &m.main).await?;
        let mut migrations = Vec::new();
        for spec in &m.migrations {
            let sql = String::from_utf8(self.fetch_verified(app_ref, &spec.file).await?)
                .map_err(|_| PortError::Rejected(format!("{} is not UTF-8", spec.file)))?;
            migrations.push(SqlMigration { version: spec.version, name: spec.name.clone(), sql });
        }

        let store = self.data.open(&m.id, migrations).await?;
        let loaded = match self.runtime.load(&m.id, main, store).await {
            Ok(l) => l,
            Err(e) => return Err(e.into()), // nothing registered yet: nothing to roll back
        };
        self.tools.publish(&m.id, loaded.tools).await?;
        self.installed.lock().insert(m.id.clone());
        self.events.emit(ServerEvent::AppInstalled(m.id.clone())).await;
        Ok(m.id.clone())
    }

    pub async fn uninstall(&self, app: &AppId, purge_data: bool) -> Result<(), AppError> {
        if !self.installed.lock().remove(app) {
            return Err(AppError::NotInstalled(app.to_string()));
        }
        self.tools.retract(app).await?;
        self.runtime.unload(app).await?;
        if purge_data {
            self.data.purge(app).await?;
        }
        self.events.emit(ServerEvent::AppUninstalled(app.clone())).await;
        Ok(())
    }

    pub async fn dispatch_ui_event(&self, app: &AppId, event: UiEvent) -> Result<(), AppError> {
        self.require_installed(app)?;
        if let Some(root) = self.runtime.dispatch_event(app, event).await? {
            self.events.emit(ServerEvent::UiTree { app: app.clone(), root }).await;
        }
        Ok(())
    }

    /// MCP `tools/call`: authorize the calling agent, then run the tool in the app's VM.
    pub async fn call_tool(
        &self,
        caller: &AgentIdentity,
        name: &QualifiedToolName,
        args: Value,
    ) -> Result<Value, AppError> {
        caller.authorize_tool_call(name)?;
        self.require_installed(name.app())?;
        Ok(self.runtime.call_tool(name.app(), name.tool(), args).await?)
    }

    /// Host shutdown: retract every app's tools, unload its VM (data is kept) and stop the agent
    /// processes. Nothing is purged and clients are not told the apps were uninstalled: they are
    /// coming back with the host.
    pub async fn shutdown(&self) {
        let apps: Vec<AppId> = self.installed.lock().drain().collect();
        for app in apps {
            if let Err(e) = self.tools.retract(&app).await {
                tracing::warn!(%app, error = %e, "retracting tools failed during shutdown");
            }
            if let Err(e) = self.runtime.unload(&app).await {
                tracing::warn!(%app, error = %e, "unloading app failed during shutdown");
            }
        }
        self.agents.stop_all().await;
    }

    /// Ids of the installed apps, sorted.
    pub fn installed_apps(&self) -> Vec<AppId> {
        let mut apps: Vec<AppId> = self.installed.lock().iter().cloned().collect();
        apps.sort();
        apps
    }

    pub async fn start_agent_run(&self, agent: &AgentId, prompt: &str) -> Result<RunId, AppError> {
        Ok(self.agents.start_run(agent, prompt).await?)
    }

    fn require_installed(&self, app: &AppId) -> Result<(), AppError> {
        if self.installed.lock().contains(app) {
            Ok(())
        } else {
            Err(AppError::NotInstalled(app.to_string()))
        }
    }

    async fn fetch_verified(&self, app_ref: &AppRef, path: &str) -> Result<Vec<u8>, AppError> {
        let url = &app_ref.file(path)?.url;
        let bytes = self.fetcher.fetch(url).await?;
        app_ref.verify(path, &bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fakes;
    use agentos_domain::{AgentId, DomainError, Permission};
    use serde_json::json;

    fn agent(grants: Vec<Permission>) -> AgentIdentity {
        AgentIdentity { id: AgentId("claude".into()), name: "Claude".into(), grants }
    }

    #[tokio::test]
    async fn install_verifies_files_then_wires_data_runtime_tools_and_announces() {
        let f = Fakes::new();
        let url = f.add_app("notes", &[("add_note", "x")]);
        let svc = f.service();
        let id = svc.install_from_url(&url).await.unwrap();
        assert_eq!(id.as_str(), "notes");
        assert_eq!(f.registry.names(), vec!["notes__add_note".to_string()]);
        assert_eq!(f.events.take(), vec![ServerEvent::AppInstalled(id.clone())]);
        assert_eq!(f.store.opened_with_migrations("notes"), Some(1));
        assert!(matches!(svc.install_from_url(&url).await, Err(AppError::AlreadyInstalled(_))));
    }

    #[tokio::test]
    async fn tampered_file_is_rejected_before_any_state_changes() {
        let f = Fakes::new();
        let url = f.add_app("notes", &[("t", "x")]);
        f.tamper("notes");
        let err = f.service().install_from_url(&url).await.unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::ChecksumMismatch { .. })), "{err:?}");
        assert!(f.registry.names().is_empty());
        assert_eq!(f.store.opened_with_migrations("notes"), None);
    }

    #[tokio::test]
    async fn uninstall_retracts_tools_unloads_vm_and_optionally_purges() {
        let f = Fakes::new();
        let url = f.add_app("notes", &[("add_note", "x")]);
        let svc = f.service();
        let id = svc.install_from_url(&url).await.unwrap();
        svc.uninstall(&id, true).await.unwrap();
        assert!(f.registry.names().is_empty());
        assert_eq!(f.runtime.loaded(), Vec::<String>::new());
        assert!(f.store.purged("notes"));
        assert!(matches!(svc.uninstall(&id, false).await, Err(AppError::NotInstalled(_))));
    }

    #[tokio::test]
    async fn shutdown_unloads_every_app_keeps_data_and_stops_agents() {
        let f = Fakes::new();
        let svc = f.service();
        svc.install_from_url(&f.add_app("notes", &[("add_note", "x")])).await.unwrap();
        svc.install_from_url(&f.add_app("todo", &[("add_task", "x")])).await.unwrap();
        f.events.take();

        svc.shutdown().await;
        assert!(f.registry.names().is_empty());
        assert!(f.runtime.loaded().is_empty());
        assert!(!f.store.purged("notes"), "shutdown keeps app data");
        assert_eq!(f.agents.stop_calls(), 1);
        assert!(f.events.take().is_empty(), "clients are not told the apps were uninstalled");
        assert!(svc.installed_apps().is_empty());
    }

    #[tokio::test]
    async fn tool_calls_are_permission_checked_per_agent() {
        let f = Fakes::new();
        let url = f.add_app("notes", &[("add_note", "x")]);
        let svc = f.service();
        let id = svc.install_from_url(&url).await.unwrap();
        let name = QualifiedToolName::parse("notes__add_note").unwrap();

        let ok = svc
            .call_tool(&agent(vec![Permission::CallTools(id.clone())]), &name, json!({"a":1}))
            .await
            .unwrap();
        assert_eq!(ok, json!({"echo": {"a":1}}));
        let denied = svc.call_tool(&agent(vec![]), &name, json!({})).await.unwrap_err();
        assert!(matches!(denied, AppError::Domain(DomainError::PermissionDenied { .. })));
        assert_eq!(f.runtime.calls(), 1, "a denied call must never reach the VM");
    }
}
