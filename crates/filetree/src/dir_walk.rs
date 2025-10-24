use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn read_dir(
    path: impl AsRef<Path>,
) -> std::io::Result<impl Iterator<Item = std::io::Result<DirEntry>>> {
    Ok(std::fs::read_dir(path)?.map(|entry| entry.and_then(DirEntry::try_from)))
}

pub fn visit_dir(
    path: impl AsRef<Path>,
    mut visit_fn: impl FnMut(io::Result<DirEntry>) -> bool,
) -> io::Result<()> {
    let mut iters = vec![fs::read_dir(path)?];
    while let Some(iter) = iters.last_mut() {
        let entry = match iter.next() {
            Some(entry) => entry.and_then(DirEntry::try_from),
            None => {
                iters.pop();
                continue;
            }
        };
        let path = if let Ok(DirEntry::Dir { ref path }) = entry {
            Some(path.to_path_buf())
        } else {
            None
        };
        let visit = visit_fn(entry);
        if let Some(path) = path
            && visit
        {
            iters.push(fs::read_dir(path)?);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirEntry {
    File { path: PathBuf },
    Dir { path: PathBuf },
}
impl DirEntry {
    pub fn path(&self) -> &Path {
        match self {
            DirEntry::File { path } => path,
            DirEntry::Dir { path } => path,
        }
    }
    pub fn into_path_buf(self) -> PathBuf {
        match self {
            DirEntry::File { path } => path,
            DirEntry::Dir { path } => path,
        }
    }
}
impl TryFrom<fs::DirEntry> for DirEntry {
    type Error = io::Error;
    fn try_from(dir_entry: fs::DirEntry) -> Result<Self, Self::Error> {
        let file_type = dir_entry.file_type()?;
        let path = dir_entry.path().canonicalize()?;
        if file_type.is_file() {
            return Ok(DirEntry::File { path });
        }
        if file_type.is_dir() {
            return Ok(DirEntry::Dir { path });
        }
        Err(io::Error::other(format!(
            "path `{}` was not a file or directory",
            path.display()
        )))
    }
}
impl TryFrom<PathBuf> for DirEntry {
    type Error = io::Error;
    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let file_type = std::fs::metadata(&path)?.file_type();
        if file_type.is_file() {
            return Ok(DirEntry::File { path });
        }
        if file_type.is_dir() {
            return Ok(DirEntry::Dir { path });
        }
        Err(io::Error::other(format!(
            "path `{}` was not a file or directory",
            path.display()
        )))
    }
}
