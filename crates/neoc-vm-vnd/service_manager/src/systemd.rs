//! systemd (Linux), system or user level.
//!
//! Discovery and inspection ask `systemctl` (`list-unit-files`, `list-units`, `show`): every service unit the manager
//! knows, whoever wrote it. Start, stop, restart, enable and disable are `systemctl` commands. Registering and
//! removing a unit go through the `service-manager` crate. A change of a unit's settings is a drop-in
//! (`<unit>.service.d/90-vos.conf`), systemd's own way to change a unit without touching its file: what the unit's
//! file says and this module does not change is left exactly as it was.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::{Deserialize, Serialize};
use service_manager::ServiceManagerKind;

use super::backend::{Backend, checked, crate_install, crate_uninstall, io_error, run};
use super::service_manager::{Capabilities, Error, Level, Patch, Restart, Service, Spec, State};

const PROPERTIES: &str = "Id,Description,LoadState,ActiveState,SubState,UnitFileState,FragmentPath,WorkingDirectory,Environment,Restart,RestartUSec,User,ExecStart,Result";
const DROPIN: &str = "90-vos.conf";
const STATE_MARK: &str = "# neoc-state: ";
/// The key `parse_show` sets for a unit that runs several commands.
const MULTIPLE: &str = "ExecStart#several";

pub(super) struct Systemd {
    level: Level,
    systemctl: PathBuf,
    /// Where drop-ins are written: `/etc/systemd/system` or the user's `systemd/user` directory.
    dropins: PathBuf,
}

impl Systemd {
    pub(super) fn native(level: Level) -> Self {
        let dropins = match level {
            Level::System => PathBuf::from("/etc/systemd/system"),
            Level::User => config_home().join("systemd/user"),
        };
        Self { level, systemctl: PathBuf::from("systemctl"), dropins }
    }

    /// A manager that runs `systemctl` from another place and writes drop-ins elsewhere (the tests' fake).
    #[cfg(test)]
    pub(super) fn with(level: Level, systemctl: PathBuf, dropins: PathBuf) -> Self {
        Self { level, systemctl, dropins }
    }

    fn ctl(&self, args: &[&str]) -> Result<Output, Error> {
        let mut all: Vec<&str> = Vec::new();
        if self.level == Level::User {
            all.push("--user");
        }
        all.extend(["--no-pager", "--no-ask-password"]);
        all.extend_from_slice(args);
        run(&self.systemctl, &all)
    }

    fn ctl_ok(&self, operation: &str, label: Option<&str>, args: &[&str]) -> Result<String, Error> {
        let output = checked(operation, label, self.ctl(args)?)?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn unit_names(&self) -> Result<BTreeSet<String>, Error> {
        let files = self.ctl_ok(
            "list services",
            None,
            &["list-unit-files", "--type=service", "--no-legend", "--plain"],
        )?;
        let units = self.ctl_ok(
            "list services",
            None,
            &["list-units", "--type=service", "--all", "--no-legend", "--plain"],
        )?;
        Ok(unit_names(&files).into_iter().chain(unit_names(&units)).collect())
    }

    /// `systemctl` reads `*`, `?` and `[` in a unit name as a pattern and acts on every unit that matches, so a name
    /// that holds one is not a name: it is refused before `systemctl` sees it.
    fn exact(label: &str) -> Result<(), Error> {
        if label.contains(['*', '?', '[', ']']) {
            return Err(Error::InvalidInput {
                reason: format!(
                    "{label:?} holds a pattern character (`* ? [ ]`): a service is named exactly"
                ),
            });
        }
        Ok(())
    }

    /// The services of the units, each once. An alias answers with the `Id` of the unit it names, so two names can
    /// be one service: the first seen is kept.
    fn show(&self, labels: &[String]) -> Result<Vec<Service>, Error> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        for chunk in labels.chunks(100) {
            let property = format!("--property={PROPERTIES}");
            let units: Vec<String> = chunk.iter().map(|l| format!("{l}.service")).collect();
            let mut args: Vec<&str> = vec!["show", &property];
            args.extend(units.iter().map(String::as_str));
            let text = self.ctl_ok("inspect services", None, &args)?;
            found.extend(
                parse_show(&text)
                    .iter()
                    .filter_map(service_from_show)
                    .filter(|service| seen.insert(service.label.clone())),
            );
        }
        Ok(found)
    }

