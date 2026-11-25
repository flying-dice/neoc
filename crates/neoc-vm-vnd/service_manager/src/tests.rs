//! The module without a real manager: the operations against the in-memory one (which stands in for an operating
//! system whose services others made), a script's view of it, and the systemd manager against a fake `systemctl`.
//! The real manager of the machine is exercised by `native_round_trip` and
//! `native_system_level_without_rights_is_permission_denied` (they change services, so they run only where
//! `NEOC_NATIVE_SERVICE_TESTS=1` says the machine may be changed: the CI runners) and by the read-only
//! `native_discovery_lists_what_the_machine_has_and_looks_each_up_again`.

use std::collections::BTreeMap;

use mlua::Value;
use neoc_vm_core::make_engine;

use super::mock::Mock;
use super::ops::{self, Control};
use super::service_manager::{Error, Level, Patch, Restart, Seed, Spec, State};

fn seed(label: &str) -> Seed {
    Seed { label: label.to_owned(), ..Seed::default() }
}

/// A program that passes the checks of `install`: an absolute path of an executable file.
fn program() -> String {
    std::env::current_exe().expect("the test binary").display().to_string()
}

fn spec(label: &str) -> Spec {
    Spec { label: label.to_owned(), program: program(), ..Spec::default() }
}

fn mock_with_external() -> Mock {
    Mock::seeded(vec![
        Seed {
            label: "nginx".to_owned(),
            description: Some("A web server".to_owned()),
            state: Some(State::Running),
            autostart: Some(true),
            program: Some("/usr/sbin/nginx".to_owned()),
            args: Some(vec!["-g".to_owned(), "daemon off;".to_owned()]),
            environment: Some(BTreeMap::from([("LANG".to_owned(), "C".to_owned())])),
            restart: Some(Restart::Always),
            restart_delay_secs: Some(2),
            ..seed("nginx")
        },
        Seed { locked: Some(true), ..seed("locked") },
        Seed { broken: Some(true), ..seed("broken") },
    ])
}

#[test]
fn services_registered_by_others_are_listed_and_inspected() {
    let mock = mock_with_external();
    let services = ops::list(&mock).expect("list");
    assert_eq!(services.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["broken", "locked", "nginx"]);
    let nginx = ops::get(&mock, "nginx").expect("get").expect("a service");
    assert_eq!(nginx.state, State::Running);
    assert_eq!(nginx.autostart, Some(true));
    assert_eq!(nginx.program.as_deref(), Some("/usr/sbin/nginx"));
    assert_eq!(nginx.restart, Some(Restart::Always));
    assert_eq!(ops::get(&mock, "nothing").expect("get"), None);
}

#[test]
fn a_change_made_behind_the_managers_back_is_seen_at_the_next_call() {
    let mock = mock_with_external();
    mock.externally("nginx", |s| s.state = State::Stopped);
    assert_eq!(ops::get(&mock, "nginx").unwrap().unwrap().state, State::Stopped);
    mock.externally("newcomer", |s| s.state = State::Running);
    assert!(ops::list(&mock).unwrap().iter().any(|s| s.label == "newcomer"));
    mock.externally_removed("nginx");
    assert_eq!(ops::get(&mock, "nginx").unwrap(), None);
    assert_eq!(
        ops::control(&mock, "nginx", Control::Start),
        Err(Error::NotFound { label: "nginx".to_owned() })
    );
}

#[test]
fn configure_changes_what_is_given_and_keeps_the_rest() {
    let mock = mock_with_external();
    let before = ops::get(&mock, "nginx").unwrap().unwrap();
    let changed =
        ops::configure(&mock, "nginx", Patch { autostart: Some(false), ..Patch::default() }).unwrap();
    assert_eq!(changed.autostart, Some(false));
    let mut expected = before.clone();
    expected.autostart = Some(false);
    assert_eq!(changed, expected, "nothing but the autostart changed");
    let changed =
        ops::configure(&mock, "nginx", Patch { restart_delay_secs: Some(9), ..Patch::default() }).unwrap();
    expected.restart_delay_secs = Some(9);
    assert_eq!(changed, expected);
}

#[test]
fn an_option_the_manager_does_not_keep_is_refused_by_name() {
    let mock = Mock::seeded(vec![]);
    let refused = ops::install(&mock, Spec { user: Some("daemon".to_owned()), ..spec("app") }).unwrap_err();
    assert!(matches!(&refused, Error::Unsupported { option: Some(o), .. } if o == "user"), "{refused:?}");
    assert_eq!(ops::get(&mock, "app").unwrap(), None, "nothing was registered");
}

