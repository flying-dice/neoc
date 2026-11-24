//! `vos:docker`: the container framework an app that runs on Docker requires.
//!
//! The framework is Luau the host carries (`lua/docker/`, bundled into `types/generated/docker.luau` by
//! `bun run codegen`): [`crate::require`] runs it in the app's own VM the first time the app requires it, so the
//! Docker an app chose and the clients that reach it are that app's alone. It is written against `vos:app` and
//! `vnd:bollard` as an app would be, and has nothing an app has not; it keeps the app's choice in `app.kv` under
//! `host`.
//!
//! What is here is what a script is told of it: its module and types in the schema ([`declare`]), from which
//! `vos-codegen` writes the Luau and TypeScript declarations as it does for every other module. The records are
//! Rust types and the signatures are written from Rust types; the Luau is held to them by `bun run lint:luau`,
//! which checks `lua/docker/main.luau` (its table is annotated with the module's type) against the generated
//! declarations.
//!
//! A function that does something answers with a plain sentence for the person when it could not, and with nothing
//! when it did: what an app does with a failure of Docker is show it.

use mlua_ir::ir::{Field, Function, Ir, Module, ModuleKind, Param, Returns, Ty, TypeBody};
use mlua_ir::{param, Callback, Definition, IntoReturn, Registry, Shape};
use neoc_vm::modules::vnd::bollard::bollard::Client;
use serde::Serialize;
use std::collections::BTreeMap;

/// The name the framework is required under.
pub const MODULE: &str = "vos:docker";

/// The framework, as the host carries it.
pub(crate) const SOURCE: &str = include_str!("../../../types/generated/docker.luau");

/// Whether Docker can be reached, as a page shows it.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct Status {
    /// Docker answered.
    pub reachable: bool,
    /// Its version, when it can be reached; empty when not.
    pub version: String,
    /// The chosen address; empty for this machine's own Docker.
    pub host: String,
    /// Why it cannot be reached, as a sentence; empty when it can.
    pub problem: String,
}

/// The end of what a container wrote, as text fit to show.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct Logs {
    /// Valid UTF-8, without a terminal's colour sequences and control characters.
    pub text: String,
    /// There was more before it.
    pub truncated: bool,
}

/// A port of the container, published on the machine.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct Port {
    /// The machine's port.
    pub host: u16,
    /// The container's port.
    pub container: u16,
    /// UDP; TCP when left out.
    pub udp: Option<bool>,
}

/// A folder of the machine the container may read. It is never writable.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct Mount {
    /// The folder on the machine, a whole path.
    pub source: String,
    /// Where the container finds it.
    pub target: String,
}

/// What a container an app owns is made from. Everything it gets of the machine is here: nothing is published or
/// mounted that is not declared.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct Declaration {
    /// The image it runs.
    pub image: String,
    /// What the image starts with, in place of its own.
    pub entrypoint: Option<Vec<String>>,
    /// What that is given, in place of the image's own.
    pub command: Option<Vec<String>>,
    /// `NAME=value`, each.
    pub environment: Option<Vec<String>>,
    /// The ports it publishes.
    pub ports: Option<Vec<Port>>,
    /// The folders it may read.
    pub mounts: Option<Vec<Mount>>,
}

/// What an owned container is doing.
#[derive(Clone, Copy, Debug, Serialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vos:docker")]
pub enum ContainerState {
    /// There is none (or Docker is away).
    Absent,
    /// It runs.
    Running,
    /// Docker is starting it again.
    Restarting,
    /// It is there and does not run.
    Stopped,
}

/// How an owned container is doing.
#[derive(Clone, Debug, Serialize, Shape)]
#[shape(module = "vos:docker")]
pub struct ContainerStatus {
    /// What it is doing.
    pub state: ContainerState,
    /// Docker's own words for it (`Up 2 hours`).
    pub status: String,
    /// It was made from the declaration it was compared with.
    pub current: bool,
    /// Why it could not be made or started the last time that was tried; empty when it could.
    pub problem: String,
}

/// Docker's filters, for its lists and its events: `{ type = { "container" }, event = { "start", "die" } }`.
pub type Filters = BTreeMap<String, Vec<String>>;

