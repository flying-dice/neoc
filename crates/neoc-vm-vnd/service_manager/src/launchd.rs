//! launchd (macOS): LaunchDaemons at the system level, the user's LaunchAgents at the user level.
//!
//! Each level has a launchd *domain* (`system`, or `gui/<uid>` for the user) and the directories whose property lists
//! define the jobs that load into it. Discovery reads those directories (the definitions) and asks the domain itself
//! (`launchctl print <domain>`) which jobs are loaded and running, including loaded jobs that have no property list;
//! when the domain cannot be asked the state is `unknown`, never a guess. Registering, removing, starting and stopping
//! go through the `service-manager` crate. A change rewrites only the keys it is about in the property list (staged
//! beside it and moved over it whole, with its mode and owner kept), and applies when the service is next loaded, which is what `restart` does.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use plist::{Dictionary, Value};
use service_manager::ServiceManagerKind;

use super::backend::{Backend, checked, crate_install, crate_uninstall, io_error, run, said};
use super::service_manager::{Capabilities, Error, Level, Patch, Restart, Service, Spec, State};

/// Label to (process id, last exit status) of the jobs a domain has loaded.
type Loaded = BTreeMap<String, (Option<u32>, i32)>;

pub(super) struct Launchd {
    level: Level,
    /// Where this module writes the property lists it registers.
    dir: PathBuf,
    /// Every directory whose property lists define jobs of the domain (the first one that has a label wins).
    dirs: Vec<PathBuf>,
    /// `system`, or `gui/<uid>`.
    domain: String,
    launchctl: PathBuf,
}

impl Launchd {
    pub(super) fn native(level: Level) -> Result<Self, Error> {
        let (dir, dirs, domain) = match level {
            Level::System => (
                PathBuf::from("/Library/LaunchDaemons"),
                vec![PathBuf::from("/Library/LaunchDaemons"), PathBuf::from("/System/Library/LaunchDaemons")],
                "system".to_owned(),
            ),
            Level::User => {
                let home =
                    std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or_else(|| Error::Unavailable {
                        reason: "the user level needs HOME to find the LaunchAgents".to_owned(),
                    })?;
                let own = PathBuf::from(home).join("Library/LaunchAgents");
                let uid = checked("find the user", None, run(Path::new("id"), &["-u"])?)?;
                let uid = String::from_utf8_lossy(&uid.stdout).trim().to_owned();
                (
                    own.clone(),
                    vec![
                        own,
                        PathBuf::from("/Library/LaunchAgents"),
                        PathBuf::from("/System/Library/LaunchAgents"),
                    ],
                    format!("gui/{uid}"),
                )
            }
        };
        Ok(Self { level, dir, dirs, domain, launchctl: PathBuf::from("launchctl") })
    }

    /// The jobs the domain has loaded, or `None` when it cannot be asked (no right to, an older `launchctl`).
    fn loaded(&self) -> Result<Option<Loaded>, Error> {
        let output = run(&self.launchctl, &["print", &self.domain])?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(parse_print(&String::from_utf8_lossy(&output.stdout))))
    }

    fn definitions(&self) -> Result<Vec<(PathBuf, Dictionary)>, Error> {
        let mut found: Vec<(PathBuf, Dictionary)> = Vec::new();
        for dir in &self.dirs {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error("list services", &e)),
            };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if path.extension().and_then(|e| e.to_str()) != Some("plist") {
                    continue;
                }
                // a property list that cannot be read (binary of another kind, damaged) is left out, not fatal
                if let Ok(Value::Dictionary(dict)) = Value::from_file(&path)
                    && let Some(label) = dict.get("Label").and_then(Value::as_string)
                    && !found.iter().any(|(_, d)| d.get("Label").and_then(Value::as_string) == Some(label))
                {
                    found.push((path, dict));
                }
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

    /// A job the domain has loaded that no property list in the directories defines.
    fn loaded_only(label: &str, loaded: Option<&Loaded>) -> Option<Service> {
        let (pid, status) = loaded?.get(label)?;
        Some(Service {
            label: label.to_owned(),
            description: None,
            state: if pid.is_some() { State::Running } else { State::Stopped },
            state_detail: exit_detail(*pid, *status),
            autostart: None,
            program: None,
            args: None,
            command_line: None,
            working_directory: None,
            environment: None,
            restart: None,
            restart_delay_secs: None,
            user: None,
            path: None,
        })
    }
}