#[test]
fn the_life_of_a_service_is_install_start_inspect_stop_uninstall() {
    let mock = Mock::seeded(vec![]);
    let installed = ops::install(
        &mock,
        Spec { autostart: Some(true), description: Some("Mine".to_owned()), ..spec("app") },
    )
    .unwrap();
    assert_eq!(installed.state, State::Stopped);
    assert_eq!(installed.autostart, Some(true));
    assert_eq!(ops::install(&mock, spec("app")), Err(Error::AlreadyExists { label: "app".to_owned() }));
    assert_eq!(ops::control(&mock, "app", Control::Start).unwrap().state, State::Running);
    assert_eq!(ops::control(&mock, "app", Control::Restart).unwrap().state, State::Running);
    assert_eq!(ops::control(&mock, "app", Control::Stop).unwrap().state, State::Stopped);
    ops::uninstall(&mock, "app").unwrap();
    assert_eq!(ops::get(&mock, "app").unwrap(), None);
    assert_eq!(ops::uninstall(&mock, "app"), Err(Error::NotFound { label: "app".to_owned() }));
}

#[test]
fn a_refusal_of_the_system_and_a_failed_start_are_typed() {
    let mock = mock_with_external();
    for result in [
        ops::control(&mock, "locked", Control::Stop),
        ops::configure(&mock, "locked", Patch { autostart: Some(false), ..Patch::default() }),
    ] {
        assert!(matches!(result, Err(Error::PermissionDenied { .. })), "{result:?}");
    }
    assert!(matches!(ops::uninstall(&mock, "locked"), Err(Error::PermissionDenied { .. })));
    assert_eq!(
        ops::get(&mock, "locked").unwrap().unwrap().state,
        State::Stopped,
        "a refused change changed nothing"
    );
    let failed = ops::control(&mock, "broken", Control::Start).unwrap_err();
    assert!(matches!(&failed, Error::Failed { operation, .. } if operation == "start"), "{failed:?}");
    assert_eq!(ops::get(&mock, "broken").unwrap().unwrap().state, State::Failed);
}

#[test]
fn what_cannot_be_a_service_is_invalid_input() {
    let mock = Mock::seeded(vec![]);
    fn invalid<T>(result: Result<T, Error>) -> bool {
        matches!(result, Err(Error::InvalidInput { .. }))
    }
    assert!(
        invalid(ops::install(&mock, Spec { label: "-rf".to_owned(), ..spec("x") })),
        "a label cannot look like an option"
    );
    assert!(invalid(ops::install(&mock, Spec { label: "a b".to_owned(), ..spec("x") })));
    assert!(invalid(ops::install(&mock, Spec { program: "relative/program".to_owned(), ..spec("x") })));
    assert!(invalid(ops::install(
        &mock,
        Spec { program: "/nowhere/neoc-no-such-program".to_owned(), ..spec("x") }
    )));
    assert!(
        invalid(ops::install(&mock, Spec { program: "/".to_owned(), ..spec("x") })),
        "a directory is not a program"
    );
    assert!(invalid(ops::install(
        &mock,
        Spec { description: Some("a\nExecStart=/bin/evil".to_owned()), ..spec("x") }
    )));
    assert!(invalid(ops::install(&mock, Spec { args: Some(vec!["a\nb".to_owned()]), ..spec("x") })));
    assert!(invalid(ops::install(
        &mock,
        Spec { working_directory: Some("/nowhere/at/all".to_owned()), ..spec("x") }
    )));
    assert!(invalid(ops::install(
        &mock,
        Spec { environment: Some(BTreeMap::from([("A B".to_owned(), String::new())])), ..spec("x") }
    )));
    assert!(invalid(ops::configure(&mock, "x", Patch::default())), "a change sets something");
    assert!(invalid(ops::get(&mock, "")));
    assert_eq!(ops::list(&mock).unwrap(), vec![], "none of them was registered");
}

// -- a script's view -----------------------------------------------------------------------------------------------

