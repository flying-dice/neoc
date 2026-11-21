//! Dev hot reload: watch an app's directory; when a Luau (or manifest) file changes, signal the host,
//! which rebuilds the VM from source and swaps it in (the database is kept; the old VM is dropped).

use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;

pub struct AppWatcher {
    _watcher: notify::RecommendedWatcher, // dropping it stops the watch (release on dispose)
    pub rx: mpsc::UnboundedReceiver<()>,
}

fn is_app_file(p: &Path) -> bool {
    // `.luau` is what the build writes; `.lua` is an app ref published before apps were Luau
    p.extension().is_some_and(|e| e == "luau" || e == "lua" || e == "json")
}

/// Watch `dir` recursively. Each change to a `.luau`, `.lua` or `.json` file sends one `()`; consumers debounce.
pub fn watch_dir(dir: &Path) -> notify::Result<AppWatcher> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut w = recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_))
                && ev.paths.iter().any(|p| is_app_file(p))
            {
                let _ = tx.send(());
            }
        }
    })?;
    w.watch(dir, RecursiveMode::Recursive)?;
    Ok(AppWatcher { _watcher: w, rx })
}

/// Wait for a change, then until `quiet` passes with no further one (editors write in bursts).
pub async fn next_change(rx: &mut mpsc::UnboundedReceiver<()>, quiet: Duration) -> Option<()> {
    rx.recv().await?;
    while let Ok(Some(())) = tokio::time::timeout(quiet, rx.recv()).await {}
    Some(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn app_files_are_luau_lua_and_json() {
        for yes in ["app.luau", "dir/app.lua", "app-ref.json"] {
            assert!(is_app_file(Path::new(yes)), "{yes}");
        }
        for no in ["app.luau.tmp", "notes.txt", "app", "app.sql"] {
            assert!(!is_app_file(Path::new(no)), "{no}");
        }
    }

    /// The build writes `app.luau`: a change to that file alone (not `app-ref.json`) must wake the watcher.
    #[tokio::test]
    async fn a_changed_app_luau_on_its_own_is_a_change() {
        let dir = std::env::temp_dir().join(format!("vos-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.luau"), "-- v1\n").unwrap();
        let mut watcher = watch_dir(&dir).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await; // let the watch settle
        std::fs::write(dir.join("app.luau"), "-- v2\n").unwrap();
        let change = tokio::time::timeout(
            Duration::from_secs(10),
            next_change(&mut watcher.rx, Duration::from_millis(100)),
        )
        .await;
        assert!(matches!(change, Ok(Some(()))), "app.luau changed and nothing was signalled");
        drop(watcher);
        let _ = std::fs::remove_dir_all(dir);
    }
}