fn exit_detail(pid: Option<u32>, status: i32) -> Option<String> {
    if pid.is_none() && status != 0 { Some(format!("last exit status {status}")) } else { None }
}

/// The `services = { ... }` block of `launchctl print <domain>`: one `pid status label` line a job (`-` for no pid).
fn parse_print(text: &str) -> Loaded {
    let mut jobs = Loaded::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if !inside {
            inside = line == "services = {";
            continue;
        }
        if line == "}" {
            break;
        }
        let mut cols = line.split_whitespace();
        let (Some(pid), Some(status)) = (cols.next(), cols.next()) else { continue };
        let label = cols.collect::<Vec<_>>().join(" ");
        if label.is_empty() {
            continue;
        }
        jobs.insert(label, (pid.parse().ok(), status.parse().unwrap_or(0)));
    }
    jobs
}

fn service_from(path: &std::path::Path, dict: &Dictionary, loaded: Option<&Loaded>) -> Option<Service> {
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
    let (state, detail) = match loaded {
        // the domain could not be asked: what is known is the definition, not whether it runs
        None => (State::Unknown, None),
        Some(jobs) => match jobs.get(&label) {
            Some((pid, status)) => {
                (if pid.is_some() { State::Running } else { State::Stopped }, exit_detail(*pid, *status))
            }
            None => (State::Stopped, None),
        },
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
        // `Program` is the executable when it is there and `ProgramArguments[0]` is then only argv[0] (the name the
        // program is told it has); without `Program` the first of `ProgramArguments` is the executable. Both keep
        // their own meaning.
        let current: Vec<String> = dict
            .get("ProgramArguments")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_string().map(str::to_owned)).collect())
            .unwrap_or_default();
        let separate = dict.get("Program").and_then(Value::as_string).map(str::to_owned);
        let no_program =
            || Error::InvalidInput { reason: "the service has no program: give `program` too".to_owned() };
        let all = if let Some(separate) = separate {
            if let Some(program) = &patch.program {
                dict.insert("Program".to_owned(), Value::String(program.clone()));
            }
            patch.args.as_ref().map(|args| {
                let argv0 = current.first().cloned().unwrap_or(separate);
                std::iter::once(argv0).chain(args.iter().cloned()).collect::<Vec<_>>()
            })
        } else {
            let program =
                patch.program.clone().or_else(|| current.first().cloned()).ok_or_else(no_program)?;
            let args = patch.args.clone().unwrap_or_else(|| current.iter().skip(1).cloned().collect());
            Some(std::iter::once(program).chain(args).collect::<Vec<_>>())
        };
        if let Some(all) = all {
            dict.insert(
                "ProgramArguments".to_owned(),
                Value::Array(all.into_iter().map(Value::String).collect()),
            );
        }
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
        let mut services: Vec<Service> = self
            .definitions()?
            .iter()
            .filter_map(|(path, dict)| service_from(path, dict, loaded.as_ref()))
            .collect();
        if let Some(jobs) = &loaded {
            for label in jobs.keys() {
                if !services.iter().any(|s| &s.label == label)
                    && let Some(job) = Self::loaded_only(label, loaded.as_ref())
                {
                    services.push(job);
                }
            }
        }
        Ok(services)
    }

    fn get(&self, label: &str) -> Result<Option<Service>, Error> {
        let loaded = self.loaded()?;
        Ok(match self.definition(label)? {
            Some((path, dict)) => service_from(&path, &dict, loaded.as_ref()),
            None => Self::loaded_only(label, loaded.as_ref()),
        })
    }

    fn install(&self, spec: &Spec) -> Result<(), Error> {
        crate_install(ServiceManagerKind::Launchd, self.level, spec)?;
        // launchd refuses to load a property list that anyone but its owner can write (`Bootstrap failed: 5`), and
        // the file is created with the caller's umask: set the mode it must have, and for a daemon the owner
        #[cfg(unix)]
        {
            let path = self.dir.join(format!("{}.plist", spec.label));
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                    .map_err(|e| io_error("set the mode of the property list", &e))?;
            }
            if self.level == Level::System {
                std::os::unix::fs::chown(&path, Some(0), Some(0))
                    .map_err(|e| io_error("set the owner of the property list", &e))?;
            }
        }
        Ok(())
    }

    fn uninstall(&self, label: &str) -> Result<(), Error> {
        let service = self.get(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })?;
        let own = service.path.as_deref().is_some_and(|p| Path::new(p).parent() == Some(self.dir.as_path()));
        if !own {
            return Err(Error::Unsupported {
                option: None,
                reason: format!(
                    "{label:?} ({}) is not a property list in {}, where services are registered: it can be stopped, not removed",
                    service.path.as_deref().unwrap_or("loaded without a property list"),
                    self.dir.display()
                ),
            });
        }
        crate_uninstall(ServiceManagerKind::Launchd, self.level, label)
    }

    /// Load the job into its domain if it is not there (the property list is read now, so a change made since is
    /// applied) and make sure it runs.
    fn start(&self, label: &str) -> Result<(), Error> {
        let (path, _) = self.definition(label)?.ok_or_else(|| Error::NotFound { label: label.to_owned() })?;
        let target = format!("{}/{label}", self.domain);
        let is_loaded = |loaded: Option<Loaded>| loaded.is_some_and(|jobs| jobs.contains_key(label));
        if !is_loaded(self.loaded()?) {
            let output = run(&self.launchctl, &["bootstrap", &self.domain, &path.display().to_string()])?;
            // another caller may have loaded it between the question and the request
            if !output.status.success() && !is_loaded(self.loaded()?) {
                // DIAGNOSTIC (temporary): what launchd saw
                let p = path.display().to_string();
                let more = ["ls -leO@d", "plutil -p", "id"].iter().map(|c| {
                    let mut it = c.split(' ');
                    let bin = it.next().unwrap_or_default();
                    let mut args: Vec<&str> = it.collect();
                    if bin != "id" { args.push(&p); }
                    std::process::Command::new(bin).args(args).output().map(|o| String::from_utf8_lossy(&[o.stdout, o.stderr].concat()).into_owned()).unwrap_or_default()
                }).collect::<Vec<_>>().join(" | ");
                return Err(super::backend::classify("start", Some(label), &format!("{} [{more}]", said(&output))));
            }
        }
        for _ in 0..20 {
            if self.loaded()?.is_some_and(|jobs| jobs.get(label).is_some_and(|(pid, _)| pid.is_some())) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // loaded and not running (it does not start at load): ask for it
        checked("start", Some(label), run(&self.launchctl, &["kickstart", &target])?).map(|_| ())
    }

    /// Unload the job from its domain: its process ends and `KeepAlive` cannot bring it back (`launchctl stop` only
    /// ends the process, which a job that is to be kept alive replaces at once). A job that is not loaded is stopped.
    fn stop(&self, label: &str) -> Result<(), Error> {
        let target = format!("{}/{label}", self.domain);
        let output = run(&self.launchctl, &["bootout", &target])?;
        if output.status.success() {
            return Ok(());
        }
        let text = said(&output);
        let lower = text.to_lowercase();
        if lower.contains("no such process")
            || lower.contains("could not find")
            || lower.contains("not found")
        {
            return Ok(());
        }
        Err(super::backend::classify("stop", Some(label), &text))
    }

    fn restart(&self, label: &str) -> Result<(), Error> {
        self.stop(label)?;
        self.start(label)
    }

    fn configure(&self, label: &str, patch: &Patch) -> Result<(), Error> {
        let (path, dict) = self.definition(label)?.ok_or_else(|| {
            // loaded without a property list, there is nothing to change
            Error::NotFound { label: label.to_owned() }
        })?;
        if path.starts_with("/System") {
            return Err(Error::Unsupported {
                option: None,
                reason: format!(
                    "{label:?} ({}) is part of the system: its definition is not changed",
                    path.display()
                ),
            });
        }
        let dict = patched(dict, patch)?;
        let mut bytes = Vec::new();
        plist::to_writer_xml(&mut bytes, &Value::Dictionary(dict))
            .map_err(|e| Error::Failed { operation: "configure".to_owned(), reason: e.to_string() })?;
        rewrite(&path, &bytes)
    }
}

