use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvError, RecvTimeoutError},
    time::Duration,
};

use eyre::Result;
use notify::{
    Event as RawEvent, EventKind, RecommendedWatcher, Watcher as _,
    event::{ModifyKind, RenameMode},
};
use tracing::{error, trace};

pub struct EventWatcher {
    watcher: RecommendedWatcher,
}
impl EventWatcher {
    pub fn new(mut handler: impl EventHandler) -> Result<Self> {
        let (thread_tx, thread_rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            if let Some(event) = WindowsEvent::try_map(event) {
                thread_tx.send(event).ok();
            }
        })?;
        std::thread::spawn(move || {
            for modification in WindowsEventParser::new(thread_rx) {
                handler.handle_event(modification);
            }
            trace!("Exiting EventWatcher thread")
        });
        Ok(Self { watcher })
    }
    pub fn watch(&mut self, path: impl AsRef<Path>, recursive_mode: RecursiveMode) -> Result<()> {
        Ok(self.watcher.watch(path.as_ref(), recursive_mode.into())?)
    }
    pub fn unwatch(&mut self, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.watcher.unwatch(path.as_ref())?)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RecursiveMode {
    Recursive,
    NonRecursive,
}
impl From<RecursiveMode> for notify::RecursiveMode {
    fn from(value: RecursiveMode) -> Self {
        match value {
            RecursiveMode::Recursive => notify::RecursiveMode::Recursive,
            RecursiveMode::NonRecursive => notify::RecursiveMode::NonRecursive,
        }
    }
}

pub trait EventHandler: Send + 'static {
    fn handle_event(&mut self, event: Event);
}
impl<F> EventHandler for F
where
    F: FnMut(Event) + Send + 'static,
{
    fn handle_event(&mut self, event: Event) {
        (self)(event)
    }
}
impl EventHandler for mpsc::Sender<Event> {
    fn handle_event(&mut self, event: Event) {
        self.send(event).ok();
    }
}

struct WindowsEventParser {
    rx: mpsc::Receiver<WindowsEvent>,
    buffered: Option<WindowsEvent>,
}
impl WindowsEventParser {
    fn new(rx: mpsc::Receiver<WindowsEvent>) -> Self {
        Self { rx, buffered: None }
    }
}
impl Iterator for WindowsEventParser {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let event = match self.buffered.take() {
            Some(event) => event,
            None => match self.rx.recv() {
                Err(RecvError) => return None,
                Ok(event) => event,
            },
        };
        match event {
            WindowsEvent::Create { path } => Some(Event::Create { path }),
            WindowsEvent::RenameFrom { path: from } => match self.rx.recv() {
                Err(RecvError) => None,
                Ok(WindowsEvent::RenameTo { path: to }) => Some(Event::Rename { from, to }),
                Ok(other_event) => {
                    error!("expected `RenameTo {{ to: _ }}` got `{other_event:?}`");
                    self.buffered = Some(other_event);
                    self.next()
                }
            },
            WindowsEvent::Remove { path: from } => {
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Err(RecvTimeoutError::Disconnected) => None,
                    Ok(WindowsEvent::Create { path: to }) => Some(Event::Move { from, to }),
                    other_event_or_timeout => {
                        if let Ok(other_event) = other_event_or_timeout {
                            self.buffered = Some(other_event);
                        }
                        Some(Event::Delete { path: from })
                    }
                }
            }
            other_event => {
                error!("got out of sequence event `{other_event:?}`");
                self.next()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Create { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Delete { path: PathBuf },
}

#[derive(Debug)]
enum WindowsEvent {
    Create { path: PathBuf },
    Remove { path: PathBuf },
    RenameFrom { path: PathBuf },
    RenameTo { path: PathBuf },
}
impl WindowsEvent {
    fn try_map(event: notify::Result<RawEvent>) -> Option<Self> {
        let event = event.inspect_err(|e| error!("{e:#}")).ok()?;
        let [path] = event
            .paths
            .try_into()
            .expect("windows events only have one path");
        match event.kind {
            EventKind::Create(_) => Some(Self::Create { path }),
            EventKind::Remove(_) => Some(Self::Remove { path }),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                Some(Self::RenameFrom { path })
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => Some(Self::RenameTo { path }),
            _ => None,
        }
    }
}
