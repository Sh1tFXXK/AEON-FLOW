use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use tokio::sync::broadcast;

pub fn start_watcher(
    dir: &Path,
    tx: broadcast::Sender<String>,
) -> notify::Result<RecommendedWatcher> {
    let (notify_tx, notify_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(notify_tx, Config::default())?;
    watcher.watch(dir, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        for res in notify_rx {
            match res {
                Ok(Event { paths, .. }) => {
                    let path_str = paths
                        .first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    let _ = tx.send(path_str);
                }
                Err(e) => tracing::warn!("watch error: {e}"),
            }
        }
    });

    Ok(watcher)
}
