//! The Windows Service Control Manager (system level).
//!
//! Discovery and inspection read `Win32_Service` through PowerShell (its property names do not depend on the
//! language of the machine). Registering, removing, starting and stopping go through the `service-manager` crate;
//! restarting is `Restart-Service`, which waits for the stop (a stop request alone answers before the service ended)
//! and never forces the services that depend on this one to stop: when some are running the answer is the native
//! dependency error.
//! The startup type and the description are changed with `sc.exe`. The program of a service must itself speak the
//! service dispatcher protocol; this module controls services and does not run a program as one.

use std::path::PathBuf;

use serde::Deserialize;
use service_manager::ServiceManagerKind;

use super::backend::{Backend, checked, crate_install, crate_start, crate_stop, crate_uninstall, run};
use super::service_manager::{Capabilities, Error, Level, Patch, Service, Spec, State};

const LIST: &str = "Get-CimInstance Win32_Service | Select-Object Name,Description,State,StartMode,PathName,StartName,ExitCode | ConvertTo-Json -Compress";
const GET: &str = "Get-CimInstance Win32_Service -Filter \"Name='$($env:NEOC_SERVICE)'\" | Select-Object Name,Description,State,StartMode,PathName,StartName,ExitCode | ConvertTo-Json -Compress";

pub(super) struct Windows {
    powershell: PathBuf,
    sc: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Cim {
    name: String,
    description: Option<String>,
    state: Option<String>,
    start_mode: Option<String>,
    path_name: Option<String>,
    start_name: Option<String>,
    exit_code: Option<i64>,
}

impl Windows {
    pub(super) fn native(level: Level) -> Result<Self, Error> {
        if level == Level::User {
            return Err(Error::Unsupported {
                option: Some("level".to_owned()),
                reason: "Windows has the system level only: services belong to the machine".to_owned(),
            });
        }
        Ok(Self { powershell: PathBuf::from("powershell.exe"), sc: PathBuf::from("sc.exe") })
    }

    fn query(&self, script: &str, label: Option<&str>) -> Result<Vec<Service>, Error> {
        let mut command = std::process::Command::new(&self.powershell);
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        if let Some(label) = label {
            command.env("NEOC_SERVICE", label);
        }
        let output = command
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| super::backend::io_error("powershell.exe", &e))?;
        let output = checked("list services", None, output)?;
        parse_cim(&String::from_utf8_lossy(&output.stdout))
    }
}

/// `ConvertTo-Json` writes one object for one service and an array for several; nothing for none.
fn parse_cim(text: &str) -> Result<Vec<Service>, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<Cim> = if text.starts_with('[') {
        serde_json::from_str(text)
    } else {
        serde_json::from_str::<Cim>(text).map(|one| vec![one])
    }
    .map_err(|e| Error::Failed {
        operation: "list services".to_owned(),
        reason: format!("unreadable answer: {e}"),
    })?;
    Ok(rows.into_iter().map(service_from).collect())
}

fn service_from(cim: Cim) -> Service {
    let state = match cim.state.as_deref() {
        Some("Running") => State::Running,
        Some("Stopped") => State::Stopped,
        Some("Start Pending" | "Continue Pending") => State::Starting,
        Some("Stop Pending") => State::Stopping,
        Some("Paused" | "Pause Pending") => State::Paused,
        _ => State::Unknown,
    };
    let failed = state == State::Stopped && cim.exit_code.is_some_and(|c| c != 0);
    Service {
        label: cim.name,
        description: cim.description.filter(|d| !d.is_empty()),
        state: if failed { State::Failed } else { state },
        state_detail: cim.exit_code.filter(|c| *c != 0).map(|c| format!("exit code {c}")),
        autostart: match cim.start_mode.as_deref() {
            Some("Auto") => Some(true),
            Some("Manual" | "Disabled") => Some(false),
            _ => None,
        },
        program: None,
        args: None,
        command_line: cim.path_name.filter(|p| !p.is_empty()),
        working_directory: None,
        environment: None,
        restart: None,
        restart_delay_secs: None,
        user: cim.start_name.filter(|u| !u.is_empty()),
        path: None,
    }
}

impl Backend for Windows {
    fn capabilities(&self) -> Capabilities {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Capabilities {
            kind: "windows".to_owned(),
            level: Level::System,
            install_options: names(&["args", "autostart", "user"]),
            configurable: names(&["autostart", "description"]),
            reported: names(&["description", "state", "state_detail", "autostart", "command_line", "user"]),
        }
    }

    fn probe(&self) -> Result<(), Error> {
        run(&self.sc, &["query", "state=", "inactive"]).map(|_| ())
    }

    fn list(&self) -> Result<Vec<Service>, Error> {
        self.query(LIST, None)
    }

    fn get(&self, label: &str) -> Result<Option<Service>, Error> {
        Ok(self.query(GET, Some(label))?.into_iter().next())
    }

    fn install(&self, spec: &Spec) -> Result<(), Error> {
        crate_install(ServiceManagerKind::Sc, Level::System, spec)
    }

    fn uninstall(&self, label: &str) -> Result<(), Error> {
        crate_uninstall(ServiceManagerKind::Sc, Level::System, label)
    }

    fn start(&self, label: &str) -> Result<(), Error> {
        crate_start(ServiceManagerKind::Sc, Level::System, label)
    }

    fn stop(&self, label: &str) -> Result<(), Error> {
        crate_stop(ServiceManagerKind::Sc, Level::System, label)
    }

    fn restart(&self, label: &str) -> Result<(), Error> {
        let mut command = std::process::Command::new(&self.powershell);
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Restart-Service -Name $env:NEOC_SERVICE -ErrorAction Stop",
            ])
            .env("NEOC_SERVICE", label)
            .stdin(std::process::Stdio::null());
        let output = command.output().map_err(|e| super::backend::io_error("powershell.exe", &e))?;
        checked("restart", Some(label), output).map(|_| ())
    }

    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error> {
        if let Some(autostart) = patch.autostart {
            let mode = if autostart { "auto" } else { "demand" };
            checked("configure", Some(label), run(&self.sc, &["config", label, "start=", mode])?)?;
        }
        if let Some(description) = &patch.description {
            checked("configure", Some(label), run(&self.sc, &["description", label, description])?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_service_and_several_are_both_read() {
        let one = r#"{"Name":"Spooler","Description":"Prints","State":"Running","StartMode":"Auto","PathName":"C:\\Windows\\spoolsv.exe","StartName":"LocalSystem","ExitCode":0}"#;
        let services = parse_cim(one).expect("one");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].state, State::Running);
        assert_eq!(services[0].autostart, Some(true));
        assert_eq!(services[0].command_line.as_deref(), Some("C:\\Windows\\spoolsv.exe"));
        let many = format!(
            "[{one},{{\"Name\":\"Bits\",\"State\":\"Stopped\",\"StartMode\":\"Disabled\",\"ExitCode\":1067}}]"
        );
        let services = parse_cim(&many).expect("many");
        assert_eq!(services[1].state, State::Failed);
        assert_eq!(services[1].autostart, Some(false));
        assert_eq!(services[1].state_detail.as_deref(), Some("exit code 1067"));
        assert!(parse_cim("").expect("none").is_empty());
    }
}