async fn run<R: mlua::FromLuaMulti>(script: &str) -> R {
    make_engine()
        .expect("engine")
        .load(script)
        .call_async(())
        .await
        .unwrap_or_else(|e| panic!("the script failed: {e}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_script_manages_a_service_it_did_not_register() {
    let (state_after_stop, autostart, kind, err_kind): (String, bool, String, String) = run(r#"
        local sm = require("vnd:service_manager")
        local manager = sm.mock({ { label = "nginx", state = "running", autostart = true }, { label = "locked", locked = true } })
        assert(manager:get("nginx").state == "running")
        local stopped = assert(manager:stop("nginx"))
        local configured = assert(manager:configure("nginx", { autostart = false }))
        local _, err = manager:stop("locked")
        local missing, none = manager:get("absent")
        assert(missing == nil and none == nil)
        return stopped.state, configured.autostart, manager:capabilities().kind, err.kind
        "#)
    .await;
    assert_eq!(state_after_stop, "stopped");
    assert!(!autostart);
    assert_eq!(kind, "mock");
    assert_eq!(err_kind, "permission_denied");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_script_sees_typed_errors_and_refused_options() {
    let (invalid, unsupported, exists, text): (String, String, String, String) = run(&format!(r#"
        local sm = require("vnd:service_manager")
        local manager = sm.mock()
        local _, invalid = manager:install({{ label = "bad label", program = "/bin/sh" }})
        local _, unsupported = manager:install({{ label = "a", program = "{program}", user = "daemon" }})
        assert(manager:install({{ label = "b", program = "{program}" }}))
        local _, exists = manager:install({{ label = "b", program = "{program}" }})
        local ok, refused = pcall(manager.install, manager, {{ label = "c", program = "{program}", nonsense = true }})
        assert(not ok)
        return invalid.kind, unsupported.kind .. ":" .. tostring(unsupported.option), exists.kind, tostring(refused)
        "#, program = program().replace('\\', "\\\\")))
    .await;
    assert_eq!(invalid, "invalid_input");
    assert_eq!(unsupported, "unsupported:user");
    assert_eq!(exists, "already_exists");
    assert!(text.contains("nonsense"), "an unknown option is refused and named: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_environment_chooses_the_shared_mock_manager() {
    let manager = super::ops::open(Some("mock"), None).expect("the shared mock");
    let again = super::ops::open(Some("mock"), None).expect("the shared mock");
    let first = manager.capabilities().kind;
    assert_eq!(first, "mock");
    let value: Value = run(r#"return require("vnd:service_manager").mock() ~= nil"#).await;
    assert_eq!(value, Value::Boolean(true));
    assert_eq!(again.capabilities().kind, "mock");
    assert!(matches!(super::ops::open(Some("nonsense"), None), Err(Error::InvalidInput { .. })));
}

// -- systemd against a fake systemctl --------------------------------------------------------------------------------

#[test]
fn a_name_with_spaces_round_trips_through_the_mock() {
    let manager = Mock::seeded(vec![seed("Print Spooler")]);
    let found = ops::get(&manager, "Print Spooler").expect("get").expect("there");
    assert_eq!(found.label, "Print Spooler");
    assert_eq!(ops::control(&manager, "Print Spooler", Control::Start).expect("start").state, State::Running);
    // a new service is held to the stricter policy
    let spec = Spec { label: "New Service".to_owned(), program: "/bin/sh".to_owned(), ..Spec::default() };
    assert!(matches!(ops::install(&manager, spec), Err(Error::InvalidInput { .. })));
}

#[cfg(unix)]
mod fake_systemd {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;
    use crate::systemd::Systemd;

    /// A `systemctl` made of a shell script over a directory: `<unit>.show` holds what `show` prints. It answers
    /// `Interactive authentication required.` when the directory has a `deny` file.
    fn fake(dir: &Path) -> std::path::PathBuf {
        let script = dir.join("systemctl");
        let body = format!(
            r#"#!/bin/sh
D='{dir}'
while [ "$1" = "--user" ] || [ "$1" = "--no-pager" ] || [ "$1" = "--no-ask-password" ]; do shift; done
cmd="$1"; shift
edit() {{ sed "$1" "$2" > "$2.tmp" && mv "$2.tmp" "$2"; }}
mutating() {{ if [ -e "$D/deny" ]; then echo "Interactive authentication required." >&2; exit 1; fi; }}
case "$cmd" in
  list-unit-files) for f in "$D"/*.show; do n=$(basename "$f" .show); echo "$n.service enabled enabled"; done ;;
  list-units) ;;
  is-system-running) echo running ;;
  daemon-reload) mutating; echo reload >> "$D/log" ;;
  show)
    shift
    for u in "$@"; do
      n="${{u%.service}}"
      if [ -e "$D/$n.show" ]; then cat "$D/$n.show"; else printf 'Id=%s\nLoadState=not-found\nActiveState=inactive\n' "$u"; fi
      echo
    done ;;
  start|restart) mutating; edit 's/^ActiveState=.*/ActiveState=active/;s/^SubState=.*/SubState=running/' "$D/${{1%.service}}.show"; echo "$cmd $1" >> "$D/log" ;;
  stop) mutating; edit 's/^ActiveState=.*/ActiveState=inactive/;s/^SubState=.*/SubState=dead/' "$D/${{1%.service}}.show"; echo "stop $1" >> "$D/log" ;;
  enable) mutating; edit 's/^UnitFileState=.*/UnitFileState=enabled/' "$D/${{1%.service}}.show"; echo "enable $1" >> "$D/log" ;;
  disable) mutating; edit 's/^UnitFileState=.*/UnitFileState=disabled/' "$D/${{1%.service}}.show"; echo "disable $1" >> "$D/log" ;;
  *) echo "unknown $cmd" >&2; exit 2 ;;
esac
"#,
            dir = dir.display()
        );
        std::fs::write(&script, body).expect("write the fake");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        script
    }

    fn unit(dir: &Path, name: &str) {
        std::fs::write(
            dir.join(format!("{name}.show")),
            format!("Id={name}.service\nDescription=The {name} service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nUnitFileState=disabled\nFragmentPath=/usr/lib/systemd/system/{name}.service\nRestart=on-failure\nRestartUSec=100ms\nExecStart={{ path=/usr/sbin/{name} ; argv[]=/usr/sbin/{name} -f /etc/{name}.conf ; ignore_errors=no }}\nResult=success\n"),
        )
        .expect("write a unit");
    }

    /// Writing an executable while another test forks leaks its write handle into the child (`ETXTBSY` on exec), so
    /// the rigs take turns.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Rig {
        _serial: std::sync::MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        path: std::path::PathBuf,
        dropins: std::path::PathBuf,
        manager: Systemd,
    }

    fn rig() -> Rig {
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let dropins = path.join("dropins");
        unit(&path, "web");
        unit(&path, "db");
        let manager = Systemd::with(Level::System, fake(&path), dropins.clone());
        Rig { _serial: serial, _dir: dir, path, dropins, manager }
    }

    #[test]
    fn units_of_the_manager_are_listed_and_controlled_whoever_made_them() {
        let rig = rig();
        let all = ops::list(&rig.manager).expect("list");
        assert_eq!(all.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["db", "web"]);
        assert_eq!(all[1].args.as_deref(), Some(&["-f".to_owned(), "/etc/web.conf".to_owned()][..]));
        assert_eq!(ops::get(&rig.manager, "ghost").expect("get"), None);
        assert_eq!(ops::control(&rig.manager, "web", Control::Start).expect("start").state, State::Running);
        assert_eq!(ops::control(&rig.manager, "web", Control::Stop).expect("stop").state, State::Stopped);
        // another program started it: the next call sees that, nothing was remembered
        std::fs::write(
            rig.path.join("web.show"),
            std::fs::read_to_string(rig.path.join("web.show"))
                .unwrap()
                .replace("ActiveState=inactive", "ActiveState=failed"),
        )
        .unwrap();
        assert_eq!(ops::get(&rig.manager, "web").unwrap().unwrap().state, State::Failed);
        assert!(ops::control(&rig.manager, "ghost", Control::Start).is_err());
    }

    #[test]
    fn a_refusal_of_the_system_is_permission_denied_and_changes_nothing() {
        let rig = rig();
        std::fs::write(rig.path.join("deny"), "").unwrap();
        let started = ops::control(&rig.manager, "web", Control::Start).unwrap_err();
        assert!(matches!(started, Error::PermissionDenied { .. }), "{started:?}");
        let configured = ops::configure(
            &rig.manager,
            "web",
            Patch { description: Some("x".to_owned()), ..Patch::default() },
        )
        .unwrap_err();
        assert!(matches!(configured, Error::PermissionDenied { .. }), "{configured:?}");
        assert_eq!(ops::get(&rig.manager, "web").unwrap().unwrap().state, State::Stopped);
    }

    #[test]
    fn configure_writes_a_drop_in_and_leaves_the_units_own_settings_alone() {
        let rig = rig();
        let unit_before = std::fs::read_to_string(rig.path.join("web.show")).unwrap();
        let changed = ops::configure(
            &rig.manager,
            "web",
            Patch {
                autostart: Some(true),
                restart: Some(Restart::Always),
                program: Some("/bin/sh".to_owned()),
                args: Some(vec!["--verbose".to_owned()]),
                ..Patch::default()
            },
        )
        .expect("configure");
        assert_eq!(changed.autostart, Some(true), "enabled through systemctl");
        let text =
            std::fs::read_to_string(rig.dropins.join("web.service.d/90-vos.conf")).expect("the drop-in");
        assert!(text.contains("ExecStart=\nExecStart=\"/bin/sh\" \"--verbose\"\n"), "{text}");
        assert!(text.contains("Restart=always\n"));
        assert!(!text.contains("Description"), "what was not given is not written: {text}");
        // a second change adds to the first and forgets nothing
        ops::configure(
            &rig.manager,
            "web",
            Patch { description: Some("Mine".to_owned()), ..Patch::default() },
        )
        .expect("configure");
        let text = std::fs::read_to_string(rig.dropins.join("web.service.d/90-vos.conf")).unwrap();
        assert!(
            text.contains("Description=Mine\n")
                && text.contains("Restart=always\n")
                && text.contains("\"--verbose\""),
            "{text}"
        );
        // the unit's own definition was never written to
        assert_eq!(
            std::fs::read_to_string(rig.path.join("web.show"))
                .unwrap()
                .replace("UnitFileState=enabled", "UnitFileState=disabled"),
            unit_before
        );
        assert!(std::fs::read_to_string(rig.path.join("log")).unwrap().contains("reload"));
    }

    #[test]
    fn a_command_is_rewritten_only_when_that_loses_nothing() {
        let rig = rig();
        // the arguments are shown joined by spaces: `--name` `hello world` cannot be told from three arguments
        std::fs::write(
            rig.path.join("spaced.show"),
            "Id=spaced.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nUnitFileState=disabled\nFragmentPath=/usr/lib/systemd/system/spaced.service\nExecStart={ path=/usr/sbin/spaced ; argv[]=/usr/sbin/spaced --name hello world ; ignore_errors=no }\n",
        )
        .unwrap();
        let program_only = Patch { program: Some("/bin/sh".to_owned()), ..Patch::default() };
        let refused = ops::configure(&rig.manager, "spaced", program_only).unwrap_err();
        assert!(
            matches!(&refused, Error::Unsupported { option: Some(o), .. } if o == "program"),
            "{refused:?}"
        );
        let args_only = Patch { args: Some(vec!["-x".to_owned()]), ..Patch::default() };
        assert!(matches!(ops::configure(&rig.manager, "spaced", args_only), Err(Error::Unsupported { .. })));
        assert!(!rig.dropins.join("spaced.service.d").exists(), "nothing was written");
        // a command given whole is not a guess
        let whole = Patch {
            program: Some("/bin/sh".to_owned()),
            args: Some(vec!["--name".to_owned(), "hello world".to_owned()]),
            ..Patch::default()
        };
        ops::configure(&rig.manager, "spaced", whole).expect("a whole command");
        let text = std::fs::read_to_string(rig.dropins.join("spaced.service.d/90-vos.conf")).unwrap();
        assert!(text.contains("\"--name\" \"hello world\"\n"), "{text}");
        // once this module wrote it, the arguments are known exactly and the program alone can change
        let again = Patch { program: Some("/bin/ls".to_owned()), ..Patch::default() };
        ops::configure(&rig.manager, "spaced", again).expect("the program alone");
        let text = std::fs::read_to_string(rig.dropins.join("spaced.service.d/90-vos.conf")).unwrap();
        assert!(text.contains("\"/bin/ls\" \"--name\" \"hello world\"\n"), "{text}");
    }

    #[test]
    fn a_unit_with_several_commands_is_not_rewritten_as_one() {
        let rig = rig();
        std::fs::write(
            rig.path.join("multi.show"),
            "Id=multi.service\nLoadState=loaded\nActiveState=inactive\nSubState=dead\nUnitFileState=disabled\nFragmentPath=/usr/lib/systemd/system/multi.service\nExecStart={ path=/bin/a ; argv[]=/bin/a ; ignore_errors=no }\nExecStart={ path=/bin/b ; argv[]=/bin/b ; ignore_errors=no }\n",
        )
        .unwrap();
        let whole = Patch { program: Some("/bin/sh".to_owned()), args: Some(vec![]), ..Patch::default() };
        let refused = ops::configure(&rig.manager, "multi", whole).unwrap_err();
        assert!(matches!(&refused, Error::Unsupported { .. }), "{refused:?}");
        assert!(!rig.dropins.join("multi.service.d").exists(), "nothing was written");
        // the other settings of such a unit can still change
        let restart = Patch { restart: Some(Restart::Always), ..Patch::default() };
        ops::configure(&rig.manager, "multi", restart).expect("a setting that is not the command");
    }

    #[test]
    fn only_a_drop_in_this_module_wrote_is_removed() {
        let rig = rig();
        let dir = rig.dropins.join("web.service.d");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("90-vos.conf"), "[Service]\nRestart=no\n").unwrap();
        assert!(!rig.manager.owns_dropin("web").unwrap(), "an unrelated file is not ours");
        assert!(rig.dropins.join("web.service.d/90-vos.conf").exists());
        ops::configure(&rig.manager, "db", Patch { restart: Some(Restart::Always), ..Patch::default() })
            .expect("configure");
        assert!(rig.manager.owns_dropin("db").unwrap());
        std::fs::write(rig.dropins.join("db.service.d/10-other.conf"), "[Service]\n").unwrap();
        rig.manager.remove_dropin("db").expect("remove");
        assert!(!rig.dropins.join("db.service.d/90-vos.conf").exists());
        assert!(rig.dropins.join("db.service.d/10-other.conf").exists(), "other drop-ins stay");
    }

    #[test]
    fn a_name_the_manager_made_can_be_managed_whatever_it_holds() {
        let rig = rig();
        // an escaped instance name, as `systemd-escape` makes them
        let escaped = "app\\x2dname@a";
        unit(&rig.path, escaped);
        assert_eq!(ops::get(&rig.manager, escaped).expect("get").map(|s| s.label), Some(escaped.to_owned()));
        let started = ops::control(&rig.manager, escaped, Control::Start).expect("start");
        assert_eq!(started.state, State::Running);
        // what could be read as an option or a path is refused
        for bad in ["-rf", "../etc", "a/b", "a\nb", ""] {
            assert!(matches!(ops::get(&rig.manager, bad), Err(Error::InvalidInput { .. })), "{bad:?}");
        }
    }

    #[test]
    fn a_pattern_is_not_a_name_and_cannot_reach_more_than_one_service() {
        let rig = rig();
        for pattern in ["*", "w?b", "[wd]*", "web*"] {
            for what in [Control::Start, Control::Stop, Control::Restart] {
                let refused = ops::control(&rig.manager, pattern, what).unwrap_err();
                assert!(matches!(refused, Error::InvalidInput { .. }), "{pattern:?}: {refused:?}");
            }
            assert!(matches!(ops::get(&rig.manager, pattern), Err(Error::InvalidInput { .. })));
            let patch = Patch { autostart: Some(true), ..Patch::default() };
            assert!(matches!(ops::configure(&rig.manager, pattern, patch), Err(Error::InvalidInput { .. })));
        }
        assert!(!rig.path.join("log").exists(), "systemctl was never asked to change anything");
        // the exact name still works
        assert_eq!(ops::control(&rig.manager, "web", Control::Start).unwrap().state, State::Running);
        assert_eq!(ops::get(&rig.manager, "db").unwrap().unwrap().state, State::Stopped);
    }

    #[test]
    fn a_drop_in_edited_since_it_was_written_is_not_regenerated_over() {
        let rig = rig();
        let restart = Patch { restart: Some(Restart::Always), ..Patch::default() };
        ops::configure(&rig.manager, "web", restart).expect("configure");
        let file = rig.dropins.join("web.service.d/90-vos.conf");
        // an administrator adds a directive and leaves the mark where it was
        let text = std::fs::read_to_string(&file).unwrap();
        let edited = format!("{text}[Service]\nNice=5\n");
        std::fs::write(&file, &edited).unwrap();
        let description = Patch { description: Some("Unrelated".to_owned()), ..Patch::default() };
        let refused = ops::configure(&rig.manager, "web", description).unwrap_err();
        assert!(matches!(refused, Error::Failed { .. }), "{refused:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), edited, "the edit is still there");
        assert!(!rig.manager.owns_dropin("web").unwrap(), "an edited file is not removed as ours");
    }

    #[test]
    fn a_file_that_is_not_ours_is_not_replaced_and_an_unknown_unit_is_not_found() {
        let rig = rig();
        let dir = rig.dropins.join("web.service.d");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("90-vos.conf"), "[Service]\nRestart=no\n").unwrap();
        let refused =
            ops::configure(&rig.manager, "web", Patch { restart: Some(Restart::Always), ..Patch::default() })
                .unwrap_err();
        assert!(matches!(refused, Error::Failed { .. }), "{refused:?}");
        assert_eq!(std::fs::read_to_string(dir.join("90-vos.conf")).unwrap(), "[Service]\nRestart=no\n");
        let missing =
            ops::configure(&rig.manager, "ghost", Patch { autostart: Some(true), ..Patch::default() })
                .unwrap_err();
        assert_eq!(missing, Error::NotFound { label: "ghost".to_owned() });
    }

    #[test]
    fn a_unit_of_the_system_cannot_be_unregistered() {
        let rig = rig();
        let refused = ops::uninstall(&rig.manager, "web").unwrap_err();
        assert!(matches!(&refused, Error::Unsupported { .. }), "{refused:?}");
        assert!(ops::get(&rig.manager, "web").unwrap().is_some());
    }

    #[test]
    fn a_machine_without_systemctl_is_unavailable() {
        let manager = Systemd::with(Level::System, "/nowhere/systemctl".into(), "/tmp".into());
        assert!(matches!(ops::list(&manager), Err(Error::Unavailable { .. })));
    }
}

// -- the real manager of the machine, changed ------------------------------------------------------------------------
//
// These register, start, change and remove a service of the machine they run on, so they run only where
// `NEOC_NATIVE_SERVICE_TESTS=1` says the machine may be changed: the CI runners, which are fresh virtual machines
// (see `.github/workflows/ci.yml`, which runs them as root on Linux and macOS and as the administrator on Windows).
// Set it, and a manager that cannot be reached is a failure, not a skip: the point is to run.

const NATIVE: &str = "NEOC_NATIVE_SERVICE_TESTS";

fn native_allowed() -> bool {
    std::env::var(NATIVE).as_deref() == Ok("1")
}

/// Whether this process may change services of the whole machine: root on Unix, and the administrator of a CI runner
/// on Windows.
fn privileged() -> bool {
    if cfg!(windows) {
        return true;
    }
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "0")
        .unwrap_or(false)
}