    fn unit(label: &str) -> String {
        format!("{label}.service")
    }

    fn dropin_path(&self, label: &str) -> PathBuf {
        self.dropins.join(format!("{label}.service.d")).join(DROPIN)
    }

    fn read_dropin(&self, label: &str) -> Result<DropIn, Error> {
        let path = self.dropin_path(label);
        match std::fs::read_to_string(&path) {
            Ok(text) => DropIn::parse_exact(&text).ok_or_else(|| Error::Failed {
                operation: "configure".to_owned(),
                reason: format!(
                    "{} was not written by this module or has been edited since: it is not replaced",
                    path.display()
                ),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DropIn::default()),
            Err(e) => Err(io_error("configure", &e)),
        }
    }

    /// Whether the drop-in of the unit is there and was written by this module. An unrelated file of the same name
    /// is not ours, and is never removed.
    pub(super) fn owns_dropin(&self, label: &str) -> Result<bool, Error> {
        match std::fs::read_to_string(self.dropin_path(label)) {
            Ok(text) => Ok(DropIn::parse_exact(&text).is_some()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error("uninstall", &e)),
        }
    }

    /// Remove the drop-in this module wrote, and its directory when nothing else is in it. A failure is reported.
    pub(super) fn remove_dropin(&self, label: &str) -> Result<(), Error> {
        let dropin = self.dropin_path(label);
        std::fs::remove_file(&dropin).map_err(|e| io_error("remove the drop-in of", &e))?;
        if let Some(dir) = dropin.parent() {
            match std::fs::remove_dir(dir) {
                Ok(()) => {}
                // the directory may hold other drop-ins: it stays
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(e) => return Err(io_error("remove the drop-in directory of", &e)),
            }
        }
        Ok(())
    }

    fn write_dropin(&self, label: &str, dropin: &DropIn) -> Result<(), Error> {
        let path = self.dropin_path(label);
        let dir =
            path.parent().ok_or_else(|| Error::Other { reason: "a drop-in has a directory".to_owned() })?;
        std::fs::create_dir_all(dir).map_err(|e| io_error("configure", &e))?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_error("configure", &e))?;
        file.write_all(dropin.render().as_bytes()).map_err(|e| io_error("configure", &e))?;
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

    /// Put a change of the command into the drop-in without changing what is not asked for. `systemctl show` prints
    /// the arguments joined by spaces, which cannot be told from an argument that holds one, and a drop-in replaces
    /// every `ExecStart=` of the unit. So a command is rewritten only when that loses nothing: it is given whole
    /// (program and arguments), or this module wrote it before, and the unit runs one command. Anything else is
    /// refused before a file is written.
    fn command_change(&self, label: &str, patch: &Patch, dropin: &mut DropIn) -> Result<(), Error> {
        let blocks = {
            let text = self.ctl_ok(
                "inspect",
                Some(label),
                &["show", &format!("--property={PROPERTIES}"), &Self::unit(label)],
            )?;
            parse_show(&text)
        };
        if blocks.first().is_some_and(|b| b.contains_key(MULTIPLE)) {
            return Err(Error::Unsupported {
                option: Some(if patch.program.is_some() { "program" } else { "args" }.to_owned()),
                reason: format!(
                    "{label:?} runs several commands (several `ExecStart=`): they cannot be replaced by one without losing the others"
                ),
            });
        }
        let given = patch.program.is_some() && patch.args.is_some();
        let kept = dropin.program.is_some() && dropin.args.is_some();
        if !given && !kept {
            return Err(Error::Unsupported {
                option: Some(if patch.program.is_some() { "program" } else { "args" }.to_owned()),
                reason: format!(
                    "the arguments of {label:?} are not known exactly (the service manager shows them joined by spaces): give `program` and `args` together"
                ),
            });
        }
        if let Some(program) = &patch.program {
            dropin.program = Some(program.clone());
        }
        if let Some(args) = &patch.args {
            dropin.args = Some(args.clone());
        }
        Ok(())
    }

    fn reload(&self) -> Result<(), Error> {
        self.ctl_ok("reload the manager", None, &["daemon-reload"]).map(|_| ())
    }
}

/// The unit names (without `.service`) of the lines of `list-unit-files` or `list-units`.
fn unit_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let first = line.split_whitespace().find(|word| *word != "●" && *word != "*")?;
            let name = first.strip_suffix(".service")?;
            // a template is not a service: only its instances are
            if name.ends_with('@') { None } else { Some(name.to_owned()) }
        })
        .collect()
}

