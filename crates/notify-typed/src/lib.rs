use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvError, RecvTimeoutError},
    task::{Context, Waker},
    time::Duration,
};

use eyre::Result;
use futures::StreamExt;
use notify::{
    Event as RawEvent, EventKind, RecommendedWatcher, Watcher as _,
    event::{ModifyKind, RenameMode},
};
use tracing::{error, trace};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::stream::DirEventStream;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::stream::DirEventStream;

impl DirEventStream {
    pub fn try_recv(&mut self) -> Option<Event> {
        let mut cx = Context::from_waker(Waker::noop());
        match self.poll_next_unpin(&mut cx) {
            std::task::Poll::Ready(Some(event)) => Some(event),
            _ => None,
        }
    }
    pub fn try_recv_many(&mut self) -> impl Iterator<Item = Event> {
        let mut cx = Context::from_waker(Waker::noop());
        std::iter::from_fn(move || match self.poll_next_unpin(&mut cx) {
            std::task::Poll::Ready(Some(event)) => Some(event),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use futures::StreamExt;
    use tokio::{io::AsyncWriteExt, pin};
    use tracing_subscriber::EnvFilter;

    use super::*;

    #[test]
    fn blocking() {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        let mut watcher = DirEventStream::new("./data").unwrap();
        loop {
            if let Some(event) = watcher.try_recv() {
                println!("{event:?}");
            } else {
                print!(".");
                std::io::stdout().flush().ok();
            }
            // for event in watcher.try_recv_iter() {
            //     println!("{event:?}");
            // }
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    #[test]
    fn asink() {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(test_name_async())
    }
    async fn test_name_async() {
        let watcher = DirEventStream::new("./data").unwrap();
        let out = tokio::io::BufWriter::new(tokio::io::stdout());
        pin!(out);
        pin!(watcher);
        while let Some(action) = watcher.next().await {
            let s = format!("{action:?}\n");
            out.write_all_buf(&mut s.as_bytes()).await.unwrap();
            out.flush().await.unwrap();
        }
    }
}

pub struct EventWatcher {
    watcher: RecommendedWatcher,
}
impl EventWatcher {
    pub fn new(mut handler: impl EventHandler) -> Result<Self> {
        let (thread_tx, thread_rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            if let Some(event) = SystemEvent::try_map(event) {
                thread_tx.send(event).ok();
            }
        })?;
        std::thread::spawn(move || {
            for event in EventParser::new(thread_rx) {
                trace!("{event:?}");
                handler.handle_event(event);
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

struct EventParser {
    rx: mpsc::Receiver<SystemEvent>,
    buffered: Option<SystemEvent>,
}
impl EventParser {
    fn new(rx: mpsc::Receiver<SystemEvent>) -> Self {
        Self { rx, buffered: None }
    }
}
impl Iterator for EventParser {
    type Item = Event;

    #[cfg(target_os = "windows")]
    fn next(&mut self) -> Option<Self::Item> {
        let event = match self.buffered.take() {
            Some(event) => event,
            None => match self.rx.recv() {
                Err(RecvError) => return None,
                Ok(event) => event,
            },
        };
        match event {
            SystemEvent::Create { path } => Some(Event::Create { path }),
            SystemEvent::RenameFrom { path: from } => match self.rx.recv() {
                Err(RecvError) => None,
                Ok(SystemEvent::RenameTo { path: to }) => Some(Event::Rename { from, to }),
                Ok(other_event) => {
                    error!("expected `RenameTo {{ to: _ }}` got `{other_event:?}`");
                    self.buffered = Some(other_event);
                    self.next()
                }
            },
            SystemEvent::Remove { path: from } => {
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Err(RecvTimeoutError::Disconnected) => None,
                    Ok(SystemEvent::Create { path: to }) => Some(Event::Move { from, to }),
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
    #[cfg(target_os = "linux")]
    fn next(&mut self) -> Option<Self::Item> {
        let event = match self.buffered.take() {
            Some(event) => event,
            None => match self.rx.recv() {
                Err(RecvError) => return None,
                Ok(event) => event,
            },
        };
        match event {
            SystemEvent::Create { path } => Some(Event::Create { path }),
            SystemEvent::RenameFrom { path: from } => {
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Err(RecvTimeoutError::Disconnected) => None,
                    Ok(SystemEvent::RenameTo { path: to }) if from.parent() == to.parent() => {
                        Some(Event::Rename { from, to })
                    }
                    Ok(SystemEvent::RenameTo { path: to }) => Some(Event::Move { from, to }),
                    other_event_or_timeout => {
                        if let Ok(other_event) = other_event_or_timeout {
                            self.buffered = Some(other_event);
                        }
                        Some(Event::Delete { path: from })
                    }
                }
            }
            SystemEvent::Remove { path } => Some(Event::Delete { path }),
            SystemEvent::RenameTo { path } => Some(Event::Create { path }),
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
enum SystemEvent {
    Create { path: PathBuf },
    Remove { path: PathBuf },
    RenameFrom { path: PathBuf },
    RenameTo { path: PathBuf },
}
impl SystemEvent {
    fn try_map(event: notify::Result<RawEvent>) -> Option<Self> {
        let mut event = event.inspect_err(|e| error!("{e:#}")).ok()?;
        let path = event
            .paths
            .drain(..)
            .next()
            .expect("no events without a path");
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