#[cfg(unix)]
const LONG_RUNNING: (&str, &[&str]) = ("/bin/sleep", &["300"]);
#[cfg(windows)]
const LONG_RUNNING: (&str, &[&str]) = ("C:\\Windows\\System32\\cmd.exe", &[]);

fn spec_with(program: &str, label: &str) -> Spec {
    Spec { label: label.to_owned(), program: program.to_owned(), ..Spec::default() }
}

/// Removes the service when the test ends, whichever way it ends.
struct Registered<'a> {
    backend: &'a dyn super::backend::Backend,
    label: String,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let _ = self.backend.stop(&self.label);
        let _ = self.backend.uninstall(&self.label);
    }
}

/// register, inspect, start, restart, change, stop and unregister on the real manager, with what the manager can do
/// read from its own capabilities: what it cannot keep is refused by name, what it can is kept and read back, and what
/// a change did not mention stays.
#[test]
fn native_round_trip() {
    if !native_allowed() {
        return eprintln!("skipped: set {NATIVE}=1 to change services of this machine");
    }
    let level = if privileged() { Level::System } else { Level::User };
    let manager =
        ops::open(None, Some(level)).unwrap_or_else(|e| panic!("the manager of the machine: {e:?}"));
    let backend = manager.backend();
    let caps = backend.capabilities();
    eprintln!("{} at the {:?} level", caps.kind, caps.level);
    let installs = |option: &str| caps.install_options.iter().any(|o| o == option);
    let configurable = |option: &str| caps.configurable.iter().any(|o| o == option);
    let reported = |option: &str| caps.reported.iter().any(|o| o == option);

    let label = format!("neoc-ci-{}", std::process::id());
    let (program, args) = LONG_RUNNING;
    let mut spec = spec_with(program, &label);
    if installs("args") && !args.is_empty() {
        spec.args = Some(args.iter().map(|a| (*a).to_owned()).collect());
    }
    if installs("description") {
        spec.description = Some("neoc native test".to_owned());
    }
    if installs("environment") {
        spec.environment = Some(BTreeMap::from([("NEOC_TEST".to_owned(), "a value".to_owned())]));
    }
    let installed = ops::install(backend, spec).unwrap_or_else(|e| panic!("install: {e:?}"));
    let _cleanup = Registered { backend, label: label.clone() };
    assert_ne!(installed.state, State::Running, "a registered service is not started");

    // what was registered is what the manager reports
    if reported("program") {
        assert_eq!(installed.program.as_deref(), Some(program));
    }
    if reported("description") && installs("description") {
        assert_eq!(installed.description.as_deref(), Some("neoc native test"));
    }
    if reported("environment") && installs("environment") {
        let env = installed.environment.as_ref().expect("the environment is reported");
        assert_eq!(env.get("NEOC_TEST").map(String::as_str), Some("a value"));
    }
    // registered by this call, listed like any other service
    assert!(ops::list(backend).expect("list").iter().any(|s| s.label == label));
    // a second registration of the label is refused
    assert!(matches!(ops::install(backend, spec_with(program, &label)), Err(Error::AlreadyExists { .. })));

    if caps.kind == "windows" {
        // `cmd.exe` does not speak the service protocol: the manager's refusal to start it is a typed failure
        let refused = ops::control(backend, &label, Control::Start).unwrap_err();
        assert!(!matches!(refused, Error::NotFound { .. }), "{refused:?}");
    } else {
        assert_eq!(ops::control(backend, &label, Control::Start).expect("start").state, State::Running);
        assert_eq!(ops::control(backend, &label, Control::Restart).expect("restart").state, State::Running);
    }

    // a change keeps everything it does not mention
    if configurable("autostart") {
        let patch = Patch { autostart: Some(true), ..Patch::default() };
        let changed = ops::configure(backend, &label, patch).expect("configure autostart");
        assert_eq!(changed.autostart, Some(true));
    }
    if configurable("restart") {
        let patch = Patch { restart: Some(Restart::Always), ..Patch::default() };
        let changed = ops::configure(backend, &label, patch).expect("configure restart");
        assert_eq!(changed.restart, Some(Restart::Always));
        if reported("program") {
            assert_eq!(changed.program.as_deref(), Some(program), "the program stays");
        }
        if reported("environment") && installs("environment") {
            assert!(changed.environment.as_ref().is_some_and(|e| e.contains_key("NEOC_TEST")));
        }
    }
    if configurable("description") {
        let patch = Patch { description: Some("changed by the test".to_owned()), ..Patch::default() };
        let changed = ops::configure(backend, &label, patch).expect("configure description");
        assert_eq!(changed.description.as_deref(), Some("changed by the test"));
        assert_eq!(changed.autostart.is_some(), configurable("autostart") || reported("autostart"));
    }
    // an option the manager does not keep is refused by name
    if let Some(option) = ["restart", "program"].into_iter().find(|o| !configurable(o)) {
        let patch = match option {
            "restart" => Patch { restart: Some(Restart::Always), ..Patch::default() },
            _ => Patch { program: Some(program.to_owned()), ..Patch::default() },
        };
        let refused = ops::configure(backend, &label, patch).unwrap_err();
        assert!(matches!(&refused, Error::Unsupported { option: Some(o), .. } if o == option), "{refused:?}");
    }

    if caps.kind != "windows" {
        assert_eq!(ops::control(backend, &label, Control::Stop).expect("stop").state, State::Stopped);
    }
    ops::uninstall(backend, &label).expect("uninstall");
    assert_eq!(ops::get(backend, &label).expect("get"), None);
    assert!(matches!(ops::uninstall(backend, &label), Err(Error::NotFound { .. })));
}

