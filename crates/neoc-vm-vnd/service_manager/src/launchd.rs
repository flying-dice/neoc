//! launchd (macOS): LaunchDaemons at the system level, the user's LaunchAgents at the user level.
//!
//! Discovery reads the property lists of the level's directory (the definitions) and asks `launchctl list` which are
//! loaded and running. Registering, removing, starting and stopping go through the `service-manager` crate. A change
//! rewrites only the keys it is about in the property list and keeps every other key as it was; it applies when the
//! service is next loaded, which is what `restart` does.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use plist::{Dictionary, Value};
use service_manager::ServiceManagerKind;

use super::backend::{
    Backend, checked, crate_install, crate_start, crate_stop, crate_uninstall, io_error, run,
};
use super::service_manager::{Capabilities, Error, Level, Patch, Restart, Service, Spec, State};

pub(super) struct Launchd {
    level: Level,
    dir: PathBuf,
    launchctl: PathBuf,
}

impl Launchd {
    pub(super) fn native(level: Level) -> Result<Self, Error> {
        let dir = match level {
            Level::System => PathBuf::from("/Library/LaunchDaemons"),
            Level::User => {
                let home =
                    std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or_else(|| Error::Unavailable {
                        reason: "the user level needs HOME to find the LaunchAgents".to_owned(),
                    })?;
                PathBuf::from(home).join("Library/LaunchAgents")
            }
        };
        Ok(Self { level, dir, launchctl: PathBuf::from("launchctl") })
    }

    /// Which services are loaded: label to (process id, last exit status).
    fn loaded(&self) -> Result<BTreeMap<String, (Option<u32>, i32)>, Error> {
        let output = checked("list loaded services", None, run(&self.launchctl, &["list"])?)?;
        Ok(parse_loaded(&String::from_utf8_lossy(&output.stdout)))
    }

    fn definitions(&self) -> Result<Vec<(PathBuf, Dictionary)>, Error> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error("list services", &e)),
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("plist") {
                continue;
            }
            // a property list that cannot be read (binary of another kind, damaged) is left out, not fatal
            if let Ok(Value::Dictionary(dict)) = Value::from_file(&path)
                && dict.get("Label").and_then(Value::as_string).is_some()
            {
                found.push((path, dict));
            }
        }
        Ok(found)
    }

    fn definition(&self, label: &str) -> Result<Option<(PathBuf, Dictionary)>, Error> {
        Ok(self
            .definitions()?
            .into_iter()
            .find(|(_, d)| d.get("Label").and_then(Value::as_string) == Some(label)))
    }
}

fn parse_loaded(text: &str) -> BTreeMap<String, (Option<u32>, i32)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let pid = cols.next()?.trim();
            let status = cols.next()?.trim();
            let label = cols.next()?.trim();
            Some((label.to_owned(), (pid.parse().ok(), status.parse().unwrap_or(0))))
        })
        .collect()
}

fn service_from(
    path: &std::path::Path,
    dict: &Dictionary,
    loaded: &BTreeMap<String, (Option<u32>, i32)>,
) -> Option<Service> {
    let label = dict.get("Label")?.as_string()?.to_owned();
    let arguments: Vec<String> = dict
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_string().map(str::to_owned)).collect())
        .unwrap_or_default();
    let program = dict
        .get("Program")
        .and_then(Value::as_string)
        .map(str::to_owned)
        .or_else(|| arguments.first().cloned());
    let args = if arguments.is_empty() { None } else { Some(arguments[1..].to_vec()) };
    let (state, detail) = match loaded.get(&label) {
        Some((Some(_), _)) => (State::Running, None),
        Some((None, status)) if *status != 0 => (State::Stopped, Some(format!("last exit status {status}"))),
        _ => (State::Stopped, None),
    };
    let restart = match dict.get("KeepAlive") {
        Some(Value::Boolean(true)) => Some(Restart::Always),
        Some(Value::Boolean(false)) | None => Some(Restart::Never),
        Some(Value::Dictionary(d)) if d.get("SuccessfulExit").and_then(Value::as_boolean) == Some(false) => {
            Some(Restart::OnFailure)
        }
        Some(_) => None,
    };
    Some(Service {
        label,
        description: None,
        state,
        state_detail: detail,
        autostart: Some(dict.get("RunAtLoad").and_then(Value::as_boolean).unwrap_or(false)),
        program,
        args,
        command_line: None,
        working_directory: dict.get("WorkingDirectory").and_then(Value::as_string).map(str::to_owned),
        environment: dict.get("EnvironmentVariables").and_then(Value::as_dictionary).map(|d| {
            d.iter().filter_map(|(k, v)| v.as_string().map(|v| (k.clone(), v.to_owned()))).collect()
        }),
        restart,
        restart_delay_secs: dict
            .get("ThrottleInterval")
            .and_then(Value::as_unsigned_integer)
            .and_then(|n| u32::try_from(n).ok()),
        user: dict.get("UserName").and_then(Value::as_string).map(str::to_owned),
        path: Some(path.display().to_string()),
    })
}