/// Replace the contents of a property list and nothing else about it. The complete new file is staged beside the old
/// one, given the old one's mode, owner and group, and only then moved over it in one step: until that step the old
/// file is untouched, and a failure at any point leaves it byte for byte as it was. What cannot be given to the new
/// file (an owner this process may not hand out, an ACL or extended attributes) refuses the change before anything
/// is written.
fn rewrite(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    replace(path, bytes, |staged, target| staged.persist(target).map(|_| ()).map_err(|e| e.error))
}

/// [`rewrite`] with the final step given, so a failure of it can be tried.
fn replace(
    path: &Path,
    bytes: &[u8],
    commit: impl FnOnce(tempfile::NamedTempFile, &Path) -> std::io::Result<()>,
) -> Result<(), Error> {
    let dir =
        path.parent().ok_or_else(|| Error::Other { reason: "a property list has a directory".to_owned() })?;
    let original = std::fs::metadata(path).map_err(|e| io_error("configure", &e))?;
    refuse_extended_metadata(path)?;
    let mut staged = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_error("configure", &e))?;
    staged.write_all(bytes).map_err(|e| io_error("configure", &e))?;
    staged.flush().map_err(|e| io_error("configure", &e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::os::unix::fs::chown(staged.path(), Some(original.uid()), Some(original.gid())).map_err(|e| {
            Error::Unsupported {
                option: None,
                reason: format!("the owner of {} cannot be kept on a rewritten file ({e})", path.display()),
            }
        })?;
    }
    staged.as_file().set_permissions(original.permissions()).map_err(|e| io_error("configure", &e))?;
    staged.as_file().sync_all().map_err(|e| io_error("configure", &e))?;
    // the staged file is removed by its own drop when this fails: nothing is left beside the original
    commit(staged, path).map_err(|e| io_error("configure", &e))
}