/// Without the rights to change services of the machine, the system level answers with a typed refusal and changes
/// nothing: no escalation is attempted. It runs where the test is not root (the first CI step, before the one that
/// runs `native_round_trip` as root).
#[cfg(unix)]
#[test]
fn native_system_level_without_rights_is_permission_denied() {
    if !native_allowed() {
        return eprintln!("skipped: set {NATIVE}=1 to try services of this machine");
    }
    if privileged() {
        return eprintln!("skipped: this process has the rights, so there is nothing to be denied");
    }
    let manager =
        ops::open(None, Some(Level::System)).unwrap_or_else(|e| panic!("the manager of the machine: {e:?}"));
    let backend = manager.backend();
    let label = format!("neoc-denied-{}", std::process::id());
    let _cleanup = Registered { backend, label: label.clone() };
    let refused = ops::install(backend, spec_with("/bin/sleep", &label)).unwrap_err();
    assert!(matches!(refused, Error::PermissionDenied { .. }), "{refused:?}");
    assert_eq!(ops::get(backend, &label).expect("get"), None, "nothing was registered");
}

/// The real manager of the machine, read only (it changes nothing, so it is not ignored): every service it lists can be
/// looked up again by the name it listed it under, whatever that name holds (a Windows service name has spaces), and
/// its state is one the manager really reported. It runs on Linux, macOS and Windows in CI, which is where the native
/// discovery of each platform is exercised; it skips itself where there is no service manager to ask.
#[test]
fn native_discovery_lists_what_the_machine_has_and_looks_each_up_again() {
    let manager = match ops::open(None, Some(Level::System)) {
        Ok(manager) => manager,
        Err(e) => return eprintln!("skipped: no service manager to ask ({e})"),
    };
    let backend = manager.backend();
    let listed = match ops::list(backend) {
        Ok(listed) => listed,
        Err(e) => return eprintln!("skipped: the service manager does not answer ({e})"),
    };
    eprintln!("{} manager lists {} services", backend.capabilities().kind, listed.len());
    assert!(!listed.is_empty(), "a machine has services");
    for service in listed.iter().take(25) {
        let again =
            ops::get(backend, &service.label).unwrap_or_else(|e| panic!("{:?}: {e:?}", service.label));
        assert_eq!(
            again.as_ref().map(|s| s.label.as_str()),
            Some(service.label.as_str()),
            "{:?} was listed and cannot be looked up",
            service.label
        );
    }
}
