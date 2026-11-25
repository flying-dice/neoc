//! The operating system's service manager: `vnd:service_manager` (the `service-manager` crate, and this module's own
//! discovery and configuration where the crate has none).
//!
//! ```lua
//! local sm = require("vnd:service_manager")
//!
//! local manager = assert(sm.manager())             -- systemd, launchd or the Windows Service Control Manager
//! for _, service in assert(manager:list()) do
//!   print(service.label, service.state, service.autostart)
//! end
//! assert(manager:install({ label = "my-app", program = "/usr/local/bin/my-app", autostart = true }))
//! assert(manager:start("my-app"))
//! print(assert(manager:get("my-app")).state)       --> running
//! assert(manager:configure("my-app", { autostart = false }))
//! assert(manager:stop("my-app"))
//! assert(manager:uninstall("my-app"))
//! ```
//!
//! The operating system is the source of truth. `list` and `get` ask it, every time, for every service it has,
//! whoever registered them (a service made by an installer, by an administrator or by another program is listed and
//! managed like one made through this module); nothing is remembered here. `configure` changes what the manager
//! keeps of one service and leaves the rest of its definition as it was.
//!
//! Managers: systemd on Linux (system or user level), launchd on macOS (LaunchDaemons or LaunchAgents) and the
//! Service Control Manager on Windows (system level). `manager()` takes the machine's own; `mock()` is an in-memory
//! one for tests, and with `NEOC_SERVICE_MANAGER=mock` in the environment `manager()` is one mock manager shared by
//! the whole process, so a program can be run without it touching the real services.
//!
//! What differs between managers is told, not hidden. `Manager:capabilities()` says which options a manager keeps
//! when a service is registered (`install_options`), which it can change afterwards (`configurable`) and which
//! fields of a `Service` it reports (`reported`). An option the manager cannot honour is an `unsupported` error and
//! is never ignored; a field it does not report is `nil`. Where a manager has no word for something (launchd has no
//! description) the field is `nil`, and a script shows it as unavailable.
//!
//! Rights: changing a service is a privileged operation of the operating system, and this module never raises its
//! own rights. It runs with the rights of the host process, and when the operating system refuses (no root, no
//! Administrator, no polkit approval) the call fails with `permission_denied`. Listing and inspecting need less and
//! usually work for anyone. Raising the rights of the host is a separate concern of the host.
//!
//! Not here: on Windows a service's program must itself speak the service dispatcher protocol to be run by the
//! Service Control Manager. This module registers and controls services; it does not run a program as one.
//!
//! A change a manager applies on the next start (a systemd drop-in, a launchd property list) is applied when the
//! service is restarted; `restart` is a stop and a start, so it picks the change up.
//!
//! Errors: a call returns `(value, nil)` or `(nil, err)`, where `err.kind` is stable and the other fields say what
//! was refused: `invalid_input`, `unavailable`, `not_found`, `already_exists`, `permission_denied`, `unsupported`,
//! `failed`, `other`. `err.reason` is the manager's own wording for a log; it differs between platforms.

// What mlua-ir's macros imply, for the module below (see `vnd:keyring`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

mod backend;
// Every manager is compiled everywhere so its parsers are tested everywhere; each is constructed only on its own platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod launchd;
mod mock;
mod ops;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod systemd;
mod validate;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows;

#[cfg(test)]
mod tests;

#[mlua_ir::module("vnd:service_manager")]
pub mod service_manager {
    use std::collections::BTreeMap;
    use std::sync::{Arc, OnceLock};

    use mlua_ir::{LuaError, Object, Options, Shape};
    use serde::{Deserialize, Serialize};

    use super::backend::Backend;
    use super::mock::Mock;
    use super::ops;

    /// The environment variable that makes `manager()` a mock manager (`mock` is its one value).
    const CHOICE: &str = "NEOC_SERVICE_MANAGER";

