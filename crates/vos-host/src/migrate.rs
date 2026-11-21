//! The one-time move from the data directory this program used before it was renamed (`~/.agentos`, with an
//! `agentos.toml` in it) to `~/.vos`, so that paired devices, app data, the agent roster and the chat history
//! survive the rename.
//!
//! This module is the only place in the program that still spells the old name.
//!
//! The rules, in order:
//!
//! 1. `~/.vos` already exists (a directory, a file or a link): nothing happens. The person has started the renamed
//!    program already (or made the directory on purpose), and two directories are never merged.
//! 2. `~/.agentos` is not a directory: nothing happens (a fresh install).
//! 3. Otherwise `~/.agentos/agentos.toml` becomes `~/.agentos/vos.toml` (when there is no `vos.toml` beside it) and
//!    the directory is renamed to `~/.vos`. A rename inside one parent is atomic, so a failure leaves everything
//!    where it was: a failed rename of the directory puts the config file's name back, and the error says which
//!    two paths were involved. Nothing is ever copied or deleted.
//!
//! It runs only when the data directory in use is the default `~/.vos` (an explicit `VOS_DATA` or `data_dir`
//! names a directory of the person's choice, and an old default directory is not theirs to be moved then).

use anyhow::{bail, Context};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The data directory of the program before its rename, under the home directory.
const LEGACY_DIR: &str = ".agentos";
/// The data directory the program uses now, under the home directory.
pub(crate) const DIR: &str = ".vos";
/// The config file name before the rename, inside the data directory.
const LEGACY_CONFIG: &str = "agentos.toml";
/// The config file name now.
const CONFIG: &str = "vos.toml";

/// What [`migrate_legacy_data_dir`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Migration {
    /// There was nothing to move (a fresh install, or `~/.vos` exists already).
    NotNeeded,
    /// `~/.agentos` was moved to `~/.vos`; `config_renamed` says whether its `agentos.toml` became `vos.toml`.
    Moved { from: PathBuf, to: PathBuf, config_renamed: bool },
}

/// The default data directory under `home`: where the host keeps its data when nothing says otherwise.
#[must_use]
pub fn default_data_dir(home: &Path) -> PathBuf {
    home.join(DIR)
}

/// Move `~/.agentos` to `~/.vos` when the data directory in use is that default and only the old one exists.
///
/// An error means nothing was lost and nothing was changed (see the module docs); the caller stops and tells the
/// person, rather than starting on an empty directory beside their data.
pub fn migrate_if_default(data_dir: &Path, home: &Path) -> anyhow::Result<Migration> {
    if data_dir != default_data_dir(home) {
        return Ok(Migration::NotNeeded);
    }
    migrate_legacy_data_dir(home)
}

/// The move itself, with `std::fs::rename`; see [`migrate_if_default`] for the guard around it.
pub fn migrate_legacy_data_dir(home: &Path) -> anyhow::Result<Migration> {
    migrate_with(home, &|from, to| std::fs::rename(from, to))
}

