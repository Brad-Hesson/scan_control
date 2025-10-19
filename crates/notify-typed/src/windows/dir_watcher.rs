use std::{
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    pin::Pin,
    task::Poll,
    time::Duration,
};

use async_stream::stream;
use binrw::BinRead;
use futures::{Stream, StreamExt};
use futures::{
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    stream::Peekable,
};
use tokio::{pin, time::timeout};
use tracing::error;
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

pub struct DirWatcher {
    event_parser: Pin<Box<dyn Stream<Item = Event>>>,
    thread_pool: ThreadPoolIO<Callback>,
    base_path: PathBuf,
}
impl DirWatcher {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let handle = DirHandle::new(&path)?;
        let (sender, recv) = mpsc::unbounded();
        let callback = Callback::new(handle, sender);
        let mut thread_pool = ThreadPoolIO::new(handle, callback)?;
        thread_pool.start();
        thread_pool.callback.start_read();
        let event_parser = Box::pin(event_parser(recv, path.as_ref().to_path_buf()));
        Ok(Self {
            event_parser,
            thread_pool,
            base_path: path.as_ref().to_path_buf(),
        })
    }
}
impl Stream for DirWatcher {
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
        let rx = rx.peekable();
        pin!(rx);
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
                            continue;
                        }
                    };
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
                    error!("got unexpected event: {other_action:?}");
                }
            }
        }
    }
}

async fn expect_action(
    mut rx: Pin<&mut Peekable<UnboundedReceiver<ActionPacket>>>,
    action_type: ActionType,
) -> Option<Result<ActionPacket, ()>> {
    let packet = rx.as_mut().peek().await?;
    if packet.action_type() == action_type {
        return Some(Ok(rx.next().await.unwrap()));
    }
    return Some(Err(()));
}

#[repr(C, align(4))]
struct ReadBuffer {
    buf: [u8; READ_BUFFER_SIZE],
}

struct Callback {
    handle: DirHandle,
    sender: UnboundedSender<ActionPacket>,
    overlapped: Overlapped,
    read_buffer: ReadBuffer,
}
impl Callback {
    fn new(handle: DirHandle, sender: UnboundedSender<ActionPacket>) -> Self {
        Self {
            handle,
            sender,
            overlapped: Overlapped::new(),
            read_buffer: unsafe { std::mem::MaybeUninit::uninit().assume_init() },
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
    next_offset: u32,
    action: u32,
    name_len: u32,
    #[br(count = name_len / 2)]
    name: Vec<u16>,
}
impl ActionPacket {
    fn byte_len(&self) -> usize {
        12 + self.name_len as usize
    }
    fn action_type(&self) -> ActionType {
        ActionType::try_from(self.action).expect("invalid action type")
    }
    fn path(&self) -> OsString {
        OsString::from_wide(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Action {
    action_type: ActionType,
    path: PathBuf,
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
    use futures::StreamExt;
    use tokio::{io::AsyncWriteExt, pin};

    use super::*;

    #[test]
    fn test_name() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(test_name_async())
    }
    async fn test_name_async() {
        let watcher = DirWatcher::new("./data").unwrap();
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