/// The blocks of `systemctl show` (one per unit, blank line between) as properties.
fn parse_show(text: &str) -> Vec<BTreeMap<String, String>> {
    let mut blocks = Vec::new();
    let mut block = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
        } else if let Some((key, value)) = line.split_once('=') {
            if block.contains_key(key) {
                // a second `ExecStart=` (a oneshot with several commands) is kept as a mark, not as a value
                if key == "ExecStart" {
                    block.insert(MULTIPLE.to_owned(), "yes".to_owned());
                }
            } else {
                block.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    if !block.is_empty() {
        blocks.push(block);
    }
    blocks
}

fn state_of(active: &str) -> State {
    match active {
        "active" | "reloading" => State::Running,
        "inactive" => State::Stopped,
        "failed" => State::Failed,
        "activating" => State::Starting,
        "deactivating" => State::Stopping,
        _ => State::Unknown,
    }
}

fn restart_of(value: &str) -> Option<Restart> {
    match value {
        "no" => Some(Restart::Never),
        "always" => Some(Restart::Always),
        "on-failure" => Some(Restart::OnFailure),
        // `on-success`, `on-abnormal`, `on-abort` and `on-watchdog` have no name here
        _ => None,
    }
}

/// `100ms`, `5s`, `1min 30s` as whole seconds (rounded up), `None` for what is not that.
fn seconds(value: &str) -> Option<u32> {
    if value.trim() == "0" {
        return Some(0);
    }
    let mut micros: f64 = 0.0;
    for word in value.split_whitespace() {
        let split = word.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let (number, unit) = word.split_at(split);
        let n: f64 = number.parse().ok()?;
        let unit_micros = match unit {
            "us" => 1.0,
            "ms" => 1_000.0,
            "s" => 1_000_000.0,
            "min" => 60_000_000.0,
            "h" => 3_600_000_000.0,
            "d" => 86_400_000_000.0,
            _ => return None,
        };
        micros += n * unit_micros;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((micros / 1_000_000.0).ceil() as u32)
}

/// The program and the arguments of `ExecStart={ path=... ; argv[]=... ; ... }`.
fn exec_start(value: &str) -> (Option<String>, Option<Vec<String>>) {
    let field = |name: &str| {
        let start = value.find(name)? + name.len();
        let rest = &value[start..];
        let end = rest.find(" ; ").or_else(|| rest.find(" }")).unwrap_or(rest.len());
        Some(rest[..end].to_owned())
    };
    let program = field("path=").filter(|p| !p.is_empty());
    let args = field("argv[]=").map(|argv| argv.split_whitespace().skip(1).map(str::to_owned).collect());
    (program, args)
}

/// A service as `systemctl show` describes it; `None` for a unit the manager does not have.
fn service_from_show(block: &BTreeMap<String, String>) -> Option<Service> {
    let get = |key: &str| block.get(key).map(String::as_str).filter(|v| !v.is_empty());
    if get("LoadState") == Some("not-found") {
        return None;
    }
    let label = get("Id")?.strip_suffix(".service")?.to_owned();
    let (program, args) = get("ExecStart").map(exec_start).unwrap_or((None, None));
    let environment = get("Environment").and_then(|v| {
        let map: BTreeMap<String, String> = shlex::split(v)?
            .into_iter()
            .filter_map(|pair| pair.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
            .collect();
        Some(map)
    });
    let result = get("Result").filter(|r| *r != "success");
    let sub = get("SubState");
    Some(Service {
        label,
        description: get("Description").map(str::to_owned),
        state: state_of(get("ActiveState").unwrap_or("")),
        state_detail: result.or(sub).map(str::to_owned),
        autostart: match get("UnitFileState") {
            Some("enabled" | "enabled-runtime") => Some(true),
            Some("disabled") => Some(false),
            // static, indirect, generated, transient, masked, alias: there is nothing to enable
            _ => None,
        },
        program,
        args,
        command_line: None,
        working_directory: get("WorkingDirectory").map(str::to_owned),
        environment,
        restart: get("Restart").and_then(restart_of),
        restart_delay_secs: get("RestartUSec").and_then(seconds),
        user: get("User").map(str::to_owned),
        path: get("FragmentPath").map(str::to_owned),
    })
}

fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
}

/// The settings this module has changed of one unit, kept in the drop-in itself (as JSON in a comment) so the next
/// change adds to them and does not forget them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DropIn {
    description: Option<String>,
    program: Option<String>,
    args: Option<Vec<String>>,
    working_directory: Option<String>,
    environment: Option<BTreeMap<String, String>>,
    restart: Option<Restart>,
    restart_delay_secs: Option<u32>,
}

/// An argument of `ExecStart=` as systemd reads it: quoted, with `$` and `%` doubled (they are specifiers).
fn quote_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("$$"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A value of `Environment=`: `"NAME=value"`, with `%` doubled.
fn quote_env(name: &str, value: &str) -> String {
    let mut out = String::from("\"");
    out.push_str(name);
    out.push('=');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A path as a value of `WorkingDirectory=`: `%` doubled. Paths are checked to hold no line break.
fn escape_value(value: &str) -> String {
    value.replace('%', "%%")
}

impl DropIn {
    fn parse(text: &str) -> Option<Self> {
        let json = text.lines().find_map(|line| line.strip_prefix(STATE_MARK))?;
        serde_json::from_str(json).ok()
    }

    /// The state of a drop-in whose directives are still exactly what this module wrote for that state. A file that
    /// carries the mark but was edited since is not read as ours: regenerating it from the mark would discard the edit.
    fn parse_exact(text: &str) -> Option<Self> {
        let parsed = Self::parse(text)?;
        (parsed.render() == text).then_some(parsed)
    }

    fn render(&self) -> String {
        let mut out =
            String::from("# Settings changed through vos. The unit's own file is left as it was.\n");
        out.push_str(STATE_MARK);
        out.push_str(&serde_json::to_string(self).unwrap_or_default());
        out.push('\n');
        if let Some(description) = &self.description {
            out.push_str("[Unit]\nDescription=");
            out.push_str(&escape_value(description));
            out.push('\n');
        }
        let mut service = String::new();
        if let (Some(program), Some(args)) = (&self.program, &self.args) {
            service.push_str("ExecStart=\nExecStart=");
            service.push_str(&quote_arg(program));
            for arg in args {
                service.push(' ');
                service.push_str(&quote_arg(arg));
            }
            service.push('\n');
        }
        if let Some(dir) = &self.working_directory {
            service.push_str("WorkingDirectory=");
            service.push_str(&escape_value(dir));
            service.push('\n');
        }
        if let Some(env) = &self.environment {
            service.push_str("Environment=\n");
            for (name, value) in env {
                service.push_str("Environment=");
                service.push_str(&quote_env(name, value));
                service.push('\n');
            }
        }
        if let Some(restart) = self.restart {
            service.push_str(match restart {
                Restart::Never => "Restart=no\n",
                Restart::Always => "Restart=always\n",
                Restart::OnFailure => "Restart=on-failure\n",
            });
        }
        if let Some(delay) = self.restart_delay_secs {
            service.push_str(&format!("RestartSec={delay}\n"));
        }
        if !service.is_empty() {
            out.push_str("[Service]\n");
            out.push_str(&service);
        }
        out
    }
}

impl Backend for Systemd {
    fn capabilities(&self) -> Capabilities {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        Capabilities {
            kind: "systemd".to_owned(),
            level: self.level,
            install_options: names(&[
                "args",
                "working_directory",
                "environment",
                "autostart",
                "restart",
                "restart_delay_secs",
                "user",
                "description",
            ]),
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
                "user",
                "path",
            ]),
        }
    }

    fn probe(&self) -> Result<(), Error> {
        let output = self.ctl(&["is-system-running"])?;
        // "offline" is what systemctl says on a machine that was not booted with systemd
        if String::from_utf8_lossy(&output.stdout).trim() == "offline" {
            return Err(Error::Unavailable { reason: "systemd is not running on this machine".to_owned() });
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<Service>, Error> {
        let names: Vec<String> = self.unit_names()?.into_iter().collect();
        self.show(&names)
    }

    fn get(&self, label: &str) -> Result<Option<Service>, Error> {
        Self::exact(label)?;
        // the unit that answers must be the one that was asked for
        Ok(self.show(&[label.to_owned()])?.into_iter().find(|s| s.label == label))
    }

    fn install(&self, spec: &Spec) -> Result<(), Error> {
        let kind = ServiceManagerKind::Systemd;
        crate_install(kind, self.level, spec)?;
        if let Some(description) = &spec.description {
            let patch = Patch { description: Some(description.clone()), ..Patch::default() };
            if let Err(e) = self.configure(&spec.label, &patch) {
                // a service registered without what was asked of it is not left behind
                let _ = crate_uninstall(kind, self.level, &spec.label);
                return Err(e);
            }
        }
        self.reload()
    }

    fn uninstall(&self, label: &str) -> Result<(), Error> {
        let service = self.get(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })?;
        let own = self.dropins.clone();
        let removable = service.path.as_deref().is_some_and(|p| Path::new(p).parent() == Some(own.as_path()));
        if !removable {
            return Err(Error::Unsupported {
                option: None,
                reason: format!(
                    "the definition of {label:?} ({}) is part of the system, not one made for this machine: it can be stopped and disabled, not removed",
                    service.path.as_deref().unwrap_or("not a file")
                ),
            });
        }
        let ours = self.owns_dropin(label)?;
        let _ = self.ctl(&["stop", &Self::unit(label)]);
        crate_uninstall(ServiceManagerKind::Systemd, self.level, label)?;
        if ours {
            self.remove_dropin(label)?;
        }
        self.reload()
    }

    fn start(&self, label: &str) -> Result<(), Error> {
        Self::exact(label)?;
        self.ctl_ok("start", Some(label), &["start", &Self::unit(label)]).map(|_| ())
    }

    fn stop(&self, label: &str) -> Result<(), Error> {
        Self::exact(label)?;
        self.ctl_ok("stop", Some(label), &["stop", &Self::unit(label)]).map(|_| ())
    }

    fn restart(&self, label: &str) -> Result<(), Error> {
        Self::exact(label)?;
        self.ctl_ok("restart", Some(label), &["restart", &Self::unit(label)]).map(|_| ())
    }

    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error> {
        Self::exact(label)?;
        self.get(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })?;
        let unit = Self::unit(label);
        let mut dropin = self.read_dropin(label)?;
        let mut changed = false;
        if let Some(description) = &patch.description {
            dropin.description = Some(description.clone());
            changed = true;
        }
        if patch.program.is_some() || patch.args.is_some() {
            self.command_change(label, patch, &mut dropin)?;
            changed = true;
        }
        if let Some(dir) = &patch.working_directory {
            dropin.working_directory = Some(dir.clone());
            changed = true;
        }
        if let Some(env) = &patch.environment {
            dropin.environment = Some(env.clone());
            changed = true;
        }
        if let Some(restart) = patch.restart {
            dropin.restart = Some(restart);
            changed = true;
        }
        if let Some(delay) = patch.restart_delay_secs {
            dropin.restart_delay_secs = Some(delay);
            changed = true;
        }
        if changed {
            self.write_dropin(label, &dropin)?;
            self.reload()?;
        }
        if let Some(autostart) = patch.autostart {
            let verb = if autostart { "enable" } else { "disable" };
            self.ctl_ok(verb, Some(label), &[verb, &unit])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = "Id=ssh.service\nDescription=OpenBSD Secure Shell server\nLoadState=loaded\nActiveState=active\nSubState=running\nUnitFileState=enabled\nFragmentPath=/usr/lib/systemd/system/ssh.service\nWorkingDirectory=\nEnvironment=\nRestart=on-failure\nRestartUSec=5s\nUser=\nExecStart={ path=/usr/sbin/sshd ; argv[]=/usr/sbin/sshd -D -e ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\nResult=success\n\nId=ghost.service\nLoadState=not-found\nActiveState=inactive\n\nId=job.service\nDescription=A job\nLoadState=loaded\nActiveState=failed\nSubState=failed\nUnitFileState=static\nRestart=no\nRestartUSec=1min 30s\nEnvironment=\"A=1 2\" B=3\nResult=exit-code\n";

    #[test]
    fn a_service_is_read_from_what_systemctl_shows() {
        let services: Vec<Service> = parse_show(SHOW).iter().filter_map(service_from_show).collect();
        assert_eq!(services.len(), 2, "a unit that is not found is not a service");
        let ssh = &services[0];
        assert_eq!(ssh.label, "ssh");
        assert_eq!(ssh.state, State::Running);
        assert_eq!(ssh.autostart, Some(true));
        assert_eq!(ssh.program.as_deref(), Some("/usr/sbin/sshd"));
        assert_eq!(ssh.args.as_deref(), Some(&["-D".to_owned(), "-e".to_owned()][..]));
        assert_eq!(ssh.restart, Some(Restart::OnFailure));
        assert_eq!(ssh.restart_delay_secs, Some(5));
        assert_eq!(ssh.working_directory, None);
        let job = &services[1];
        assert_eq!(job.state, State::Failed);
        assert_eq!(job.state_detail.as_deref(), Some("exit-code"));
        assert_eq!(job.autostart, None, "a static unit has nothing to enable");
        assert_eq!(job.restart, Some(Restart::Never));
        assert_eq!(job.restart_delay_secs, Some(90));
        let env = job.environment.as_ref().expect("variables");
        assert_eq!(env.get("A").map(String::as_str), Some("1 2"));
        assert_eq!(env.get("B").map(String::as_str), Some("3"));
    }

    #[test]
    fn unit_names_skip_templates_and_the_bullet() {
        let files = "ssh.service enabled enabled\ngetty@.service enabled enabled\nbackup@home.service disabled enabled\ncron.timer enabled enabled\n";
        assert_eq!(unit_names(files), ["ssh", "backup@home"]);
        let units =
            "● failed.service loaded failed failed A failed one\n  ok.service loaded active running Fine\n";
        assert_eq!(unit_names(units), ["failed", "ok"]);
    }

    #[test]
    fn a_drop_in_is_rendered_quoted_and_read_back() {
        let dropin = DropIn {
            description: Some("Does 100% of it".to_owned()),
            program: Some("/opt/app/run".to_owned()),
            args: Some(vec!["--name".to_owned(), "a \"b\" $HOME".to_owned()]),
            working_directory: Some("/opt/app".to_owned()),
            environment: Some(BTreeMap::from([("K".to_owned(), "v 50%".to_owned())])),
            restart: Some(Restart::Always),
            restart_delay_secs: Some(3),
        };
        let text = dropin.render();
        assert!(text.contains("Description=Does 100%% of it\n"));
        assert!(text.contains("ExecStart=\nExecStart=\"/opt/app/run\" \"--name\" \"a \\\"b\\\" $$HOME\"\n"));
        assert!(text.contains("Environment=\nEnvironment=\"K=v 50%%\"\n"));
        assert!(text.contains("Restart=always\nRestartSec=3\n"));
        assert_eq!(DropIn::parse(&text), Some(dropin));
        assert_eq!(
            DropIn::parse("[Service]\nRestart=no\n"),
            None,
            "a file that is not ours is not read as ours"
        );
    }

    #[test]
    fn durations_are_whole_seconds() {
        assert_eq!(seconds("100ms"), Some(1));
        assert_eq!(seconds("1min 30s"), Some(90));
        assert_eq!(seconds("0"), Some(0));
        assert_eq!(seconds("infinity"), None);
    }
}