/// A function's declaration: its parameters and what it returns are read from Rust types.
fn function<R: IntoReturn>(
    reg: &mut Registry,
    name: &str,
    doc: &str,
    params: fn(&mut Registry) -> Vec<Param>,
) -> Function {
    Function {
        name: name.to_owned(),
        doc: Some(doc.to_owned()),
        params: params(reg),
        returns: R::returns(reg),
        raises: None,
        is_async: false,
    }
}

/// One container the app owns: a table with its filters and its three functions, each called with `:`.
pub struct Container;

impl Container {
    /// A function of the container as a field: `container:<name>(...)`.
    fn method<R: IntoReturn>(
        reg: &mut Registry,
        name: &str,
        doc: &str,
        params: fn(&mut Registry) -> Vec<Param>,
    ) -> Field {
        let mut params = params(reg);
        params.insert(0, param::<Container>(reg, "self", &[]));
        Field {
            name: name.to_owned(),
            doc: Some(doc.to_owned()),
            ty: Ty::Function { params, returns: Box::new(R::returns(reg)) },
            default: None,
            readonly: false,
        }
    }
}

impl Shape for Container {
    fn shape(reg: &mut Registry) -> Ty {
        reg.named("Container", Some(MODULE), std::any::type_name::<Self>(), |reg| Definition {
            doc: Some(
                "One container the app owns, found by a label that carries the app's id (`vos.app=<app.id>`), never\n\
                 by its name alone."
                    .to_owned(),
            ),
            body: TypeBody::Record {
                fields: vec![
                    Field {
                        name: "filters".to_owned(),
                        doc: Some("Docker's filters for what happens to the container, for `docker.watch`.".to_owned()),
                        ty: Filters::shape(reg),
                        default: None,
                        readonly: false,
                    },
                    Self::method::<ContainerStatus>(
                        reg,
                        "status",
                        "How the container is doing, and whether it is what `declared` says (never, for none). With\n\
                         Docker away it is `absent`.",
                        |reg| vec![param::<Option<Declaration>>(reg, "declared", &[])],
                    ),
                    Self::method::<Option<String>>(
                        reg,
                        "apply",
                        "Have the container be what `declared` says: its image pulled when Docker has none, made and\n\
                         started; removed for none. One that is that already, and runs, is left running unless\n\
                         `always`. It is never changed: a change of the declaration is the container removed and made\n\
                         again. Answers with why it could not, as a sentence (a declaration that is not one is refused\n\
                         before Docker hears of it), and with nothing when it did.",
                        |reg| {
                            vec![
                            param::<Option<Declaration>>(reg, "declared", &[]),
                            param::<Option<bool>>(reg, "always", &[]),
                        ]
                        },
                    ),
                    Self::method::<(Option<Logs>, Option<String>)>(
                        reg,
                        "logs",
                        "The end of what the container wrote (`docker.logs`). Nothing, and no reason, when there is no\n\
                         container.",
                        |reg| vec![param::<Option<f64>>(reg, "tail", &[])],
                    ),
                ],
            },
        })
    }
}

/// A client, or why there is none, as a sentence.
type Connected = (Option<Client>, Option<String>);

