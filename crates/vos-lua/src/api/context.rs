//! What a handler or a tool is called with: who is calling and when, and `ctx:fail(message)`.

use crate::error::ToolFailure;
use mlua_ir::{IntoLuaRet, Object, Registry, Shape};
use neoc_vm2::mlua::{self, Lua};
use serde::Serialize;
use vos_application::ports::CallContext;
use vos_domain::{time, ActorKind as DomainActorKind};

/// What makes a call: the person, from the app's page, or an agent, through a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vos:app")]
pub enum ActorKind {
    /// The person, from the app's page.
    User,
    /// An agent, through a tool.
    Agent,
}

/// Who is calling.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct Actor {
    /// The person or an agent.
    pub kind: ActorKind,
    /// The caller's id: `"user"` for the person, the agent's id for an agent.
    pub id: String,
    /// The name to show for the caller.
    pub name: String,
}

/// What a call that never returns gives back: nothing, ever. It is declared `never`, the bottom type Luau and
/// TypeScript both have under that name, so `return ctx:fail(...)` fits a handler whatever its result is. mlua-ir's
/// IR has no type for it; it is named here as the type of that name, which both generators write as it is.
pub enum Never {}

/// The name of the bottom type in Luau and in TypeScript.
pub const NEVER: &str = "never";

impl Shape for Never {
    fn shape(_: &mut Registry) -> mlua_ir::ir::Ty {
        mlua_ir::ir::Ty::named(NEVER)
    }
}

impl IntoLuaRet for Never {
    fn into_lua_ret(self, _: &Lua, _: &'static str) -> mlua::Result<mlua::Value> {
        match self {}
    }
}

/// What a handler of the app's backend, or a tool's `run`, is called with: who is calling and when.
#[derive(Clone, Debug, Object)]
#[shape(module = "vos:app")]
pub struct Context {
    actor: Actor,
    now: String,
}

impl Context {
    pub(crate) fn new(ctx: &CallContext) -> Self {
        let kind = match ctx.actor.kind {
            DomainActorKind::User => ActorKind::User,
            DomainActorKind::Agent => ActorKind::Agent,
        };
        Self {
            actor: Actor { kind, id: ctx.actor.id.clone(), name: ctx.actor.name.clone() },
            now: time::iso8601_utc(ctx.now_ms),
        }
    }
}

#[mlua_ir::object(module = "vos:app")]
impl Context {
    /// Who is calling. From the app's page it is the person (`kind` is `"user"`).
    #[get]
    pub fn actor(&self) -> Actor {
        self.actor.clone()
    }

    /// When the call was made: an ISO 8601 time in UTC.
    #[get]
    pub fn now(&self) -> String {
        self.now.clone()
    }

    /// End the call with `message`, which the caller is shown as it is: no file or line. A handler's caller is the
    /// page, whose `rpc` rejects with an application error (-32010) carrying the message; to fail with another
    /// JSON-RPC code, raise `error("RPC-32602 Title cannot be empty", 0)`. A tool's caller is the agent.
    pub fn fail(&self, message: String) -> mlua::Result<Never> {
        Err(mlua::Error::external(ToolFailure(message)))
    }
}
