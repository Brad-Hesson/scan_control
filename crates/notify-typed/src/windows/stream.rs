use std::{
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    pin::Pin,
    ptr::addr_of_mut,
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
use pin_project::pin_project;
use tracing::{error, trace};
use windows::core::Error;

use crate::{
    Event,
    windows::bindings::{
        handle::{DirChangesBuffer, DirHandle, Filter, Overlapped},
        threadpool_io::{BytesWritten, ThreadPoolCallback, ThreadPoolIO},
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
        let (tx, rx) = mpsc::unbounded();
        let callback = Pin::from(Callback::new_boxed(handle.clone(), tx));
        let mut thread_pool = ThreadPoolIO::new(&handle, callback)?;
        thread_pool.start();
        thread_pool.callback.as_mut().start_read();
        let event_parser = Box::pin(event_parser(rx, path.as_ref().to_path_buf()));
        Ok(Self {
            event_parser,
            _thread_pool: thread_pool,
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
    }.inspect(|event| trace!("parsed dir event: {event:?}"))
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
        out = fut => Ok(out),
        _ = del => Err(()),
    }
}

#[pin_project]
struct Callback {
    handle: Arc<DirHandle>,
    sender: UnboundedSender<ActionPacketExt>,
    #[pin]
    overlapped: Overlapped,
    #[pin]
    read_buffer: DirChangesBuffer<READ_BUFFER_SIZE>,
}
impl Callback {
    fn new_boxed(handle: Arc<DirHandle>, sender: UnboundedSender<ActionPacketExt>) -> Box<Self> {
        let mut uninit = Box::<Self>::new_uninit();
        let p = uninit.as_mut_ptr();
        unsafe {
            addr_of_mut!((*p).handle).write(handle);
            addr_of_mut!((*p).sender).write(sender);
            addr_of_mut!((*p).overlapped).write(Overlapped::new());
            // read_buffer can be uninitialized
            uninit.assume_init()
        }
    }
    fn start_read(self: Pin<&mut Self>) {
        let this = self.project();
        this.handle
            .read_dir_changes_ex_overlapped(
                this.read_buffer,
                true,
                Filter::DirCRD | Filter::FileCRD,
                this.overlapped,
            )
            .unwrap();
    }
}
impl ThreadPoolCallback for Callback {
    fn call(self: Pin<&mut Self>, bytes_written: Result<&BytesWritten, u32>) {
        let num_bytes = bytes_written.unwrap();
        let mut buf = Cursor::new(self.read_buffer.read(num_bytes));
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