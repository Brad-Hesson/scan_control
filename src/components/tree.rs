use std::{
    collections::{btree_map, BTreeMap},
    ffi::OsString,
    fs::ReadDir,
    iter,
    path::{Path, PathBuf},
};

use eyre::Context;

use crate::app::OkTraceExt;

pub struct FileTree<T> {
    files: BTreeMap<PathBuf, T>,
    load_fn: Box<dyn Fn(&Path) -> Option<T>>,
}
impl<T> FileTree<T> {
    pub fn new(load_fn: impl Fn(&Path) -> Option<T> + 'static) -> Self {
        Self {
            files: BTreeMap::new(),
            load_fn: Box::new(load_fn),
        }
    }
    pub fn load_path(&mut self, path: impl AsRef<Path>) {
        let Some(iter) = WalkDirIter::new(path).ok_trace() else {
            return;
        };
        for path in iter {
            if let Some(data) = (self.load_fn)(&path) {
                self.files.insert(path, data);
            }
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = File<'_, T>> + ExactSizeIterator {
        self.files.iter().map(|(path, data)| File { path, data })
    }
    pub fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = FileMut<'_, T>> + ExactSizeIterator {
        self.files
            .iter_mut()
            .map(|(path, data)| FileMut { path, data })
    }
    pub fn iter_items(&self) -> impl Iterator<Item = Item<'_, T>> {
        self.files
            .iter()
            .scan(PathBuf::new(), |current_folder, (path, data)| todo!())
    }
}

pub struct ItemIter<'a, T> {
    root: PathBuf,
    folder_stack: Vec<OsString>,
    iter: iter::Peekable<btree_map::Iter<'a, PathBuf, T>>,
}
impl<'a, T> ItemIter<'a, T> {
    fn new(map: &'a BTreeMap<PathBuf, T>, root: PathBuf) -> Self {
        Self {
            root,
            iter: map.iter().peekable(),
            folder_stack: Vec::new(),
        }
    }
}
impl<'a, T> Iterator for ItemIter<'a, T> {
    type Item = Item<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let (path, _) = self.iter.peek()?;
        let next_parent = path.strip_prefix(&self.root).unwrap().parent().unwrap();

        todo!()
    }
}

pub struct File<'a, T> {
    path: &'a Path,
    data: &'a T,
}
pub struct FileMut<'a, T> {
    path: &'a Path,
    data: &'a mut T,
}

pub enum Item<'a, T> {
    File { file: File<'a, T> },
    Folder { path: &'a Path },
}

pub enum ItemMut<'a, T> {
    File { file: FileMut<'a, T> },
    Folder { path: &'a Path },
}

enum Entry<T> {
    File { data: T },
    Folder,
}

struct WalkDirIter {
    iter: ReadDir,
    sub_dir: Option<Box<WalkDirIter>>,
}
impl WalkDirIter {
    fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self {
            iter: std::fs::read_dir(path)?,
            sub_dir: None,
        })
    }
}
impl Iterator for WalkDirIter {
    type Item = PathBuf;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sub_dir) = self.sub_dir.as_mut() {
            match sub_dir.next() {
                Some(ret) => return Some(ret),
                None => {
                    self.sub_dir = None;
                    return self.next();
                }
            }
        }
        let entry = self
            .iter
            .next()?
            .context("failed to construct dir entry")
            .ok_trace()?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to get file type for `{}`", path.display()))
            .ok_trace()?;
        if file_type.is_file() {
            return Some(path);
        }
        if file_type.is_dir() {
            if let Some(sub_dir) = Self::new(path).ok_trace() {
                self.sub_dir = Some(Box::new(sub_dir));
            }
            return self.next();
        }
        tracing::error!("`{}` was not a file or directory", path.display());
        return self.next();
    }
}