/// The module and its types, as the schema has them.
fn module(reg: &mut Registry) -> Module {
    let container = Returns::One { ty: Container::shape(reg) };
    let functions = vec![
        function::<String>(reg, "host", "The chosen Docker's address; empty for this machine's own.", |_| vec![]),
        function::<Option<String>>(
            reg,
            "use",
            "Choose the Docker the app uses: `unix:///...`, `tcp://...`, `http://...` or `https://...`, or empty for\n\
             this machine's own. Answers with why the address was refused, and with nothing when it was taken (a\n\
             Docker that is not running there is a fine choice: `status` says so, and it may start later).",
            |reg| vec![param::<String>(reg, "host", &[])],
        ),
        function::<Status>(
            reg,
            "status",
            "Whether Docker can be reached. Never fails: a Docker that is away is an answer.",
            |_| vec![],
        ),
        function::<String>(
            reg,
            "sentence",
            "Why a `vnd:bollard` call failed, as a sentence for the person: Docker's own message when it has one,\n\
             the framework's words when Docker cannot be reached or is not answering.",
            |reg| vec![param::<Option<neoc_vm::modules::vnd::bollard::error::DockerError>>(reg, "err", &[])],
        ),
        function::<Connected>(
            reg,
            "reader",
            "The client that reads what Docker has (it waits 5 seconds for an answer), or why there is none, as a\n\
             sentence.",
            |_| vec![],
        ),
        function::<Connected>(reg, "actor", "The client that changes it (90 seconds).", |_| vec![]),
        function::<Connected>(reg, "puller", "The client that pulls an image (10 minutes).", |_| vec![]),
        function::<(Option<Logs>, Option<String>)>(
            reg,
            "logs",
            "The end of what a container wrote, by id or name: its output and its errors together, at most 256 KiB,\n\
             as valid UTF-8 without a terminal's colour sequences and control characters. `tail` is how many lines\n\
             from the end (200 when left out, at most 10000).",
            |reg| vec![param::<String>(reg, "container", &[]), param::<Option<f64>>(reg, "tail", &[])],
        ),
        function::<()>(
            reg,
            "watch",
            "Watch Docker for as long as the app is loaded, and call `changed` whenever what it has of `filters`\n\
             (Docker's own, for its events) may be different: something happened to a container, Docker came or\n\
             went, or another Docker was chosen. A Docker that is away is looked for again every 5 seconds. It\n\
             never returns: `task.spawn(docker.watch, filters, changed)`.",
            |reg| vec![param::<Filters>(reg, "filters", &[]), param::<Callback<fn()>>(reg, "changed", &[])],
        ),
        Function {
            returns: container,
            ..function::<()>(
                reg,
                "container",
                "The container named `name` (a new one is given that name) that is this app's.",
                |reg| vec![param::<String>(reg, "name", &[])],
            )
        },
    ];
    Module {
        name: MODULE.to_owned(),
        kind: ModuleKind::Require,
        doc: Some(
            "The container framework, over `vnd:bollard`.\n\n\
             `local docker = require(\"vos:docker\")` in the backend of an app that runs on Docker: which Docker (the\n\
             person's choice, kept in the app's settings), the clients that reach it, the watch for changes, and one\n\
             container the app owns, kept as its declaration says. The host carries it and runs it in the app's own\n\
             VM, so the Docker an app chose is that app's alone."
                .to_owned(),
        ),
        functions,
        constants: Vec::new(),
        objects: Vec::new(),
    }
}

/// Add the framework's module and its types to `ir`, which has `vnd:bollard`'s already.
pub(crate) fn declare(ir: &mut Ir) {
    let mut reg = Registry::new();
    let module = module(&mut reg);
    // only its own: what it reaches of `vnd:bollard` (the client, and every type the client reaches) is declared
    // by that module
    ir.types.extend(reg.into_types().into_iter().filter(|def| def.module.as_deref() == Some(MODULE)));
    ir.modules.push(module);
    ir.modules.sort_by(|a, b| a.name.cmp(&b.name));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The module is in the schema with the functions the Luau returns, and its types are its own.
    #[test]
    fn the_schema_has_the_framework_and_its_types() {
        let ir = crate::declarations::schema();
        let docker = ir.modules.iter().find(|m| m.name == MODULE).expect("vos:docker");
        let names: Vec<&str> = docker.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["host", "use", "status", "sentence", "reader", "actor", "puller", "logs", "watch", "container"]
        );
        // what the Luau returns is exactly that table (`lua/docker/main.luau`)
        for name in &names {
            assert!(SOURCE.contains(&format!("{name} = ")), "the framework has no `{name}`");
        }
        let container = ir.type_def("vos_docker_Container").expect("Container");
        let TypeBody::Record { fields } = &container.body else { panic!("{container:?}") };
        let fields: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(fields, ["filters", "status", "apply", "logs"]);
        assert_eq!(container.module.as_deref(), Some(MODULE));
        // the client is `vnd:bollard`'s own type, declared once
        assert_eq!(ir.types.iter().filter(|t| t.name == "vnd_bollard_Client").count(), 1);
    }
}
