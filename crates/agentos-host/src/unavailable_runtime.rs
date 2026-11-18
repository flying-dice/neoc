//! TODO(lua51): adapter swap. A placeholder `AppRuntime` while the Luau adapter (`agentos-luau`) is out of
//! the build and its replacement on `agentos-luavm` is not written yet. Delete this module with it.
//!
//! Every call that would run app code fails with a typed [`PortError::Internal`]; the calls that only
//! release or poll (`unload`, `close_view`, `dirty_views`, `take_view_effects`) succeed as no-ops, so
//! shutdown and the view service's housekeeping do not trip over it.

use agentos_application::{
    ports::{
        AppData, AppPackage, AppRuntime, CallContext, LoadedApp, PortResult, RenderOutput, ViewEffect,
        ViewEventCall,
    },
    PortError, SourceId,
};
use agentos_domain::{AppId, Capabilities};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::sync::Arc;

/// An `AppRuntime` that refuses to run any app.
pub struct UnavailableRuntime;

fn unavailable<T>() -> PortResult<T> {
    Err(PortError::Internal(
        "the app runtime is unavailable: the Lua 5.1 adapter is not wired in yet".to_string(),
    ))
}

#[async_trait]
impl AppRuntime for UnavailableRuntime {
    async fn load(
        &self,
        _app: &AppId,
        _package: AppPackage,
        _data: Arc<dyn AppData>,
        _capabilities: Capabilities,
    ) -> PortResult<LoadedApp> {
        unavailable()
    }

    async fn unload(&self, _app: &AppId) -> PortResult<()> {
        Ok(())
    }

    async fn call_tool(
        &self,
        _app: &AppId,
        _tool: &str,
        _args: Value,
        _ctx: CallContext,
        _read_only: bool,
    ) -> PortResult<Value> {
        unavailable()
    }

    async fn write_row(
        &self,
        _app: &AppId,
        _source: &SourceId,
        _key: Value,
        _field: &str,
        _value: Value,
        _ctx: CallContext,
    ) -> PortResult<()> {
        unavailable()
    }

    async fn windows(&self, _app: &AppId) -> PortResult<Vec<String>> {
        unavailable()
    }

    async fn open_view(
        &self,
        _app: &AppId,
        _view: &str,
        _window: &str,
        _params: Map<String, Value>,
        _restored: Map<String, Value>,
    ) -> PortResult<RenderOutput> {
        unavailable()
    }

    async fn render_view(&self, _app: &AppId, _view: &str) -> PortResult<RenderOutput> {
        unavailable()
    }

    async fn dispatch_view_event(
        &self,
        _app: &AppId,
        _view: &str,
        _call: ViewEventCall,
        _ctx: CallContext,
    ) -> PortResult<bool> {
        unavailable()
    }

    async fn set_client_values(
        &self,
        _app: &AppId,
        _view: &str,
        _values: Map<String, Value>,
    ) -> PortResult<()> {
        unavailable()
    }

    async fn close_view(&self, _app: &AppId, _view: &str) -> PortResult<()> {
        Ok(())
    }

    async fn dirty_views(&self, _app: &AppId) -> PortResult<Vec<String>> {
        Ok(Vec::new())
    }

    async fn take_view_effects(&self, _app: &AppId) -> PortResult<Vec<(String, ViewEffect)>> {
        Ok(Vec::new())
    }
}
