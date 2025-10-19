use std::{
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use async_stream::stream;
use binrw::BinRead;
use futures::{
    Stream, StreamExt,
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    pin_mut,
    stream::Peekable,
};
use tracing::{error, trace};
use windows::core::Error;

use crate::{
    Event,
    windows::{
        handle::{DirHandle, Filter, Overlapped},
        threadpool_io::{ThreadPoolCallback, ThreadPoolIO},
    },
};

const READ_BUFFER_SIZE: usize = 16 * 1024;
const DELETE_TIMEOUT: Duration = Duration::from_millis(100);

pub struct DirEventStream {
    event_parser: Pin<Box<dyn Stream<Item = Event>>>,
    _thread_pool: ThreadPoolIO<Callback>,
}
impl DirEventStream {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let handle = Arc::new(DirHandle::new(&path)?);
        let (sender, recv) = mpsc::unbounded();
        let mut _thread_pool = ThreadPoolIO::new(&handle.clone(), Callback::new(handle, sender))?;
        _thread_pool.start();
        _thread_pool.callback.start_read();
        let event_parser = Box::pin(
            event_parser(recv, path.as_ref().to_path_buf())
                .inspect(|event| trace!("parsed dir event: {event:?}")),
        );
        Ok(Self {
            event_parser,
            _thread_pool,
        })
    }
}
impl Stream for DirEventStream {
    type Item = Event;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.event_parser.poll_next_unpin(cx)
    }
}

fn event_parser(rx: UnboundedReceiver<ActionPacket>, base: PathBuf) -> impl Stream<Item = Event> {
    stream! {
        let rx = rx.inspect(|packet| {
            trace!("system dir event: {:?} : {}", packet.action_type(), packet.path().display());
        }).peekable();
        pin_mut!(rx);
        while let Some(packet) = rx.next().await {
            match packet.action_type() {
                ActionType::Added => {
                    yield Event::Create {
                        path: base.join(packet.path()),
                    }
                }
                ActionType::RenamedOld => {
                    let from = base.join(packet.path());
                    match expect_action(rx.as_mut(), ActionType::RenamedNew).await {
                        None => return,
                        Some(Ok(packet)) => {
                            yield Event::Rename {
                                from,
                                to: base.join(packet.path()),
                            }
                        }
                        Some(Err(())) => {
                            error!("got rename event without new path");
                        }
                    }
                }
                ActionType::Removed => {
                    let from = base.join(packet.path());
                    match timeout(
                        DELETE_TIMEOUT,
                        expect_action(rx.as_mut(), ActionType::Added),
                    )
                    .await
                    {
                        Ok(None) => return,
                        Ok(Some(Ok(packet))) => {
                            yield Event::Move {
                                from,
                                to: base.join(packet.path()),
                            }
                        }
                        _ => yield Event::Delete { path: from },
                    }
                }
                other_action => {
                    error!("got unexpected event: {other_action:?} : {}", packet.path().display());
                }
            }
        }
    }
}

async fn expect_action<S: Stream<Item = ActionPacket>>(
    mut rx: Pin<&mut Peekable<S>>,
    action_type: ActionType,
) -> Option<Result<ActionPacket, ()>> {
    let packet = rx.as_mut().peek().await?;
    if packet.action_type() == action_type {
        return Some(Ok(rx.next().await.expect("just peeked the packet")));
    }
    Some(Err(()))
}

async fn timeout<I>(dur: Duration, fut: impl IntoFuture<Output = I>) -> Result<I, ()> {
    let del = futures_timer::Delay::new(dur);
    let fut = fut.into_future();
    pin_mut!(fut);
    match futures::future::select(fut, del).await {
        futures::future::Either::Left((out, _)) => Ok(out),
        futures::future::Either::Right(_) => Err(()),
    }
}

#[repr(C, align(4))]
struct ReadBuffer {
    buf: [u8; READ_BUFFER_SIZE],
}

struct Callback {
    handle: Arc<DirHandle>,
    sender: UnboundedSender<ActionPacket>,
    overlapped: Overlapped,
    read_buffer: ReadBuffer,
}
impl Callback {
    fn new(handle: Arc<DirHandle>, sender: UnboundedSender<ActionPacket>) -> Self {
        Self {
            handle,
            sender,
            overlapped: Overlapped::new(),
            read_buffer: unsafe { std::mem::MaybeUninit::zeroed().assume_init() },
        }
    }
    fn start_read(&mut self) {
        self.handle
            .read_dir_changes_overlapped(
                &mut self.read_buffer.buf,
                true,
                Filter::DirCRD | Filter::FileCRD,
                &mut self.overlapped,
            )
            .unwrap();
    }
}
impl ThreadPoolCallback for Callback {
    fn call(&mut self, bytes_written: Result<usize, u32>) {
        let num_bytes = bytes_written.unwrap();
        let mut buf = Cursor::new(&self.read_buffer.buf[..num_bytes]);
        while let Ok(packet) = ActionPacket::read_ne(&mut buf) {
            let send_resp = self.sender.unbounded_send(packet);
            if send_resp.is_err() {
                return;
            }
        }
        self.start_read();
    }
}

#[derive(Debug, binrw::BinRead)]
pub struct ActionPacket {
    #[br(align_before(4))]
    _next_offset: u32,
    action: u32,
    _name_len: u32,
    #[br(count = _name_len / 2)]
    name: Vec<u16>,
}
impl ActionPacket {
    fn action_type(&self) -> ActionType {
        ActionType::try_from(self.action).expect("invalid action type")
    }
    fn path(&self) -> OsString {
        OsString::from_wide(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::TryFromPrimitive)]
#[repr(u32)]
pub enum ActionType {
    Added = 0x1,
    Removed = 0x2,
    Modified = 0x3,
    RenamedOld = 0x4,
    RenamedNew = 0x5,
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Waker};

    use futures::StreamExt;
    use tokio::{io::AsyncWriteExt, pin};
    use tracing_subscriber::EnvFilter;

    use super::*;

    #[test]
    fn blocking() {
        let mut watcher = DirEventStream::new("./data").unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            while let Poll::Ready(Some(event)) = watcher.poll_next_unpin(&mut cx) {
                println!("{event:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[test]
    fn test_name() {
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
