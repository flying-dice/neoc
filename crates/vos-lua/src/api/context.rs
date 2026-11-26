//! What a handler or a tool is called with: who is calling and when, and `ctx:fail(message)`.

use crate::error::ToolFailure;
use mlua_ir::{Object, Shape};
use neoc_vm::mlua;
use serde::Serialize;
use std::convert::Infallible;
use vos_application::ports::CallContext;
use vos_domain::{time, ActorKind as DomainActorKind};

/// What makes a call: the person, from the app's page, an agent, through a tool, or another app's backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vos:app")]
pub enum ActorKind {
    /// The person, from the app's page.
    User,
    /// An agent, through a tool.
    Agent,
    /// Another app's backend: a request it sent (`app.request`), or its notification reaching a listener (`app.on`).
    App,
}

/// Who is calling.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:app")]
pub struct Actor {
    /// The person, an agent or another app.
    pub kind: ActorKind,
    /// The caller's id: `"user"` for the person, the agent's id for an agent, the app's id for an app.
    pub id: String,
    /// The name to show for the caller.
    pub name: String,
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
            DomainActorKind::App => ActorKind::App,
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
    /// JSON-RPC code, raise `error("RPC-32602 Title cannot be empty", 0)`. A tool's caller is the agent. It never
    /// returns (it is declared `never`), so `return ctx:fail(...)` fits a handler whatever its result is.
    pub fn fail(&self, message: String) -> mlua::Result<Infallible> {
        Err(mlua::Error::external(ToolFailure(message)))
    }
}
