use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use egui::Context;
use eyre::Result;
use itertools::Itertools as _;
use notify::{
    event::ModifyKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _,
};
use tracing::{error, trace};

pub struct ModificationWatcher {
    watcher: RecommendedWatcher,
}
impl ModificationWatcher {
    pub fn new(tx: mpsc::Sender<Modification>, ctx: Context) -> Result<Self> {
        let (thread_tx, thread_rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(thread_tx)?;
        std::thread::spawn(move || {
            for modification in ModificationIter::new(thread_rx) {
                if tx.send(modification).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            trace!("Exiting watcher thread")
        });
        Ok(Self { watcher })
    }
    pub fn watch(&mut self, path: impl AsRef<Path>, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(self.watcher.watch(path.as_ref(), recursive_mode)?)
    }
}

struct ModificationIter {
    rx: mpsc::Receiver<notify::Result<Event>>,
    buffered: Option<Event>,
}
impl ModificationIter {
    fn new(rx: mpsc::Receiver<notify::Result<Event>>) -> Self {
        Self { rx, buffered: None }
    }
}
impl Iterator for ModificationIter {
    type Item = Modification;

    fn next(&mut self) -> Option<Self::Item> {
        let mut event = match self.buffered.take() {
            Some(event) => event,
            None => match self.rx.recv().ok()? {
                Ok(event) => event,
                Err(e) => {
                    error!("{e:#}");
                    return self.next();
                }
            },
        };
        let get_path = |event: &mut Event| {
            event
                .paths
                .drain(..)
                .exactly_one()
                .inspect_err(|_| error!("got event with more or less than one path"))
                .ok()
        };
        match event.kind {
            EventKind::Create(_) => {
                let Some(path) = get_path(&mut event) else {
                    return self.next();
                };
                Some(Modification::Create { path })
            }
            EventKind::Modify(ModifyKind::Name(_)) => {
                let Some(from) = get_path(&mut event) else {
                    return self.next();
                };
                let mut event = match self.rx.recv().ok()? {
                    Ok(event) => event,
                    Err(e) => {
                        error!("{e:#}");
                        return self.next();
                    }
                };
                match event.kind {
                    EventKind::Modify(ModifyKind::Name(_)) => {
                        let Some(to) = get_path(&mut event) else {
                            return self.next();
                        };
                        Some(Modification::Rename { from, to })
                    }
                    _ => {
                        error!("expected EventKind::Modify(ModifyKind::Name(_) got {event:?}");
                        self.next()
                    }
                }
            }
            EventKind::Remove(_) => {
                let Some(from) = get_path(&mut event) else {
                    return self.next();
                };
                let mut event = match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(event)) => event,
                    Ok(Err(e)) => {
                        error!("{e:#}");
                        return self.next();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return None,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        return Some(Modification::Delete { path: from })
                    }
                };
                match event.kind {
                    EventKind::Create(_) => {
                        let Some(to) = get_path(&mut event) else {
                            return self.next();
                        };
                        Some(Modification::Move { from, to })
                    }
                    _ => {
                        self.buffered = Some(event.clone());
                        Some(Modification::Delete { path: from })
                    }
                }
            }
            _ => self.next(),
        }
    }
}

pub enum Modification {
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Create { path: PathBuf },
    Delete { path: PathBuf },
}
