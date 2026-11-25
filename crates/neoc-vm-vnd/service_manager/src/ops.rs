//! The operations of a [`Manager`](super::service_manager::Manager), written once for every backend: the checks
//! that do not depend on the manager (a label, a spec, an option it does not keep, a service it does not have),
//! and the answer a call gives (the service as it now is).

use std::sync::Arc;

use super::backend::Backend;
use super::service_manager::{
    Capabilities, Error, Level, Manager, Patch, Service, Spec, choice, shared_mock,
};
use super::validate;

pub(super) enum Control {
    Start,
    Stop,
    Restart,
}

/// Runs `job` on the blocking pool and waits for it: a manager call runs programs.
pub(super) async fn blocking<F, R>(job: F) -> Result<R, Error>
where
    F: FnOnce() -> Result<R, Error> + Send + 'static,
    R: Send + 'static,
{
    // Do nothing on the first poll: where Luau refuses the yield the call is dropped after it, and a job started
    // now would change a service although the script was told the call failed (`std:fs` has the same rule).
    tokio::task::yield_now().await;
    tokio::task::spawn_blocking(job)
        .await
        .unwrap_or_else(|e| Err(Error::Other { reason: format!("the service manager job failed: {e}") }))
}

/// The manager `manager()` opens for a value of `NEOC_SERVICE_MANAGER` (`None` when it is not set).
pub(super) fn open(env_choice: Option<&str>, level: Option<Level>) -> Result<Manager, Error> {
    match env_choice {
        None | Some("") => {}
        Some("mock") => return Ok(shared_mock()),
        Some(other) => {
            return Err(Error::InvalidInput {
                reason: format!("{} is `{other}`: the one value it takes is `mock`", choice()),
            });
        }
    }
    let level = level.unwrap_or(Level::System);
    let backend: Arc<dyn Backend> = native(level)?;
    backend.probe()?;
    Ok(Manager::new(backend))
}

#[cfg(target_os = "linux")]
fn native(level: Level) -> Result<Arc<dyn Backend>, Error> {
    Ok(Arc::new(super::systemd::Systemd::native(level)))
}

#[cfg(target_os = "macos")]
fn native(level: Level) -> Result<Arc<dyn Backend>, Error> {
    Ok(Arc::new(super::launchd::Launchd::native(level)?))
}

#[cfg(target_os = "windows")]
fn native(level: Level) -> Result<Arc<dyn Backend>, Error> {
    Ok(Arc::new(super::windows::Windows::native(level)?))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn native(_level: Level) -> Result<Arc<dyn Backend>, Error> {
    Err(Error::Unavailable { reason: "this platform has no service manager this module drives".to_owned() })
}

pub(super) fn list(backend: &dyn Backend) -> Result<Vec<Service>, Error> {
    let mut services = backend.list()?;
    services.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(services)
}

pub(super) fn get(backend: &dyn Backend, label: &str) -> Result<Option<Service>, Error> {
    validate::label(label)?;
    backend.get(label)
}

/// The service, which the manager must have.
fn existing(backend: &dyn Backend, label: &str) -> Result<Service, Error> {
    validate::label(label)?;
    backend.get(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })
}

/// Refuse the options that the manager does not keep, naming the first one.
fn supported(
    capabilities: &Capabilities,
    supplied: &[&'static str],
    allowed: &[String],
    what: &str,
) -> Result<(), Error> {
    for option in supplied {
        if !allowed.iter().any(|a| a == option) {
            return Err(Error::Unsupported {
                option: Some((*option).to_owned()),
                reason: format!(
                    "the {} service manager does not keep the option \"{option}\" ({what})",
                    capabilities.kind
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn install(backend: &dyn Backend, spec: Spec) -> Result<Service, Error> {
    validate::spec(&spec)?;
    let capabilities = backend.capabilities();
    supported(&capabilities, &spec.supplied(), &capabilities.install_options, "install")?;
    if backend.get(&spec.label)?.is_some() {
        return Err(Error::AlreadyExists { label: spec.label });
    }
    backend.install(&spec)?;
    backend.get(&spec.label)?.ok_or_else(|| Error::Failed {
        operation: "install".to_owned(),
        reason: "the manager accepted the service but does not list it".to_owned(),
    })
}

pub(super) fn uninstall(backend: &dyn Backend, label: &str) -> Result<(), Error> {
    existing(backend, label)?;
    backend.uninstall(label)
}

pub(super) fn control(backend: &dyn Backend, label: &str, what: Control) -> Result<Service, Error> {
    existing(backend, label)?;
    match what {
        Control::Start => backend.start(label)?,
        Control::Stop => backend.stop(label)?,
        Control::Restart => backend.restart(label)?,
    }
    existing(backend, label)
}

pub(super) fn configure(backend: &dyn Backend, label: &str, patch: Patch) -> Result<Service, Error> {
    validate::label(label)?;
    validate::patch(&patch)?;
    let capabilities = backend.capabilities();
    supported(&capabilities, &patch.supplied(), &capabilities.configurable, "configure")?;
    existing(backend, label)?;
    backend.configure(label, &patch)?;
    existing(backend, label)
}