/// The dictionary with the patch's keys changed and every other key as it was.
fn patched(mut dict: Dictionary, patch: &Patch) -> Result<Dictionary, Error> {
    if let Some(v) = patch.autostart {
        dict.insert("RunAtLoad".to_owned(), Value::Boolean(v));
    }
    if patch.program.is_some() || patch.args.is_some() {
        let current: Vec<String> = dict
            .get("ProgramArguments")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_string().map(str::to_owned)).collect())
            .unwrap_or_default();
        let program = patch
            .program
            .clone()
            .or_else(|| current.first().cloned())
            .or_else(|| dict.get("Program").and_then(Value::as_string).map(str::to_owned))
            .ok_or_else(|| Error::InvalidInput {
                reason: "the service has no program: give `program` too".to_owned(),
            })?;
        let args = patch.args.clone().unwrap_or_else(|| current.iter().skip(1).cloned().collect());
        let mut all = vec![program];
        all.extend(args);
        dict.remove("Program");
        dict.insert(
            "ProgramArguments".to_owned(),
            Value::Array(all.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(v) = &patch.working_directory {
        dict.insert("WorkingDirectory".to_owned(), Value::String(v.clone()));
    }
    if let Some(env) = &patch.environment {
        let mut vars = Dictionary::new();
        for (k, v) in env {
            vars.insert(k.clone(), Value::String(v.clone()));
        }
        dict.insert("EnvironmentVariables".to_owned(), Value::Dictionary(vars));
    }
    if let Some(restart) = patch.restart {
        let keep = match restart {
            Restart::Never => Value::Boolean(false),
            Restart::Always => Value::Boolean(true),
            Restart::OnFailure => {
                let mut d = Dictionary::new();
                d.insert("SuccessfulExit".to_owned(), Value::Boolean(false));
                Value::Dictionary(d)
            }
        };
        dict.insert("KeepAlive".to_owned(), keep);
    }
    if let Some(delay) = patch.restart_delay_secs {
        dict.insert("ThrottleInterval".to_owned(), Value::Integer(i64::from(delay).into()));
    }
    Ok(dict)
}

impl Backend for Launchd {
    fn capabilities(&self) -> Capabilities {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Capabilities {
            kind: "launchd".to_owned(),
            level: self.level,
            install_options: names(&[
                "args",
                "working_directory",
                "environment",
                "autostart",
                "restart",
                "restart_delay_secs",
                "user",
            ]),
            configurable: names(&[
                "autostart",
                "program",
                "args",
                "working_directory",
                "environment",
                "restart",
                "restart_delay_secs",
            ]),
            reported: names(&[
                "state",
                "state_detail",
                "autostart",
                "program",
                "args",
                "working_directory",
                "environment",
                "restart",
                "restart_delay_secs",
                "user",
                "path",
            ]),
        }
    }

    fn probe(&self) -> Result<(), Error> {
        run(&self.launchctl, &["version"]).map(|_| ())
    }

    fn list(&self) -> Result<Vec<Service>, Error> {
        let loaded = self.loaded()?;
        Ok(self.definitions()?.iter().filter_map(|(path, dict)| service_from(path, dict, &loaded)).collect())
    }

    fn get(&self, label: &str) -> Result<Option<Service>, Error> {
        let loaded = self.loaded()?;
        Ok(self.definition(label)?.and_then(|(path, dict)| service_from(&path, &dict, &loaded)))
    }

    fn install(&self, spec: &Spec) -> Result<(), Error> {
        crate_install(ServiceManagerKind::Launchd, self.level, spec)
    }

    fn uninstall(&self, label: &str) -> Result<(), Error> {
        crate_uninstall(ServiceManagerKind::Launchd, self.level, label)
    }

    fn start(&self, label: &str) -> Result<(), Error> {
        crate_start(ServiceManagerKind::Launchd, self.level, label)
    }

    fn stop(&self, label: &str) -> Result<(), Error> {
        crate_stop(ServiceManagerKind::Launchd, self.level, label)
    }

    fn restart(&self, label: &str) -> Result<(), Error> {
        self.stop(label)?;
        self.start(label)
    }

    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error> {
        let (path, dict) =
            self.definition(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })?;
        let dict = patched(dict, patch)?;
        let dir = path
            .parent()
            .ok_or_else(|| Error::Other { reason: "a property list has a directory".to_owned() })?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_error("configure", &e))?;
        plist::to_writer_xml(&mut file, &Value::Dictionary(dict))
            .map_err(|e| Error::Failed { operation: "configure".to_owned(), reason: e.to_string() })?;
        file.flush().map_err(|e| io_error("configure", &e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o644))
                .map_err(|e| io_error("configure", &e))?;
        }
        file.persist(&path).map_err(|e| io_error("configure", &e.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(xml: &str) -> Dictionary {
        Value::from_reader_xml(xml.as_bytes()).expect("a plist").into_dictionary().expect("a dictionary")
    }

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>org.example.app</string>
<key>ProgramArguments</key><array><string>/usr/local/bin/app</string><string>--serve</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>StandardOutPath</key><string>/var/log/app.log</string>
<key>ThrottleInterval</key><integer>7</integer>
</dict></plist>"#;

    #[test]
    fn a_service_is_read_from_its_property_list_and_launchctl() {
        let loaded = parse_loaded("PID\tStatus\tLabel\n123\t0\torg.example.app\n-\t78\torg.example.other\n");
        let service =
            service_from(std::path::Path::new("/Library/LaunchDaemons/a.plist"), &dict(PLIST), &loaded)
                .expect("a service");
        assert_eq!(service.label, "org.example.app");
        assert_eq!(service.state, State::Running);
        assert_eq!(service.program.as_deref(), Some("/usr/local/bin/app"));
        assert_eq!(service.args, Some(vec!["--serve".to_owned()]));
        assert_eq!(service.autostart, Some(true));
        assert_eq!(service.restart, Some(Restart::OnFailure));
        assert_eq!(service.restart_delay_secs, Some(7));
        assert_eq!(service.description, None, "launchd has no description");
    }

    #[test]
    fn a_change_keeps_every_other_key_of_the_property_list() {
        let patch =
            Patch { autostart: Some(false), args: Some(vec!["--other".to_owned()]), ..Patch::default() };
        let changed = patched(dict(PLIST), &patch).expect("patched");
        assert_eq!(changed.get("RunAtLoad").and_then(Value::as_boolean), Some(false));
        let all = changed.get("ProgramArguments").and_then(Value::as_array).expect("array");
        assert_eq!(all.len(), 2);
        assert_eq!(
            all[0].as_string(),
            Some("/usr/local/bin/app"),
            "the program stays when only the arguments change"
        );
        assert_eq!(changed.get("StandardOutPath").and_then(Value::as_string), Some("/var/log/app.log"));
        assert!(changed.get("KeepAlive").is_some());
        assert_eq!(changed.get("ThrottleInterval").and_then(Value::as_unsigned_integer), Some(7));
    }
}