/// An ACL or an extended attribute lives beside the contents and a new file would not have it: say so, do not drop it.
#[cfg(target_os = "macos")]
fn refuse_extended_metadata(path: &Path) -> Result<(), Error> {
    let output = run(Path::new("ls"), &["-ldeO@", &path.display().to_string()])?;
    if has_extended_metadata(&String::from_utf8_lossy(&output.stdout)) {
        return Err(Error::Unsupported {
            option: None,
            reason: format!(
                "{} has an ACL or extended attributes that a rewritten file would lose: it is not changed",
                path.display()
            ),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn refuse_extended_metadata(_path: &Path) -> Result<(), Error> {
    Ok(())
}

/// Whether `ls -ldeO@` printed anything after the file's own line: an ACL entry (` 0: user:...`) or an attribute
/// (a tab, its name and size). The `@` after the mode is a hint that there is something, not the evidence.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn has_extended_metadata(listing: &str) -> bool {
    listing.lines().filter(|line| !line.trim().is_empty()).count() > 1
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

    const PRINT: &str = "system = {\n\ttype = system\n\tservices = {\n\t\t   123      0 org.example.app\n\t\t     -     78 org.example.other\n\t\t     -      0 com.apple.loaded.without.plist\n\t}\n\n\tunrelated = {\n\t\t1 2 x\n\t}\n}\n";

    #[test]
    fn the_domain_is_asked_for_its_jobs_and_a_job_without_a_property_list_is_listed() {
        let loaded = parse_print(PRINT);
        assert_eq!(loaded.len(), 3, "only the services block is read");
        assert_eq!(loaded["org.example.app"], (Some(123), 0));
        assert_eq!(loaded["org.example.other"], (None, 78));
        let bare = Launchd::loaded_only("org.example.other", Some(&loaded)).expect("a job");
        assert_eq!(bare.state, State::Stopped);
        assert_eq!(bare.state_detail.as_deref(), Some("last exit status 78"));
        assert_eq!(bare.path, None);
        assert!(Launchd::loaded_only("org.example.nothing", Some(&loaded)).is_none());
        // a domain that cannot be asked says nothing about running
        let unknown = service_from(std::path::Path::new("/a.plist"), &dict(PLIST), None).expect("a service");
        assert_eq!(unknown.state, State::Unknown);
    }

    #[test]
    fn a_program_and_its_argv0_keep_their_own_meaning() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>org.example.named</string>
<key>Program</key><string>/usr/bin/example</string>
<key>ProgramArguments</key><array><string>custom-name</string><string>--old</string></array>
</dict></plist>"#;
        let only_args = Patch { args: Some(vec!["--new".to_owned()]), ..Patch::default() };
        let changed = patched(dict(xml), &only_args).expect("patched");
        assert_eq!(changed.get("Program").and_then(Value::as_string), Some("/usr/bin/example"));
        let all: Vec<&str> = changed
            .get("ProgramArguments")
            .and_then(Value::as_array)
            .expect("array")
            .iter()
            .filter_map(Value::as_string)
            .collect();
        assert_eq!(all, ["custom-name", "--new"], "argv[0] is not the executable");
        let only_program = Patch { program: Some("/usr/bin/other".to_owned()), ..Patch::default() };
        let changed = patched(dict(xml), &only_program).expect("patched");
        assert_eq!(changed.get("Program").and_then(Value::as_string), Some("/usr/bin/other"));
        assert_eq!(
            changed.get("ProgramArguments").and_then(Value::as_array).map(Vec::len),
            Some(2),
            "the arguments are untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_change_keeps_the_mode_and_the_owner_of_the_property_list() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("org.example.app.plist");
        std::fs::write(&path, PLIST).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let before = std::fs::metadata(&path).expect("meta");
        let launchd = Launchd {
            level: Level::User,
            dir: dir.path().to_path_buf(),
            dirs: vec![dir.path().to_path_buf()],
            domain: "gui/501".to_owned(),
            launchctl: PathBuf::from("/nowhere/launchctl"),
        };
        let patch = Patch { autostart: Some(false), ..Patch::default() };
        launchd.configure("org.example.app", &patch).expect("configure");
        let after = std::fs::metadata(&path).expect("meta");
        assert_eq!(after.permissions().mode() & 0o777, 0o600, "a restrictive mode stays restrictive");
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()), "the owner stays");
        let (_, changed) = launchd.definition("org.example.app").expect("read").expect("found");
        assert_eq!(changed.get("RunAtLoad").and_then(Value::as_boolean), Some(false));
        assert_eq!(changed.get("StandardOutPath").and_then(Value::as_string), Some("/var/log/app.log"));
    }

    #[test]
    fn a_failed_commit_leaves_the_original_byte_for_byte_and_nothing_beside_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("org.example.app.plist");
        std::fs::write(&path, PLIST).expect("write");
        let before = std::fs::read(&path).expect("read");
        let failed = replace(&path, b"<plist>a whole new file</plist>", |_staged, _target| {
            Err(std::io::Error::other("the disk is full"))
        });
        assert!(matches!(failed, Err(Error::Failed { .. })), "{failed:?}");
        assert_eq!(std::fs::read(&path).expect("read"), before, "the original is intact");
        let left: Vec<_> = std::fs::read_dir(dir.path()).expect("dir").flatten().collect();
        assert_eq!(left.len(), 1, "the staged file is gone: {left:?}");
        // and the real commit replaces it whole
        replace(&path, b"new", |staged, target| staged.persist(target).map(|_| ()).map_err(|e| e.error))
            .expect("replace");
        assert_eq!(std::fs::read(&path).expect("read"), b"new");
    }

    #[test]
    fn an_acl_or_an_extended_attribute_is_seen_in_the_listing() {
        assert!(!has_extended_metadata("-rw-r--r--  1 root  wheel  - 1 Jan  1 00:00 /a.plist\n"));
        assert!(has_extended_metadata(
            "-rw-r--r--+ 1 root  wheel  - 1 Jan  1 00:00 /a.plist\n 0: user:x allow read\n"
        ));
        assert!(has_extended_metadata(
            "-rw-r--r--@ 1 root  wheel  - 1 Jan  1 00:00 /a.plist\n\tcom.apple.quarantine\t 20\n"
        ));
    }

    #[test]
    fn a_job_of_the_system_is_not_changed() {
        let launchd = Launchd {
            level: Level::System,
            dir: PathBuf::from("/nowhere"),
            dirs: vec![],
            domain: "system".to_owned(),
            launchctl: PathBuf::from("/nowhere/launchctl"),
        };
        // no definition found: not found rather than a guess
        let patch = Patch { autostart: Some(true), ..Patch::default() };
        assert!(matches!(launchd.configure("ghost", &patch), Err(Error::NotFound { .. })));
    }

    #[test]
    fn a_service_is_read_from_its_property_list_and_launchctl() {
        let loaded = parse_print(PRINT);
        let service =
            service_from(std::path::Path::new("/Library/LaunchDaemons/a.plist"), &dict(PLIST), Some(&loaded))
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
