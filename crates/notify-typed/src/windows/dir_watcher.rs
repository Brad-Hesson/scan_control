use std::{
    ffi::OsString,
    io::Cursor,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
    task::Poll,
};

use bbqueue::{BBBuffer, Consumer, GrantW, Producer};
use binrw::BinRead;
use futures::{Stream, task::AtomicWaker};
use windows::core::Error;

use crate::windows::{
    handle::{DirHandle, Filter},
    threadpool_io::{ThreadPoolCallback, ThreadPoolIO},
};

const BUFFER_SIZE: usize = 64 * 1024;
const GRANT_SIZE: usize = 1024;

pub struct DirWatcher {
    buffer: Box<BBBuffer<BUFFER_SIZE>>,
    cons: Consumer<'static, BUFFER_SIZE>,
    thread_pool: ThreadPoolIO<Callback>,
    waker: Arc<AtomicWaker>,
    base_path: PathBuf,
}
impl DirWatcher {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let buffer = Box::new(BBBuffer::new());
        let (prod, cons) = unsafe { NonNull::from_ref(&*buffer).as_ref() }
            .try_split()
            .expect("buffer should not already be split");
        let handle = DirHandle::new(&path)?;
        let waker = Arc::new(AtomicWaker::new());
        let callback = Callback::new(
            handle,
            prod,
            waker.clone(),
            Filter::DirCRD | Filter::FileCRD,
        );
        let mut thread_pool = ThreadPoolIO::new(handle, callback)?;
        thread_pool.start();
        thread_pool.callback.start_read();
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
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.waker.register(cx.waker());
        if let Ok(read) = self.cons.read() {
            let mut reader = Cursor::new(read.buf());
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
    prod: Producer<'static, BUFFER_SIZE>,
    grant: Option<GrantW<'static, BUFFER_SIZE>>,
    waker: Arc<AtomicWaker>,
    filter: Filter,
}
impl Callback {
    fn new(
        handle: DirHandle,
        prod: Producer<'static, BUFFER_SIZE>,
        waker: Arc<AtomicWaker>,
        filter: Filter,
    ) -> Self {
        Self {
            handle,
            prod,
            grant: None,
            waker,
            filter,
        }
    }
    fn start_read(&mut self) {
        self.grant = Some(self.prod.grant_max_remaining(BUFFER_SIZE).unwrap());
        if self.grant.as_ref().unwrap().len() < GRANT_SIZE {
            drop(self.grant.take().unwrap());
            drop(self.prod.grant_exact(GRANT_SIZE).unwrap());
            self.grant = Some(self.prod.grant_max_remaining(BUFFER_SIZE).unwrap());
        }
        dbg!(self.grant.as_ref().unwrap().len());
        self.handle
            .read_dir_changes_overlapped(
                self.grant.as_mut().expect("just loaded the grant"),
                true,
                self.filter,
            )
            .unwrap();
    }
}
impl ThreadPoolCallback for Callback {
    fn call(&mut self, bytes_written: Result<usize, u32>) {
        let num_bytes = bytes_written.unwrap();
        let grant = self.grant.take().unwrap();
        grant.commit(dword_align(num_bytes));
        self.start_read();
        self.waker.wake();
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
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::pin;

    use super::*;

    #[test]
    fn test_name() {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(test_name_async())
    }
    async fn test_name_async() {
        let watcher = DirWatcher::new("./data").unwrap();
        tokio::spawn(async {
            pin!(watcher);
            while let Some(action) = watcher.next().await {
                eprintln!("{action:?}");
            }
        });
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            eprint!("|");
        }
    }
}

fn dword_align(n: usize) -> usize {
    n.div_ceil(4) * 4
}
