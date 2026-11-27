//! `app.tools`: the tools of every running app, for an app that offers them to an agent of its own, here
//! (`app.tools.call`) or over the host's `/mcp` with a token it leases for it (`app.tools.lease`).

use super::{app::tools, json::Json, shared};
use mlua_ir::ir::Ty;
use mlua_ir::{LuaError, Registry, Shape, Unknown};
use neoc_vm::mlua;
use neoc_vm::mlua::Lua;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use vos_application::PortError;
use vos_domain::{Actor, QualifiedToolName};

mlua_ir::unknown_reason!(
    /// Why a tool's input has no declared shape here.
    ToolInput = "the tool's input: its shape is that tool's (its contract's `params`)"
);
mlua_ir::unknown_reason!(
    /// Why a tool's result has no declared shape.
    ToolOutput = "whatever JSON the tool answers the agent with"
);

mlua_ir::unknown_reason!(
    /// Why a part of a tool's input schema has no declared shape.
    JsonSchema = "a JSON Schema: its keywords are the schema's own"
);

/// The JSON Schema of a tool's input, as a field of a record: an object of keywords.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct InputSchema(Value);

impl Shape for InputSchema {
    fn shape(reg: &mut Registry) -> Ty {
        BTreeMap::<String, Unknown<JsonSchema>>::shape(reg)
    }
}

/// A tool of a running app, as an agent is shown it.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct HostTool {
    /// `<appId>__<tool>`: what `app.tools.call` takes.
    pub name: String,
    /// The app it belongs to.
    pub app: String,
    /// The name a person is shown.
    pub title: Option<String>,
    /// What the tool does.
    pub description: String,
    /// The JSON Schema of its input.
    pub input_schema: InputSchema,
    /// It changes nothing: it runs with every write refused.
    pub read_only: bool,
}

/// The agent of the app's own a tool is called for: who the tool is told called it (its `ctx.actor`).
#[derive(Clone, Debug, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(module = "vos:app")]
pub struct Caller {
    /// The app's own id for its agent, 1 to 200 bytes.
    pub id: String,
    /// The agent's name, 1 to 200 bytes.
    pub name: String,
}

/// What `app.tools.lease` takes besides the agent.
#[derive(Clone, Debug, Default, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(module = "vos:app")]
pub struct LeaseOptions {
    /// The host name or IP address the URL names: the address this machine has where the agent is (the local
    /// address of the connection to it). The host's own address when left out.
    pub host: Option<String>,
}

/// A bearer token for the host's `/mcp`, leased for an agent of the app's own, and where the agent uses it.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct Lease {
    /// The bearer token (`Authorization: Bearer <token>`).
    pub token: String,
    /// `<scheme>://<host>:<port>/mcp`.
    pub url: String,
}

/// Why a tool of a running app gave no result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "vos:app")]
pub enum ToolCallError {
    /// No running app has the tool: it does not exist, or its app is not running.
    #[error("{text}")]
    NotFound {
        #[serde(skip)]
        text: String,
    },
    /// The tool failed: it refused its input, raised, or said it could not (`ctx:fail`).
    #[error("{text}")]
    Failed {
        #[serde(skip)]
        text: String,
    },
    /// The host could not run it.
    #[error("{text}")]
    Internal {
        #[serde(skip)]
        text: String,
    },
}

impl From<PortError> for ToolCallError {
    fn from(error: PortError) -> Self {
        let text = error.to_string();
        match error {
            PortError::NotFound(_) => Self::NotFound { text },
            PortError::Rejected(_) | PortError::Rpc { .. } => Self::Failed { text },
            PortError::Internal(_) => Self::Internal { text },
        }
    }
}

/// The named arguments a tool is called with.
pub type ToolArgs = Json<BTreeMap<String, Unknown<ToolInput>>>;

/// The longest id or name of the agent a tool call is made for.
const MAX_CALLER: usize = 200;

