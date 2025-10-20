use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    pin::Pin,
    task::Poll,
    time::Duration,
};

use async_stream::stream;
use futures::{
    FutureExt as _, Stream, StreamExt, pin_mut, select_biased,
    stream::{Peekable, empty},
};
use inotify::{EventMask, EventStream, Inotify, WatchDescriptor, WatchMask};
use pin_project::pin_project;
use tracing::error;

use crate::Event;

const BUFFER_SIZE: usize = 16 * 1024;
const DELETE_TIMEOUT: Duration = Duration::from_millis(100);
const MASK: WatchMask = WatchMask::from_bits_truncate(
    WatchMask::CREATE.bits() | WatchMask::DELETE.bits() | WatchMask::MOVE.bits(),
);

pub struct DirEventStream {
    notifier: Pin<Box<dyn Stream<Item = Event>>>,
}
impl DirEventStream {
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let notifier = inotify::Inotify::init()?;
        let mut base_map = BTreeMap::new();
        let wd = notifier.watches().add(&path, MASK)?;
        base_map.insert(wd, path.as_ref().to_path_buf());
        let notifier = notifier.into_event_stream([0u8; BUFFER_SIZE])?;
        let notifier = Box::pin(event_parser(watch_manager(notifier, base_map)));
        Ok(Self { notifier })
    }
}
impl Stream for DirEventStream {
    type Item = Event;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.notifier.poll_next_unpin(cx)
    }
}

fn event_parser(rx: impl Stream<Item = std::io::Result<SystemEvent>>) -> impl Stream<Item = Event> {
    stream! {
        let rx = rx.map(|event| event.unwrap()).peekable();
        pin_mut!(rx);
        while let Some(event) = rx.next().await{
            if event.mask.contains(EventMask::CREATE){
                yield Event::Create { path: event.path }
            } else if event.mask.contains(EventMask::DELETE){
                yield Event::Delete { path: event.path }
            } else if event.mask.contains(EventMask::MOVED_FROM){
                let from = event.path;
                let event = timeout(DELETE_TIMEOUT, next_if(rx.as_mut(), |e| e.mask.contains(EventMask::MOVED_TO))).await;
                match event {
                    Ok(None) => return,
                    Ok(Some(Ok(event))) if event.path.parent() == from.parent() => {
                        yield Event::Rename { from, to: event.path }
                    },
                    Ok(Some(Ok(event))) => {
                        yield Event::Move { from, to: event.path }
                    },
                    other_event_or_timeout => {
                        if other_event_or_timeout.is_ok(){
                            error!("got move event without destination");
                        }
                        continue;
                    },
                }
            } else if event.mask.contains(EventMask::MOVED_TO){
                yield Event::Create { path: event.path }
            }
        }
    }
}

fn watch_manager(
    rx: EventStream<[u8; BUFFER_SIZE]>,
    mut base_map: BTreeMap<WatchDescriptor, PathBuf>,
) -> impl Stream<Item = std::io::Result<SystemEvent>> {
    stream! {
        pin_mut!(rx);
        while let Some(packet) = rx.next().await {
            match packet{
                Ok(packet) => {
                    let path = base_map.get(&packet.wd).unwrap().join(packet.name.unwrap());
                    if packet.mask.contains(EventMask::ISDIR){
                        if packet.mask.intersects(EventMask::CREATE){
                            let wd = rx.watches().add(&path, MASK).unwrap();
                            base_map.insert(wd, path.clone());
                        }
                        if packet.mask.intersects(EventMask::MOVED_TO){
                            *base_map.get_mut(&packet.wd).unwrap() = path.clone();
                        }
                        if packet.mask.intersects(EventMask::DELETE){
                            base_map.remove(&packet.wd);
                            rx.watches().remove(packet.wd).unwrap();
                        }
                    }
                    yield Ok(SystemEvent{ path, mask: packet.mask })
                }
                Err(e) => yield Err(e)
            }
        }
    }
}

struct SystemEvent {
    path: PathBuf,
    mask: EventMask,
}

async fn timeout<I>(dur: Duration, fut: impl IntoFuture<Output = I>) -> Result<I, TimeoutError> {
    let mut del = futures_timer::Delay::new(dur).fuse();
    let fut = fut.into_future().fuse();
    pin_mut!(fut);
    select_biased! {
        out = fut => Ok(out),
        _ = del => Err(TimeoutError),
    }
}
struct TimeoutError;

async fn next_if<S: Stream>(
    mut rx: Pin<&mut Peekable<S>>,
    f: impl FnOnce(&S::Item) -> bool,
) -> Option<Result<S::Item, NextIfError>> {
    let packet = rx.as_mut().peek().await?;
    if f(packet) {
        return Some(Ok(rx.next().await.expect("just peeked the packet")));
    }
    Some(Err(NextIfError))
}
struct NextIfError;
