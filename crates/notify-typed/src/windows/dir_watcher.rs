use std::{
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    sync::Arc,
    task::Poll,
};

use bbq2::{
    prod_cons::stream::{StreamConsumer, StreamGrantW, StreamProducer},
    queue::{ArcBBQueue, BBQueue},
    traits::{notifier::maitake::MaiNotSpsc, storage::BoxedSlice},
};
use binrw::BinRead;
use futures::{Stream, task::AtomicWaker};
use windows::core::Error;

use crate::windows::{
    atomic_coord_min::AtomicCoordMin,
    handle::{DirHandle, Filter, Overlapped},
    threadpool_io::{ThreadPoolCallback, ThreadPoolIO},
};

const BUFFER_SIZE: usize = 64 * 1024;
const GRANT_SIZE: usize = 1024;
type Buffer = ArcBBQueue<BoxedSlice, AtomicCoordMin, MaiNotSpsc>;
type BufferHandle = Arc<BBQueue<BoxedSlice, AtomicCoordMin, MaiNotSpsc>>;

pub struct DirWatcher {
    buffer: Buffer,
    cons: StreamConsumer<BufferHandle>,
    thread_pool: ThreadPoolIO<Callback>,
    waker: Arc<AtomicWaker>,
    base_path: PathBuf,
}
impl DirWatcher {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let handle = DirHandle::new(&path)?;
        let buffer = Buffer::new_with_storage(BoxedSlice::new(BUFFER_SIZE));
        let waker = Arc::new(AtomicWaker::new());
        let callback = Callback::new(handle, buffer.stream_producer(), waker.clone());
        let mut thread_pool = ThreadPoolIO::new(handle, callback)?;
        thread_pool.start();
        thread_pool.callback.start_read();
        let cons = buffer.stream_consumer();
        Ok(Self {
            buffer,
            cons,
            thread_pool,
            waker,
            base_path: path.as_ref().to_path_buf(),
        })
    }
}
impl Stream for DirWatcher {
    type Item = ActionPacket;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.waker.register(cx.waker());
        if let Ok(read) = self.cons.read() {
            let mut reader = Cursor::new(&*read);
            if let Ok(action) = ActionPacket::read_ne(&mut reader) {
                read.release(dword_align(action.byte_len()));
                // let action = Action {
                //     action_type: ActionType::try_from(action.action).unwrap(),
                //     path: self.base_path.join(action.name),
                // };
                return Poll::Ready(Some(action));
            }
            read.release(0);
        }
        Poll::Pending
    }
}

struct Callback {
    handle: DirHandle,
    prod: StreamProducer<BufferHandle>,
    grant: Option<StreamGrantW<BufferHandle>>,
    waker: Arc<AtomicWaker>,
    overlapped: Overlapped,
}
impl Callback {
    fn new(handle: DirHandle, prod: StreamProducer<BufferHandle>, waker: Arc<AtomicWaker>) -> Self {
        Self {
            handle,
            prod,
            grant: None,
            waker,
            overlapped: Overlapped::new(),
        }
    }
    fn start_read(&mut self) {
        self.grant = Some(self.prod.grant_max_remaining(GRANT_SIZE).unwrap());
        self.handle
            .read_dir_changes_overlapped(
                self.grant.as_mut().expect("just loaded the grant"),
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
        let grant = self.grant.take().unwrap();
        grant.commit(dword_align(num_bytes));
        self.waker.wake();
        self.start_read();
    }
}

#[derive(Debug, binrw::BinRead)]
pub struct ActionPacket {
    next_offset: u32,
    #[br(map = |v: u32| ActionType::try_from(v).expect("undefined action type"))]
    action: ActionType,
    name_len: u32,
    #[br(map = |v: Vec<u16>| OsString::from_wide(&v))]
    #[br(count = name_len / 2)]
    name: OsString,
}
impl ActionPacket {
    fn byte_len(&self) -> usize {
        12 + self.name_len as usize
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

fn dword_align(n: usize) -> usize {
    n.div_ceil(4) * 4
}
