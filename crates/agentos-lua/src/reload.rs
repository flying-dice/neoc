//! Dev hot reload: watch an app's directory; when a Lua (or manifest) file changes, signal the host,
//! which rebuilds the VM from source and swaps it in (the database is kept; the old VM is dropped).

use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;

pub struct AppWatcher {
    _watcher: notify::RecommendedWatcher, // dropping it stops the watch (release on dispose)
    pub rx: mpsc::UnboundedReceiver<()>,
}

fn is_app_file(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "lua" || e == "json")
}

/// Watch `dir` recursively. Each change to a `.lua` or `.json` file sends one `()`; consumers debounce.
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
