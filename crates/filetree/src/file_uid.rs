use std::{
    cell::Cell,
    fmt::Debug,
    hash::{BuildHasher, Hash, Hasher},
    io::Read,
    path::Path,
    u64,
};

use twox_hash::XxHash3_64;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ContentID {
    hash: u64,
}
impl Hash for ContentID {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}
impl Debug for ContentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("ContentID({:0>16x})", self.hash))
    }
}

pub struct FileHasher {
    buf: Box<[u8]>,
}
impl FileHasher {
    pub fn new(byte_limit: usize) -> Self {
        Self {
            buf: unsafe { Box::new_uninit_slice(byte_limit).assume_init() },
        }
    }
    pub fn hash_file(&mut self, path: impl AsRef<Path>) -> std::io::Result<ContentID> {
        let mut file = std::fs::File::options().read(true).open(path)?;
        let read_len = self.buf.len().min(file.metadata()?.len() as usize);
        file.read_exact(&mut self.buf[..read_len])?;
        let id = XxHash3_64::oneshot(&self.buf[..read_len]);
        Ok(ContentID { hash: id })
    }
}
impl Default for FileHasher {
    fn default() -> Self {
        Self::new(16 * 1024)
    }
}

pub struct IdentityHasher;
impl BuildHasher for IdentityHasher {
    type Hasher = IdentityHasherInner;

    fn build_hasher(&self) -> Self::Hasher {
        IdentityHasherInner(Cell::default())
    }
}
pub struct IdentityHasherInner(Cell<Option<u64>>);
impl Hasher for IdentityHasherInner {
    fn finish(&self) -> u64 {
        self.0
            .take()
            .expect("IdentityHasher was given nothing before finish")
    }

    fn write(&mut self, _bytes: &[u8]) {
        panic!("IdentityHasher can only hash u64s");
    }

    fn write_u64(&mut self, i: u64) {
        if self.0.replace(Some(i)).is_some() {
            panic!("IdentityHasher was given multiple u64s before finish");
        }
    }
}
