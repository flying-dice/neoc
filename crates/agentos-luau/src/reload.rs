//! Dev hot reload: watch an app directory; on change, send the new `main.luau` bytes so the host
//! can build a fresh VM and swap it in (old VM is dropped = unloaded).

use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::mpsc;

pub struct AppWatcher {
    _watcher: notify::RecommendedWatcher, // dropping it stops the watch (release on dispose)
    pub rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

pub fn watch_app(dir: &Path) -> notify::Result<AppWatcher> {
    let (tx, rx) = mpsc::unbounded_channel();
    let main: PathBuf = dir.join("main.luau");
    let main2 = main.clone();
    let mut w = recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_))
                && ev.paths.iter().any(|p| p.ends_with("main.luau"))
            {
                // editors write in bursts; the consumer debounces
                if let Ok(bytes) = std::fs::read(&main2) {
                    let _ = tx.send(bytes);
                }
            }
        }
    })?;
    w.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(AppWatcher { _watcher: w, rx })
}

/// Collapse a burst of change events into the latest content.
pub async fn next_debounced(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>, quiet: Duration) -> Option<Vec<u8>> {
    let mut last = rx.recv().await?;
    while let Ok(Some(more)) = tokio::time::timeout(quiet, rx.recv()).await {
        last = more;
    }
    Some(last)
}
