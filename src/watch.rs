use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

pub struct Watcher {
    pub rx: Receiver<()>,
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

pub fn spawn(root: &Path) -> Result<Watcher> {
    let (tx, rx) = channel();
    let mut debouncer =
        new_debouncer(Duration::from_millis(120), move |res: DebounceEventResult| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })?;
    debouncer
        .watcher()
        .watch(root, notify::RecursiveMode::Recursive)?;
    Ok(Watcher {
        rx,
        _debouncer: debouncer,
    })
}
