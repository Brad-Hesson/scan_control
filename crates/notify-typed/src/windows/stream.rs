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
    FutureExt, Stream, StreamExt,
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    pin_mut, select_biased,
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

fn event_parser(
    rx: UnboundedReceiver<ActionPacketExt>,
    base: PathBuf,
) -> impl Stream<Item = Event> {
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
                    match next_if(rx.as_mut(), |p| p.action_type() == ActionType::RenamedNew).await {
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
                    let id = packet.file_id;
                    match timeout(
                        DELETE_TIMEOUT,
                        next_if(rx.as_mut(), |p| p.action_type() == ActionType::Added && p.file_id == id),
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

async fn next_if<S: Stream<Item = ActionPacketExt>>(
    mut rx: Pin<&mut Peekable<S>>,
    f: impl FnOnce(&ActionPacketExt) -> bool,
) -> Option<Result<ActionPacketExt, ()>> {
    let packet = rx.as_mut().peek().await?;
    if f(packet) {
        return Some(Ok(rx.next().await.expect("just peeked the packet")));
    }
    Some(Err(()))
}

async fn timeout<I>(dur: Duration, fut: impl IntoFuture<Output = I>) -> Result<I, ()> {
    let mut del = futures_timer::Delay::new(dur).fuse();
    let fut = fut.into_future().fuse();
    pin_mut!(fut);
    select_biased! {
        _ = del => Err(()),
        out = fut => Ok(out)
    }
}

#[repr(C, align(4))]
struct ReadBuffer {
    buf: [u8; READ_BUFFER_SIZE],
}

struct Callback {
    handle: Arc<DirHandle>,
    sender: UnboundedSender<ActionPacketExt>,
    overlapped: Overlapped,
    read_buffer: ReadBuffer,
}
impl Callback {
    fn new(handle: Arc<DirHandle>, sender: UnboundedSender<ActionPacketExt>) -> Self {
        Self {
            handle,
            sender,
            overlapped: Overlapped::new(),
            read_buffer: unsafe { std::mem::MaybeUninit::zeroed().assume_init() },
        }
    }
    fn start_read(&mut self) {
        self.handle
            .read_dir_changes_ex_overlapped(
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
        while let Ok(packet) = ActionPacketExt::read_ne(&mut buf) {
            let send_resp = self.sender.unbounded_send(packet);
            if send_resp.is_err() {
                return;
            }
        }
        self.start_read();
    }
}

#[derive(Debug, binrw::BinRead)]
pub struct ActionPacketExt {
    #[br(align_before(4))]
    _next_offset: u32,
    action: u32,
    _creation_time: i64,
    _last_mod_time: i64,
    _last_change_time: i64,
    _last_access_time: i64,
    _allocated_length: i64,
    _file_size: i64,
    _file_attrs: u32,
    _dummy_union: u32,
    file_id: i64,
    _parent_file_id: i64,
    _name_len: u32,
    #[br(count = _name_len / 2)]
    name: Vec<u16>,
}
impl ActionPacketExt {
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
