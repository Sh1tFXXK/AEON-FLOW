use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum FileEvent {
    Created { path: PathBuf },
    Modified { path: PathBuf },
    Deleted { path: PathBuf },
}

pub fn default_sync_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    #[cfg(target_os = "windows")]
    {
        return vec![home.join("AEON"), home.join("Desktop"), home.join("Documents"), home.join("Downloads"), home.join("Pictures")];
    }
    #[cfg(target_os = "linux")]
    {
        return vec![home.join("AEON"), home.join("Documents"), home.join("Downloads"), home.join("Pictures")];
    }
    vec![home.join("AEON")]
}

pub fn start(dirs: Vec<PathBuf>, tx: mpsc::Sender<FileEvent>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let mapped = match event.kind {
                    EventKind::Create(_) => event.paths.into_iter().map(|path| FileEvent::Created { path }).collect::<Vec<_>>(),
                    EventKind::Modify(_) => event.paths.into_iter().map(|path| FileEvent::Modified { path }).collect::<Vec<_>>(),
                    EventKind::Remove(_) => event.paths.into_iter().map(|path| FileEvent::Deleted { path }).collect::<Vec<_>>(),
                    _ => vec![],
                };
                for m in mapped {
                    let _ = tx.blocking_send(m);
                }
            }
        },
        Config::default(),
    )?;

    for dir in dirs { let _ = std::fs::create_dir_all(&dir); watcher.watch(&dir, RecursiveMode::Recursive)?; }
    Ok(watcher)
}