    /// Why a call failed. The kind is stable; `reason` is the manager's wording, for a log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// An argument or option was refused before any manager was asked: a label that is not a name, a program
        /// that is not an executable file, a value with a line break.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// This machine has no service manager that can be used (`systemctl` missing or systemd not running, a
        /// platform with none), or the one asked for is not this platform's.
        #[error("{reason}")]
        Unavailable {
            /// Why there is none.
            reason: String,
        },
        /// The manager has no service by that label.
        #[error("there is no service {label:?}")]
        NotFound {
            /// The label that was asked for.
            label: String,
        },
        /// The manager has a service by that label already.
        #[error("there is a service {label:?} already")]
        AlreadyExists {
            /// The label that was asked for.
            label: String,
        },
        /// The operating system refused for want of rights. Nothing was changed. This module does not raise its
        /// own rights: the host process needs them.
        #[error("not permitted to {operation}: {reason}")]
        PermissionDenied {
            /// What was attempted.
            operation: String,
            /// The operating system's words.
            reason: String,
        },
        /// This manager cannot do what was asked: an option it does not keep (`option` names it) or an operation
        /// it has no way to do. Nothing was changed.
        #[error("{reason}")]
        Unsupported {
            /// The option that is not supported, when one is.
            option: Option<String>,
            /// What is not supported, and by which manager.
            reason: String,
        },
        /// The manager refused or the operation failed (a program that exits at once, a service that cannot be
        /// started).
        #[error("{operation} failed: {reason}")]
        Failed {
            /// What was attempted.
            operation: String,
            /// The manager's words.
            reason: String,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    /// Where a manager keeps its services: for everyone on the machine, or for one user.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Level {
        /// Services of the machine (systemd system units, LaunchDaemons, Windows services). Changing them needs
        /// administrator rights.
        System,
        /// Services of the user the host runs as (systemd user units, LaunchAgents).
        User,
    }

