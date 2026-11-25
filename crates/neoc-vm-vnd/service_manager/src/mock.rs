//! The in-memory manager: `mock()`, and the process's shared one under `NEOC_SERVICE_MANAGER=mock`. It keeps what a
//! script gives it and nothing else, so it is a stand-in for an operating system whose services were made by others.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use super::backend::Backend;
use super::service_manager::{Capabilities, Error, Level, Patch, Restart, Seed, Service, Spec, State};

/// Every option, so a script can be tried against it. Not `user`: the mock has no accounts, which also makes `user`
/// the option a script can see refused as `unsupported`.
const OPTIONS: [&str; 7] =
    ["args", "description", "working_directory", "environment", "autostart", "restart", "restart_delay_secs"];

#[derive(Default)]
struct State_ {
    services: BTreeMap<String, Service>,
    locked: BTreeSet<String>,
    broken: BTreeSet<String>,
}

pub(super) struct Mock {
    state: Mutex<State_>,
}

fn service(label: &str) -> Service {
    Service {
        label: label.to_owned(),
        description: None,
        state: State::Stopped,
        state_detail: None,
        autostart: Some(false),
        program: None,
        args: None,
        command_line: None,
        working_directory: None,
        environment: None,
        restart: None,
        restart_delay_secs: None,
        user: None,
        path: None,
    }
}

impl Mock {
    pub(super) fn seeded(seeds: Vec<Seed>) -> Self {
        let mock = Self { state: Mutex::new(State_::default()) };
        for seed in seeds {
            mock.seed(seed);
        }
        mock
    }

    /// What `NEOC_SERVICE_MANAGER=mock` starts with: services of "other programs", one of them refusing changes and
    /// one that cannot be started.
    pub(super) fn fixture() -> Self {
        let seed = |label: &str, description: &str, state: State, autostart: bool| Seed {
            label: label.to_owned(),
            description: Some(description.to_owned()),
            state: Some(state),
            autostart: Some(autostart),
            program: Some(format!("/usr/sbin/{label}")),
            args: None,
            working_directory: None,
            environment: None,
            restart: Some(Restart::OnFailure),
            restart_delay_secs: Some(5),
            locked: None,
            broken: None,
        };
        let mut locked = seed(
            "mock-locked",
            "A service of another user, which this host may not change",
            State::Running,
            true,
        );
        locked.locked = Some(true);
        let mut broken = seed("mock-broken", "A service whose program exits at once", State::Failed, false);
        broken.broken = Some(true);
        Self::seeded(vec![
            seed("mock-nginx", "A web server", State::Running, true),
            seed("mock-cups", "A printing service", State::Stopped, false),
            locked,
            broken,
        ])
    }

    fn seed(&self, seed: Seed) {
        let mut s = service(&seed.label);
        s.description = seed.description;
        s.state = seed.state.unwrap_or(State::Stopped);
        s.autostart = Some(seed.autostart.unwrap_or(false));
        s.program = seed.program;
        s.args = seed.args;
        s.working_directory = seed.working_directory;
        s.environment = seed.environment;
        s.restart = seed.restart;
        s.restart_delay_secs = seed.restart_delay_secs;
        let mut state = self.state.lock().expect("the mock's lock");
        if seed.locked == Some(true) {
            state.locked.insert(seed.label.clone());
        }
        if seed.broken == Some(true) {
            state.broken.insert(seed.label.clone());
        }
        state.services.insert(seed.label, s);
    }