/// The actor a tool is told called it for `agent`, an agent of the app's own: refused (by `what`) when its `id` or
/// `name` is not 1 to 200 bytes.
fn caller(sh: &super::Sh, what: &str, agent: &Caller) -> mlua::Result<Actor> {
    for (key, value) in [("id", &agent.id), ("name", &agent.name)] {
        if value.is_empty() || value.len() > MAX_CALLER {
            return Err(mlua::Error::runtime(format!(
                "{what}: the agent needs `{key}`, a string of 1 to {MAX_CALLER} bytes"
            )));
        }
    }
    // the id is the app's own for its agent: under the app's name, it is never the id of an agent the person
    // connected to this machine, nor of another app's
    Ok(Actor::agent(&format!("{}:{}", sh.app, agent.id), &agent.name))
}

#[mlua_ir::object(module = "vos:app")]
impl tools {
    /// Every tool of every running app (this one's too), in name order.
    pub fn all(lua: &Lua) -> mlua::Result<Vec<HostTool>> {
        let sh = shared(lua)?;
        Ok(sh
            .host_tools
            .all()
            .into_iter()
            .map(|(name, spec)| HostTool {
                app: name.app().to_string(),
                name: name.to_string(),
                title: spec.title,
                description: spec.description,
                input_schema: InputSchema(spec.input_schema),
                read_only: spec.read_only,
            })
            .collect())
    }

    /// Run a tool of a running app for an agent of this app's own, with `args` (none when left out): `agent` is who
    /// the tool is told called it (its `ctx.actor`). Nothing checks what that agent may call: the app decides whom it offers the tools to. The
    /// result is `(result, nil)`, or `(nil, err)` when the tool failed, does not exist or its app is not running.
    /// Raises for a name that is not `<appId>__<tool>`, an agent whose `id` or `name` is not 1 to 200 bytes, and
    /// inside a read-only call. It waits without the app's turn: the tool runs in another app's VM, or in this
    /// one's as any other call.
    #[host("Wait")]
    pub async fn call(
        lua: Lua,
        name: String,
        agent: Caller,
        args: Option<ToolArgs>,
    ) -> mlua::Result<Result<Json<Unknown<ToolOutput>>, ToolCallError>> {
        let sh = shared(&lua)?;
        sh.writable("app.tools.call")?;
        let name = QualifiedToolName::parse(&name).map_err(|e| mlua::Error::runtime(e.to_string()))?;
        let caller = caller(&sh, "app.tools.call", &agent)?;
        let args = match args.map(|args| args.value) {
            Some(args @ Value::Object(_)) => args,
            // none given, or an empty table (which reads as `[]`): no arguments
            _ => json!({}),
        };
        Ok(sh.host_tools.call(caller, &name, args).await.map(Json::new).map_err(ToolCallError::from))
    }

    /// Lease a bearer token for the host's `/mcp` for an agent of this app's own, so an agent elsewhere calls the
    /// tools `app.tools.call` runs for it: every tool of every running app but this one's, the tool told the same
    /// `ctx.actor`. Like `app.tools.call`, nothing checks whom the app gives it to. The lease is in memory only, never
    /// in the person's roster: it works until `app.tools.release(token)`, or until this app's backend stops (it is
    /// stopped, reloaded or uninstalled, or the host stops). Raises for an agent whose `id` or `name` is not 1 to 200
    /// bytes, a `host` that is not a host name or an IP address, and inside a read-only call.
    pub fn lease(lua: &Lua, agent: Caller, opts: Option<LeaseOptions>) -> mlua::Result<Lease> {
        let sh = shared(lua)?;
        sh.writable("app.tools.lease")?;
        let actor = caller(&sh, "app.tools.lease", &agent)?;
        let host = opts.and_then(|o| o.host);
        let lease = sh
            .leases
            .lease(&sh.app, &actor, host.as_deref())
            .map_err(|e| mlua::Error::runtime(format!("app.tools.lease: {e}")))?;
        Ok(Lease { token: lease.token, url: lease.url })
    }

    /// Revoke a token this app leased: it stops working at once, and the agent's MCP sessions are told their tool
    /// list changed. A token that is not one of this app's leases (released already, say) is left alone. Raises
    /// inside a read-only call.
    #[host("Wait")]
    pub async fn release(lua: Lua, token: String) -> mlua::Result<()> {
        let sh = shared(&lua)?;
        sh.writable("app.tools.release")?;
        sh.leases.release(&sh.app, &token).await;
        Ok(())
    }
}