    /// What a service is doing now.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum State {
        /// Running.
        Running,
        /// Registered and not running.
        Stopped,
        /// Ended with a failure, and has not been started since.
        Failed,
        /// Being started.
        Starting,
        /// Being stopped.
        Stopping,
        /// Paused (Windows).
        Paused,
        /// The manager did not say.
        Unknown,
    }

    /// When the manager starts a service again after it ended.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Restart {
        /// Never.
        Never,
        /// Whenever it ends.
        Always,
        /// When it ends with a failure.
        OnFailure,
    }

    /// A service as the manager has it now. A field the manager does not report is `nil`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct Service {
        /// The manager's own name for the service: the systemd unit without `.service`, the launchd label, the
        /// Windows service name. Use it as the label of every other call.
        pub label: String,
        /// What the manager says the service is for.
        pub description: Option<String>,
        /// What the service is doing now.
        pub state: State,
        /// The manager's own words about the state, when it has some (a result, a last exit status).
        pub state_detail: Option<String>,
        /// Whether the service starts by itself (at boot, or at login for a user level service). `nil` when the
        /// manager has no setting for it (a systemd unit that is `static`).
        pub autostart: Option<bool>,
        /// The program the service runs, as the manager has it.
        pub program: Option<String>,
        /// The program's arguments. systemd reports them joined by spaces: one that holds a space is shown split.
        pub args: Option<Vec<String>>,
        /// The whole command line when the manager keeps one string (Windows).
        pub command_line: Option<String>,
        /// The directory the program starts in.
        pub working_directory: Option<String>,
        /// Variables the manager sets for the program.
        pub environment: Option<BTreeMap<String, String>>,
        /// When the service is started again after it ended.
        pub restart: Option<Restart>,
        /// Seconds the manager waits before it starts the service again.
        pub restart_delay_secs: Option<u32>,
        /// The user the program runs as.
        pub user: Option<String>,
        /// The file the manager keeps the definition in, when it is one.
        pub path: Option<String>,
    }

    /// What `install` registers.
    #[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Options)]
    pub struct Spec {
        /// The name of the new service (see `Service.label`): letters, digits and `_ . - @ :`.
        pub label: String,
        /// The absolute path of an executable file.
        pub program: String,
        /// The program's arguments.
        pub args: Option<Vec<String>>,
        /// What the service is for (not kept by every manager).
        pub description: Option<String>,
        /// The absolute path of a directory the program starts in.
        pub working_directory: Option<String>,
        /// Variables for the program: letters, digits and `_`.
        pub environment: Option<BTreeMap<String, String>>,
        /// Start by itself: `false` when not given.
        pub autostart: Option<bool>,
        /// When to start it again after it ended: the manager's default when not given.
        pub restart: Option<Restart>,
        /// Seconds to wait before starting it again.
        pub restart_delay_secs: Option<u32>,
        /// The user the program runs as: the account the manager runs services as when not given (root or
        /// Administrator at system level).
        pub user: Option<String>,
    }

    impl Spec {
        /// The names of the options that were given, apart from the ones every service has.
        pub(crate) fn supplied(&self) -> Vec<&'static str> {
            let mut names = Vec::new();
            let flags = [
                ("args", self.args.is_some()),
                ("description", self.description.is_some()),
                ("working_directory", self.working_directory.is_some()),
                ("environment", self.environment.is_some()),
                ("autostart", self.autostart.is_some()),
                ("restart", self.restart.is_some()),
                ("restart_delay_secs", self.restart_delay_secs.is_some()),
                ("user", self.user.is_some()),
            ];
            for (name, given) in flags {
                if given {
                    names.push(name);
                }
            }
            names
        }
    }

    /// What `configure` changes: the options that are given, and nothing else of the service.
    #[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Options)]
    pub struct Patch {
        /// Start by itself, or not.
        pub autostart: Option<bool>,
        /// What the service is for.
        pub description: Option<String>,
        /// The absolute path of an executable file. The arguments stay unless `args` is given.
        pub program: Option<String>,
        /// The program's arguments; the whole list is replaced.
        pub args: Option<Vec<String>>,
        /// The absolute path of a directory the program starts in.
        pub working_directory: Option<String>,
        /// The variables the manager sets, replacing the ones it has (a file of variables is left alone).
        pub environment: Option<BTreeMap<String, String>>,
        /// When to start it again after it ended.
        pub restart: Option<Restart>,
        /// Seconds to wait before starting it again.
        pub restart_delay_secs: Option<u32>,
    }

    impl Patch {
        /// The names of the options that were given.
        pub(crate) fn supplied(&self) -> Vec<&'static str> {
            let mut names = Vec::new();
            let flags = [
                ("autostart", self.autostart.is_some()),
                ("description", self.description.is_some()),
                ("program", self.program.is_some()),
                ("args", self.args.is_some()),
                ("working_directory", self.working_directory.is_some()),
                ("environment", self.environment.is_some()),
                ("restart", self.restart.is_some()),
                ("restart_delay_secs", self.restart_delay_secs.is_some()),
            ];
            for (name, given) in flags {
                if given {
                    names.push(name);
                }
            }
            names
        }
    }

    /// Which manager `manager` opens.
    #[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Options)]
    pub struct ManagerOptions {
        /// `"system"` (the default) or `"user"`: where the services are. Windows has only the system level.
        pub level: Option<Level>,
    }

    /// What a manager reports and changes. Read it before offering an option to a person.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct Capabilities {
        /// Which manager: `systemd`, `launchd`, `windows` or `mock`.
        pub kind: String,
        /// Where its services are.
        pub level: Level,
        /// The options of `install` the manager keeps; any other given is `unsupported`.
        pub install_options: Vec<String>,
        /// The options of `configure` the manager can change; any other given is `unsupported`.
        pub configurable: Vec<String>,
        /// The fields of a `Service` the manager reports; the others are always `nil`.
        pub reported: Vec<String>,
    }

    /// A service for `mock` to start with, as if another program had registered it.
    #[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Options)]
    pub struct Seed {
        /// The label.
        pub label: String,
        /// What it is for.
        pub description: Option<String>,
        /// Its state: stopped when not given.
        pub state: Option<State>,
        /// Starts by itself: `false` when not given.
        pub autostart: Option<bool>,
        /// The program.
        pub program: Option<String>,
        /// The arguments.
        pub args: Option<Vec<String>>,
        /// The directory it starts in.
        pub working_directory: Option<String>,
        /// Its variables.
        pub environment: Option<BTreeMap<String, String>>,
        /// When it starts again.
        pub restart: Option<Restart>,
        /// Seconds before it starts again.
        pub restart_delay_secs: Option<u32>,
        /// The mock refuses every change of it with `permission_denied`, as a system service of another user.
        pub locked: Option<bool>,
        /// The mock fails to start it, as a program that exits at once.
        pub broken: Option<bool>,
    }

    /// A service manager: the machine's, or a mock. Copies of the handle are the same manager.
    #[derive(Clone, Object)]
    pub struct Manager {
        inner: Arc<dyn Backend>,
    }

    impl Manager {
        pub(crate) fn new(inner: Arc<dyn Backend>) -> Self {
            Self { inner }
        }

        #[cfg(test)]
        pub(crate) fn backend(&self) -> &dyn Backend {
            &*self.inner
        }
    }

    /// The manager of this machine (systemd on Linux, launchd on macOS, the Service Control Manager on Windows),
    /// at the system level unless `level` says `"user"`. A machine whose manager is not there is `unavailable`.
    /// With `NEOC_SERVICE_MANAGER=mock` in the environment it is the process's one mock manager instead, with a
    /// few services of "another program" in it (`mock-nginx`, `mock-cups`, `mock-locked`, `mock-broken`).
    ///
    /// ```luau
    /// local manager = assert(service_manager.manager())
    /// print(manager:capabilities().kind)
    /// ```
    #[host("Wait")]
    pub async fn manager(options: Option<ManagerOptions>) -> Result<Manager, Error> {
        let options = options.unwrap_or_default();
        ops::blocking(move || ops::open(std::env::var(CHOICE).ok().as_deref(), options.level)).await
    }

    /// A new mock manager, for tests: in memory, its own, and gone with the process. It starts with `seeds` as if
    /// other programs had registered them, and it keeps every option, so it is the one to try a script against.
    ///
    /// ```luau
    /// local manager = service_manager.mock({ { label = "nginx", state = "running", autostart = true } })
    /// assert(manager:get("nginx").state == "running")
    /// assert(manager:stop("nginx"))
    /// assert(manager:get("nginx").state == "stopped")
    /// ```
    pub fn mock(seeds: Option<Vec<Seed>>) -> Manager {
        Manager::new(Arc::new(Mock::seeded(seeds.unwrap_or_default())))
    }

    pub(crate) fn shared_mock() -> Manager {
        static SHARED: OnceLock<Manager> = OnceLock::new();
        SHARED.get_or_init(|| Manager::new(Arc::new(Mock::fixture()))).clone()
    }

    pub(crate) fn choice() -> &'static str {
        CHOICE
    }

    #[mlua_ir::object]
    impl Manager {
        /// What this manager reports and changes.
        pub fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        /// Every service the manager has registered, whoever registered it, by label. Asked of the operating
        /// system on every call.
        #[host("Wait")]
        pub async fn list(&self) -> Result<Vec<Service>, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::list(&*inner)).await
        }

        /// One service as the manager has it now, or `(nil, nil)` when it has none by that label.
        #[host("Wait")]
        pub async fn get(&self, label: String) -> Result<Option<Service>, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::get(&*inner, &label)).await
        }

        /// Register a new service and answer it as the manager now has it. It is not started. A label the manager
        /// has is `already_exists`; an option the manager does not keep is `unsupported`; a program that is not an
        /// absolute path of an executable file is `invalid_input`. Needs the rights of the level
        /// (`permission_denied` without them).
        #[host("Wait")]
        pub async fn install(&self, spec: Spec) -> Result<Service, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::install(&*inner, spec)).await
        }

        /// Remove the registration of a service, whoever made it. The program it ran is left where it is. A
        /// service whose definition the manager keeps as part of the system (a vendor unit) is `unsupported`.
        #[host("Wait")]
        pub async fn uninstall(&self, label: String) -> Result<(), Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::uninstall(&*inner, &label)).await
        }

        /// Start a service and answer it as it now is. A service that is running is started already.
        #[host("Wait")]
        pub async fn start(&self, label: String) -> Result<Service, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::control(&*inner, &label, ops::Control::Start)).await
        }

        /// Stop a service and answer it as it now is.
        #[host("Wait")]
        pub async fn stop(&self, label: String) -> Result<Service, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::control(&*inner, &label, ops::Control::Stop)).await
        }

        /// Stop a service and start it again, and answer it as it now is. Changes a manager applies on a start
        /// are applied by it.
        #[host("Wait")]
        pub async fn restart(&self, label: String) -> Result<Service, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::control(&*inner, &label, ops::Control::Restart)).await
        }

        /// Change what the manager keeps of a service, whoever registered it, and answer it as the manager now
        /// has it. Only the options given change; the rest of the definition, whatever the manager keeps in it, is
        /// left as it was. An option the manager cannot change is `unsupported` (see `capabilities`).
        #[host("Wait")]
        pub async fn configure(&self, label: String, patch: Patch) -> Result<Service, Error> {
            let inner = self.inner.clone();
            ops::blocking(move || ops::configure(&*inner, &label, patch)).await
        }
    }
}