    /// A change made behind the manager's back, as another program would make it (the tests use it).
    #[cfg(test)]
    pub(super) fn externally(&self, label: &str, change: impl FnOnce(&mut Service)) {
        let mut state = self.state.lock().expect("the mock's lock");
        match state.services.get_mut(label) {
            Some(s) => change(s),
            None => {
                let mut s = service(label);
                change(&mut s);
                state.services.insert(label.to_owned(), s);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn externally_removed(&self, label: &str) {
        self.state.lock().expect("the mock's lock").services.remove(label);
    }

    fn denied(state: &State_, label: &str, operation: &str) -> Result<(), Error> {
        if state.locked.contains(label) {
            return Err(Error::PermissionDenied {
                operation: operation.to_owned(),
                reason: format!("the mock service {label:?} belongs to another user"),
            });
        }
        Ok(())
    }
}

impl Backend for Mock {
    fn capabilities(&self) -> Capabilities {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Capabilities {
            kind: "mock".to_owned(),
            level: Level::System,
            install_options: names(&OPTIONS),
            configurable: names(&[
                "autostart",
                "description",
                "program",
                "args",
                "working_directory",
                "environment",
                "restart",
                "restart_delay_secs",
            ]),
            reported: names(&[
                "description",
                "state",
                "state_detail",
                "autostart",
                "program",
                "args",
                "working_directory",
                "environment",
                "restart",
                "restart_delay_secs",
            ]),
        }
    }

    fn probe(&self) -> Result<(), Error> {
        Ok(())
    }

    fn list(&self) -> Result<Vec<Service>, Error> {
        Ok(self.state.lock().expect("the mock's lock").services.values().cloned().collect())
    }

    fn get(&self, label: &str) -> Result<Option<Service>, Error> {
        Ok(self.state.lock().expect("the mock's lock").services.get(label).cloned())
    }

    fn install(&self, spec: &Spec) -> Result<(), Error> {
        let mut s = service(&spec.label);
        s.description = spec.description.clone();
        s.autostart = Some(spec.autostart.unwrap_or(false));
        s.program = Some(spec.program.clone());
        s.args = Some(spec.args.clone().unwrap_or_default());
        s.working_directory = spec.working_directory.clone();
        s.environment = spec.environment.clone();
        s.restart = spec.restart;
        s.restart_delay_secs = spec.restart_delay_secs;
        self.state.lock().expect("the mock's lock").services.insert(spec.label.clone(), s);
        Ok(())
    }

    fn uninstall(&self, label: &str) -> Result<(), Error> {
        let mut state = self.state.lock().expect("the mock's lock");
        Self::denied(&state, label, "uninstall")?;
        state.services.remove(label);
        Ok(())
    }

    fn start(&self, label: &str) -> Result<(), Error> {
        let mut state = self.state.lock().expect("the mock's lock");
        Self::denied(&state, label, "start")?;
        if state.broken.contains(label) {
            if let Some(s) = state.services.get_mut(label) {
                s.state = State::Failed;
                s.state_detail = Some("exit-code".to_owned());
            }
            return Err(Error::Failed {
                operation: "start".to_owned(),
                reason: format!("{label} exited with code 1 at once"),
            });
        }
        if let Some(s) = state.services.get_mut(label) {
            s.state = State::Running;
            s.state_detail = None;
        }
        Ok(())
    }

    fn stop(&self, label: &str) -> Result<(), Error> {
        let mut state = self.state.lock().expect("the mock's lock");
        Self::denied(&state, label, "stop")?;
        if let Some(s) = state.services.get_mut(label) {
            s.state = State::Stopped;
            s.state_detail = None;
        }
        Ok(())
    }

    fn restart(&self, label: &str) -> Result<(), Error> {
        self.stop(label)?;
        self.start(label)
    }

    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error> {
        let mut state = self.state.lock().expect("the mock's lock");
        Self::denied(&state, label, "configure")?;
        let Some(s) = state.services.get_mut(label) else {
            return Err(Error::NotFound { label: label.to_owned() });
        };
        if let Some(v) = patch.autostart {
            s.autostart = Some(v);
        }
        if let Some(v) = &patch.description {
            s.description = Some(v.clone());
        }
        if let Some(v) = &patch.program {
            s.program = Some(v.clone());
        }
        if let Some(v) = &patch.args {
            s.args = Some(v.clone());
        }
        if let Some(v) = &patch.working_directory {
            s.working_directory = Some(v.clone());
        }
        if let Some(v) = &patch.environment {
            s.environment = Some(v.clone());
        }
        if let Some(v) = patch.restart {
            s.restart = Some(v);
        }
        if let Some(v) = patch.restart_delay_secs {
            s.restart_delay_secs = Some(v);
        }
        Ok(())
    }
}