/// [`migrate_legacy_data_dir`] over any `rename`, so a test can make it fail.
fn migrate_with(home: &Path, rename: &dyn Fn(&Path, &Path) -> io::Result<()>) -> anyhow::Result<Migration> {
    let from = home.join(LEGACY_DIR);
    let to = default_data_dir(home);
    if std::fs::symlink_metadata(&to).is_ok() || !from.is_dir() {
        return Ok(Migration::NotNeeded);
    }

    let old_config = from.join(LEGACY_CONFIG);
    let new_config = from.join(CONFIG);
    let rename_config = old_config.is_file() && std::fs::symlink_metadata(&new_config).is_err();
    if rename_config {
        rename(&old_config, &new_config).with_context(|| {
            format!(
                "could not rename {} to {} (nothing was moved; {} is untouched)",
                old_config.display(),
                new_config.display(),
                from.display()
            )
        })?;
    }
    if let Err(e) = rename(&from, &to) {
        let restored = if rename_config { rename(&new_config, &old_config) } else { Ok(()) };
        if let Err(restore) = restored {
            bail!(
                "could not move {} to {}: {e}; and could not put {} back as {}: {restore} (no data was lost: it \
                 is all still in {})",
                from.display(),
                to.display(),
                new_config.display(),
                old_config.display(),
                from.display()
            );
        }
        return Err(e).with_context(|| {
            format!(
                "could not move {} to {} (nothing was changed: your data is still in {})",
                from.display(),
                to.display(),
                from.display()
            )
        });
    }
    Ok(Migration::Moved { from, to, config_renamed: rename_config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fresh home directory for one test.
    fn home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vos-migrate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `~/.agentos`-shaped directory: the system database, an app's data, the config and the MCP file.
    fn legacy_dir(home: &Path) -> PathBuf {
        let dir = home.join(LEGACY_DIR);
        fs::create_dir_all(dir.join("apps")).unwrap();
        fs::write(dir.join("system.db"), b"paired devices").unwrap();
        fs::write(dir.join("apps/calculator.db"), b"app data").unwrap();
        fs::write(dir.join(LEGACY_CONFIG), b"port = 7500\n").unwrap();
        dir
    }

    #[test]
    fn the_old_directory_moves_with_its_data_and_its_config_gets_the_new_name() {
        let home = home("moves");
        let old = legacy_dir(&home);

        let outcome = migrate_legacy_data_dir(&home).unwrap();

        let new = home.join(DIR);
        assert_eq!(outcome, Migration::Moved { from: old.clone(), to: new.clone(), config_renamed: true });
        assert!(!old.exists(), "moved, not copied");
        assert_eq!(fs::read(new.join("system.db")).unwrap(), b"paired devices");
        assert_eq!(fs::read(new.join("apps/calculator.db")).unwrap(), b"app data");
        assert_eq!(fs::read(new.join(CONFIG)).unwrap(), b"port = 7500\n");
        assert!(!new.join(LEGACY_CONFIG).exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_directory_without_a_config_moves_and_says_so() {
        let home = home("no-config");
        let old = legacy_dir(&home);
        fs::remove_file(old.join(LEGACY_CONFIG)).unwrap();

        let outcome = migrate_legacy_data_dir(&home).unwrap();

        assert!(matches!(outcome, Migration::Moved { config_renamed: false, .. }));
        assert!(home.join(DIR).join("system.db").is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_vos_toml_that_is_already_there_is_never_overwritten() {
        let home = home("both-configs");
        let old = legacy_dir(&home);
        fs::write(old.join(CONFIG), b"port = 7600\n").unwrap();

        migrate_legacy_data_dir(&home).unwrap();

        let new = home.join(DIR);
        assert_eq!(fs::read(new.join(CONFIG)).unwrap(), b"port = 7600\n");
        assert_eq!(
            fs::read(new.join(LEGACY_CONFIG)).unwrap(),
            b"port = 7500\n",
            "the old file is left for the person"
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn nothing_happens_when_the_new_directory_exists() {
        let home = home("new-exists");
        let old = legacy_dir(&home);
        let new = home.join(DIR);
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("system.db"), b"already running as vos").unwrap();

        let outcome = migrate_legacy_data_dir(&home).unwrap();

        assert_eq!(outcome, Migration::NotNeeded);
        assert_eq!(
            fs::read(new.join("system.db")).unwrap(),
            b"already running as vos",
            "never merged or replaced"
        );
        assert_eq!(
            fs::read(old.join("system.db")).unwrap(),
            b"paired devices",
            "the old directory is left alone"
        );
        assert!(old.join(LEGACY_CONFIG).is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn nothing_happens_when_neither_directory_exists() {
        let home = home("neither");

        let outcome = migrate_legacy_data_dir(&home).unwrap();

        assert_eq!(outcome, Migration::NotNeeded);
        assert!(!home.join(DIR).exists(), "a fresh install is not given a directory by the migration");
        assert!(!home.join(LEGACY_DIR).exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_file_where_the_old_directory_would_be_is_not_moved() {
        let home = home("old-is-a-file");
        fs::write(home.join(LEGACY_DIR), b"not a directory").unwrap();

        assert_eq!(migrate_legacy_data_dir(&home).unwrap(), Migration::NotNeeded);
        assert!(!home.join(DIR).exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_failed_move_is_reported_and_loses_nothing() {
        let home = home("rename-fails");
        let old = legacy_dir(&home);
        let failing = |from: &Path, to: &Path| -> io::Result<()> {
            // the config rename inside the old directory works, the directory rename does not
            if from.file_name().is_some_and(|n| n == LEGACY_DIR) {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            } else {
                std::fs::rename(from, to)
            }
        };

        let err = migrate_with(&home, &failing).unwrap_err();

        let text = format!("{err:#}");
        assert!(text.contains("could not move") && text.contains("denied"), "{text}");
        assert!(
            text.contains(old.to_str().unwrap()) && text.contains(home.join(DIR).to_str().unwrap()),
            "{text}"
        );
        assert!(!home.join(DIR).exists(), "no half-made new directory");
        assert_eq!(fs::read(old.join("system.db")).unwrap(), b"paired devices");
        assert_eq!(fs::read(old.join("apps/calculator.db")).unwrap(), b"app data");
        assert_eq!(
            fs::read(old.join(LEGACY_CONFIG)).unwrap(),
            b"port = 7500\n",
            "the config name is put back"
        );
        assert!(!old.join(CONFIG).exists());
        // and the next start, with the fault gone, finishes the job
        assert!(matches!(migrate_legacy_data_dir(&home).unwrap(), Migration::Moved { .. }));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_failed_config_rename_stops_before_anything_is_moved() {
        let home = home("config-rename-fails");
        let old = legacy_dir(&home);
        let failing = |_: &Path, _: &Path| -> io::Result<()> { Err(io::Error::other("disk on fire")) };

        let err = migrate_with(&home, &failing).unwrap_err();

        assert!(format!("{err:#}").contains("disk on fire"));
        assert!(!home.join(DIR).exists());
        assert!(old.join(LEGACY_CONFIG).is_file() && old.join("system.db").is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_failed_move_that_cannot_put_the_config_name_back_still_says_where_the_data_is() {
        let home = home("restore-fails");
        let old = legacy_dir(&home);
        // every rename but the first (the config) fails
        let calls = std::cell::Cell::new(0);
        let failing = |from: &Path, to: &Path| -> io::Result<()> {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                std::fs::rename(from, to)
            } else {
                Err(io::Error::other("still broken"))
            }
        };

        let err = migrate_with(&home, &failing).unwrap_err();

        let text = format!("{err:#}");
        assert!(text.contains("could not put") && text.contains("no data was lost"), "{text}");
        assert!(old.join("system.db").is_file() && old.join(CONFIG).is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn an_explicit_data_directory_is_not_a_reason_to_move_the_old_default() {
        let home = home("explicit");
        let old = legacy_dir(&home);

        let elsewhere = home.join("elsewhere");
        assert_eq!(migrate_if_default(&elsewhere, &home).unwrap(), Migration::NotNeeded);
        assert!(old.join("system.db").is_file(), "left where it is");
        assert!(!home.join(DIR).exists());

        let moved = migrate_if_default(&default_data_dir(&home), &home).unwrap();
        assert!(matches!(moved, Migration::Moved { .. }));
        let _ = fs::remove_dir_all(home);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_old_directory_moves_as_a_link_and_its_target_stays_put() {
        let home = home("symlink");
        let target = home.join("on-another-disk");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("system.db"), b"paired devices").unwrap();
        std::os::unix::fs::symlink(&target, home.join(LEGACY_DIR)).unwrap();

        migrate_legacy_data_dir(&home).unwrap();

        assert_eq!(fs::read(home.join(DIR).join("system.db")).unwrap(), b"paired devices");
        assert!(target.join("system.db").is_file(), "the data was not copied or removed");
        let _ = fs::remove_dir_all(home);
    }
}
