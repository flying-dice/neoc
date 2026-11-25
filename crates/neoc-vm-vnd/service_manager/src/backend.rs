//! What every service manager of this module is behind: the [`Backend`] trait, the process helper the real ones
//! use, and how a failure of the operating system is told apart (refused, not there, failed).

use std::io;
use std::process::{Command, Output, Stdio};

use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceManagerKind,
    ServiceStartCtx, ServiceStopCtx, ServiceUninstallCtx,
};

use super::service_manager::{Capabilities, Error, Level, Patch, Restart, Service, Spec};

/// One service manager: the operating system's, or the in-memory one. Every call blocks (a real manager runs
/// programs), so the module runs them on the blocking pool. A label has been checked before it gets here.
pub(super) trait Backend: Send + Sync {
    /// What this manager can report and change.
    fn capabilities(&self) -> Capabilities;
    /// Whether the manager is there to be asked (`unavailable` when it is not).
    fn probe(&self) -> Result<(), Error>;
    /// Every service the manager has registered, whoever registered it.
    fn list(&self) -> Result<Vec<Service>, Error>;
    /// One service as the manager has it now, `None` when it has none by that label.
    fn get(&self, label: &str) -> Result<Option<Service>, Error>;
    /// Register a new service. The label is free (checked by the caller).
    fn install(&self, spec: &Spec) -> Result<(), Error>;
    /// Remove the registration of a service. The program it runs is left where it is.
    fn uninstall(&self, label: &str) -> Result<(), Error>;
    fn start(&self, label: &str) -> Result<(), Error>;
    fn stop(&self, label: &str) -> Result<(), Error>;
    fn restart(&self, label: &str) -> Result<(), Error>;
    /// Change what the manager keeps of a service, and nothing else of it.
    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error>;
}

/// Run a program with no shell, no input, and wait for it.
pub(super) fn run(program: &std::path::Path, args: &[&str]) -> Result<Output, Error> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| io_error(&program.display().to_string(), &e))
}

/// The text a failed command said, trimmed: its standard error, or its output when that was empty.
pub(super) fn said(output: &Output) -> String {
    let err = String::from_utf8_lossy(&output.stderr);
    let text = if err.trim().is_empty() { String::from_utf8_lossy(&output.stdout) } else { err };
    text.trim().to_owned()
}

/// Tell what a failed `operation` was: refused for want of rights, a service the manager does not have, or a
/// failure with the manager's own words.
pub(super) fn classify(operation: &str, label: Option<&str>, said: &str) -> Error {
    let lower = said.to_lowercase();
    let denied = [
        "access denied",
        "permission denied",
        "access is denied",
        "authentication is required",
        "interactive authentication",
        "not permitted",
        "must be root",
        "requires elevation",
        "requires administrator",
    ];
    if denied.iter().any(|p| lower.contains(p)) {
        return Error::PermissionDenied { operation: operation.to_owned(), reason: said.to_owned() };
    }
    let missing =
        ["not found", "not loaded", "could not be found", "does not exist", "no such unit", "cannot find"];
    if let Some(label) = label
        && missing.iter().any(|p| lower.contains(p))
    {
        return Error::NotFound { label: label.to_owned() };
    }
    Error::Failed {
        operation: operation.to_owned(),
        reason: if said.is_empty() { "it failed without saying why".to_owned() } else { said.to_owned() },
    }
}

/// A command's failure as an error, or its output when it succeeded.
pub(super) fn checked(operation: &str, label: Option<&str>, output: Output) -> Result<Output, Error> {
    if output.status.success() { Ok(output) } else { Err(classify(operation, label, &said(&output))) }
}

/// An operating-system error as one of this module's: refused, a program that is not there, or a failure.
pub(super) fn io_error(operation: &str, error: &io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::PermissionDenied => {
            Error::PermissionDenied { operation: operation.to_owned(), reason: error.to_string() }
        }
        io::ErrorKind::NotFound => Error::Unavailable { reason: format!("{operation}: {error}") },
        _ => classify(operation, None, &error.to_string()),
    }
}

// -- the `service-manager` crate, where it adds value: registering, removing, starting and stopping ----------------

fn crate_manager(kind: ServiceManagerKind, level: Level) -> Result<Box<dyn ServiceManager>, Error> {
    let mut manager = <dyn ServiceManager>::target(kind);
    let level = match level {
        Level::System => ServiceLevel::System,
        Level::User => ServiceLevel::User,
    };
    manager.set_level(level).map_err(|e| io_error("service_manager set_level", &e))?;
    Ok(manager)
}

fn crate_label(label: &str) -> ServiceLabel {
    // the label is the manager's own name for the service: kept whole, never split on its dots
    ServiceLabel { qualifier: None, organization: None, application: label.to_owned() }
}

pub(super) fn crate_install(kind: ServiceManagerKind, level: Level, spec: &Spec) -> Result<(), Error> {
    let manager = crate_manager(kind, level)?;
    let restart_policy = match (spec.restart, spec.restart_delay_secs) {
        (None, None) => RestartPolicy::default(),
        (Some(Restart::Never), _) => RestartPolicy::Never,
        (Some(Restart::Always), delay_secs) => RestartPolicy::Always { delay_secs },
        (Some(Restart::OnFailure) | None, delay_secs) => {
            RestartPolicy::OnFailure { delay_secs, max_retries: None, reset_after_secs: None }
        }
    };
    let ctx = ServiceInstallCtx {
        label: crate_label(&spec.label),
        program: spec.program.clone().into(),
        args: spec.args.clone().unwrap_or_default().into_iter().map(Into::into).collect(),
        contents: None,
        username: spec.user.clone(),
        working_directory: spec.working_directory.clone().map(Into::into),
        environment: spec.environment.clone().map(|env| env.into_iter().collect()),
        autostart: spec.autostart.unwrap_or(false),
        restart_policy,
    };
    manager.install(ctx).map_err(|e| io_error("install", &e))
}

pub(super) fn crate_uninstall(kind: ServiceManagerKind, level: Level, label: &str) -> Result<(), Error> {
    crate_manager(kind, level)?
        .uninstall(ServiceUninstallCtx { label: crate_label(label) })
        .map_err(|e| io_error("uninstall", &e))
}

pub(super) fn crate_start(kind: ServiceManagerKind, level: Level, label: &str) -> Result<(), Error> {
    crate_manager(kind, level)?
        .start(ServiceStartCtx { label: crate_label(label) })
        .map_err(|e| io_error("start", &e))
}

pub(super) fn crate_stop(kind: ServiceManagerKind, level: Level, label: &str) -> Result<(), Error> {
    crate_manager(kind, level)?
        .stop(ServiceStopCtx { label: crate_label(label) })
        .map_err(|e| io_error("stop", &e))
}
