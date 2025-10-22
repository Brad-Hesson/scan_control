use std::{hash::Hash, io::Read, path::Path, u64};

use twox_hash::XxHash3_64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ContentID {
    hash: u64,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dir_walk::{self, DirEntry};

    #[test]
    fn file_id() {
        let mut paths = vec![];
        dir_walk::visit_dir("../../data", |entry| {
            if let Ok(DirEntry::File { path }) = entry {
                paths.push(path);
            };
            true
        })
        .unwrap();
        let mut hasher = FileHasher::default();
        let start = std::time::Instant::now();
        for i in 0.. {
            hasher.hash_file(&paths[i % paths.len()]).unwrap();
            if i.is_multiple_of(500) {
                let dur = start.elapsed();
                println!("files per second: {}", i as f64 / dur.as_secs_f64());
            }
        }
    }
}
