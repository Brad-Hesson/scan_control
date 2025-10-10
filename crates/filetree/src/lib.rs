use std::{
    cell::{Ref, RefCell, RefMut},
    ops::Index,
    path::{Path, PathBuf},
    usize,
};

use crate::dir_walk::{DirEntry, visit_dir};

mod dir_walk;

pub struct FileTree<F, D> {
    entries: Vec<Entry<F, D>>,
    file_load_fn: Box<dyn Fn(&Path) -> Option<F>>,
    dir_load_fn: Box<dyn Fn(&Path) -> Option<D>>,
}
impl<F, D> FileTree<F, D> {
    pub fn new(
        file_load_fn: impl Fn(&Path) -> Option<F> + 'static,
        dir_load_fn: impl Fn(&Path) -> Option<D> + 'static,
    ) -> Self {
        Self {
            entries: Vec::new(),
            dir_load_fn: Box::new(dir_load_fn),
            file_load_fn: Box::new(file_load_fn),
        }
    }
    pub fn load_dir(&mut self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut new_entries = Vec::new();
        if let Some(data) = (self.dir_load_fn)(path.as_ref()) {
            new_entries.push(Entry::new_dir(path.as_ref().to_path_buf(), data));
        } else {
            return Ok(());
        };
        visit_dir(&path, |entry| {
            let Some(entry) = entry.ok_trace() else {
                return false;
            };
            match entry {
                DirEntry::File { path } => {
                    if let Some(data) = (self.file_load_fn)(&path) {
                        new_entries.push(Entry::new_file(path, data));
                        return true;
                    };
                }
                DirEntry::Dir { path } => {
                    if let Some(data) = (self.dir_load_fn)(&path) {
                        new_entries.push(Entry::new_dir(path, data));
                        return true;
                    };
                }
            }
            return false;
        })?;
        new_entries.sort_by(|e1, e2| e1.path().cmp(e2.path()));
        self.entries = new_entries;
        Ok(())
    }
    pub fn root(&self) -> Option<DirRef<'_, D>> {
        self.entries
            .first()
            .map(|entry| entry.as_dir().expect("root should be a dir").borrow())
    }
    pub fn root_mut(&self) -> Option<DirRefMut<'_, D>> {
        self.entries
            .first()
            .map(|entry| entry.as_dir().expect("root should be a dir").borrow_mut())
    }
    pub fn get(&self, path: impl AsRef<Path>) -> Option<EntryRef<'_, F, D>> {
        self.index_of(path).map(|i| self.entries[i].borrow())
    }
    pub fn get_mut(&self, path: impl AsRef<Path>) -> Option<EntryRefMut<'_, F, D>> {
        self.index_of(path).map(|i| self.entries[i].borrow_mut())
    }
    pub fn get_index(&self, i: usize) -> Option<EntryRef<'_, F, D>> {
        self.entries.get(i).map(Entry::borrow)
    }
    pub fn get_index_mut(&self, i: usize) -> Option<EntryRefMut<'_, F, D>> {
        self.entries.get(i).map(Entry::borrow_mut)
    }
    pub fn iter(&self) -> impl Iterator<Item = EntryRef<'_, F, D>> {
        self.entries.iter().map(Entry::borrow)
    }
    pub fn iter_mut(&self) -> impl Iterator<Item = EntryRefMut<'_, F, D>> {
        self.entries.iter().map(Entry::borrow_mut)
    }
    pub fn iter_files(&self) -> impl DoubleEndedIterator<Item = FileRef<'_, F>> {
        self.entries
            .iter()
            .filter_map(|entry| entry.as_file().map(File::borrow))
    }
    pub fn iter_files_mut(&self) -> impl DoubleEndedIterator<Item = FileRefMut<'_, F>> {
        self.entries
            .iter()
            .filter_map(|entry| entry.as_file().map(File::borrow_mut))
    }
    pub fn parent_of(&self, path: impl AsRef<Path>) -> Option<DirRef<'_, D>> {
        let parent_path = path.as_ref().parent().unwrap();
        let index = self.index_of(&path)?;
        (0..index)
            .rev()
            .filter_map(|i| self.entries[i].as_dir())
            .inspect(|dir| eprintln!("checking: {}", dir.path.display()))
            .find_map(|dir| (dir.path == parent_path).then_some(dir.borrow()))
    }
    pub fn parent_of_mut(&self, path: impl AsRef<Path>) -> Option<DirRefMut<'_, D>> {
        let parent_path = path.as_ref().parent().unwrap();
        let index = self.index_of(&path)?;
        self.entries[0..index]
            .iter()
            .rev()
            .filter_map(Entry::as_dir)
            .find_map(|dir| (dir.path == parent_path).then_some(dir.borrow_mut()))
    }
    pub fn iter_children_of(
        &self,
        path: impl AsRef<Path>,
    ) -> impl Iterator<Item = EntryRef<'_, F, D>> {
        let index = self.index_of(&path).unwrap_or(self.entries.len() - 1);
        self.entries[index + 1..]
            .iter()
            .filter(move |entry| {
                entry
                    .path()
                    .strip_prefix(&path)
                    .is_ok_and(|rest| rest.iter().count() == 1)
            })
            .map(Entry::borrow)
    }
    pub fn iter_children_of_mut(
        &self,
        path: impl AsRef<Path>,
    ) -> impl Iterator<Item = EntryRefMut<'_, F, D>> {
        let index = self.index_of(&path).unwrap_or(self.entries.len() - 1);
        self.entries[index + 1..]
            .iter()
            .filter(move |entry| {
                entry
                    .path()
                    .strip_prefix(&path)
                    .is_ok_and(|rest| rest.iter().count() == 1)
            })
            .map(Entry::borrow_mut)
    }
    pub fn index_of(&self, path: impl AsRef<Path>) -> Option<usize> {
        let path = path.as_ref();
        self.entries
            .binary_search_by(|entry| entry.path().cmp(path))
            .ok()
    }
}

// #########################################
// ----------------- Entry -----------------
// #########################################
enum Entry<F, D> {
    File { file: File<F> },
    Dir { dir: Dir<D> },
}
impl<F, D> Entry<F, D> {
    fn new_file(path: PathBuf, data: F) -> Self {
        Self::File {
            file: File::new(path, data),
        }
    }
    fn new_dir(path: PathBuf, data: D) -> Self {
        Self::Dir {
            dir: Dir::new(path, data),
        }
    }
    fn as_file(&self) -> Option<&File<F>> {
        match self {
            Entry::File { file } => Some(file),
            Entry::Dir { .. } => None,
        }
    }
    fn as_dir(&self) -> Option<&Dir<D>> {
        match self {
            Entry::File { .. } => None,
            Entry::Dir { dir } => Some(dir),
        }
    }
    fn path(&self) -> &Path {
        match self {
            Entry::File {
                file: File { path, .. },
            } => path,
            Entry::Dir {
                dir: Dir { path, .. },
            } => path,
        }
    }
    fn borrow<'a>(&'a self) -> EntryRef<'a, F, D> {
        match self {
            Entry::File { file } => EntryRef::File {
                file: file.borrow(),
            },
            Entry::Dir { dir } => EntryRef::Dir { dir: dir.borrow() },
        }
    }
    fn borrow_mut<'a>(&'a self) -> EntryRefMut<'a, F, D> {
        match self {
            Entry::File { file } => EntryRefMut::File {
                file: file.borrow_mut(),
            },
            Entry::Dir { dir } => EntryRefMut::Dir {
                dir: dir.borrow_mut(),
            },
        }
    }
}
pub enum EntryRef<'a, F, D> {
    File { file: FileRef<'a, F> },
    Dir { dir: DirRef<'a, D> },
}
impl<'a, F, D> EntryRef<'a, F, D> {
    pub fn path(&self) -> &Path {
        match self {
            EntryRef::File { file } => file.path,
            EntryRef::Dir { dir } => dir.path,
        }
    }
}
pub enum EntryRefMut<'a, F, D> {
    File { file: FileRefMut<'a, F> },
    Dir { dir: DirRefMut<'a, D> },
}
impl<'a, F, D> EntryRefMut<'a, F, D> {
    pub fn path(&self) -> &Path {
        match self {
            EntryRefMut::File { file } => file.path,
            EntryRefMut::Dir { dir } => dir.path,
        }
    }
    pub fn as_file(&mut self) -> Option<&mut FileRefMut<'a, F>> {
        match self {
            EntryRefMut::File { file } => Some(file),
            EntryRefMut::Dir { .. } => None,
        }
    }
    pub fn as_dir(&mut self) -> Option<&mut DirRefMut<'a, D>> {
        match self {
            EntryRefMut::File { .. } => None,
            EntryRefMut::Dir { dir } => Some(dir),
        }
    }
}

// ########################################
// ----------------- File -----------------
// ########################################
struct File<T> {
    path: PathBuf,
    data: RefCell<T>,
}
impl<F> File<F> {
    fn new(path: PathBuf, data: F) -> Self {
        Self {
            path,
            data: RefCell::new(data),
        }
    }
    fn borrow<'a>(&'a self) -> FileRef<'a, F> {
        FileRef {
            path: &self.path,
            data: self.data.borrow(),
        }
    }
    fn borrow_mut<'a>(&'a self) -> FileRefMut<'a, F> {
        FileRefMut {
            path: &self.path,
            data: self.data.borrow_mut(),
        }
    }
}
pub struct FileRef<'a, F> {
    pub path: &'a Path,
    pub data: Ref<'a, F>,
}
pub struct FileRefMut<'a, F> {
    pub path: &'a Path,
    pub data: RefMut<'a, F>,
}

// #######################################
// ----------------- Dir -----------------
// #######################################
struct Dir<D> {
    path: PathBuf,
    data: RefCell<D>,
}
impl<D> Dir<D> {
    fn new(path: PathBuf, data: D) -> Self {
        Self {
            path,
            data: RefCell::new(data),
        }
    }
    fn borrow<'a>(&'a self) -> DirRef<'a, D> {
        DirRef {
            path: &self.path,
            data: self.data.borrow(),
        }
    }
    fn borrow_mut<'a>(&'a self) -> DirRefMut<'a, D> {
        DirRefMut {
            path: &self.path,
            data: self.data.borrow_mut(),
        }
    }
}
pub struct DirRef<'a, D> {
    pub path: &'a Path,
    pub data: Ref<'a, D>,
}
pub struct DirRefMut<'a, D> {
    pub path: &'a Path,
    pub data: RefMut<'a, D>,
}

trait OkTraceExt<T> {
    fn ok_trace(self) -> Option<T>;
}
impl<T, E: std::fmt::Display> OkTraceExt<T> for Result<T, E> {
    fn ok_trace(self) -> Option<T> {
        self.inspect_err(|e| tracing::error!("{e:#}")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetree_dump() {
        let mut ft = FileTree::new(
            |_| Some(()),
            |path| {
                (!path
                    .components()
                    .any(|c| ["target"].contains(&c.as_os_str().to_string_lossy().as_ref())))
                .then_some(())
            },
        );
        ft.load_dir(".").unwrap();
        for entry in ft.iter() {
            match entry {
                EntryRef::File { file } => println!("File: {}", file.path.display()),
                EntryRef::Dir { dir } => println!("Dir:  {}", dir.path.display()),
            }
        }
    }

    #[test]
    fn parent() {
        let mut ft = FileTree::new(|_| Some(()), |_| Some(()));
        ft.load_dir(".").unwrap();
        dbg!(ft.parent_of("./Cargo.toml").map(|dir| dir.path.display()));
    }

    #[test]
    fn children() {
        let mut ft = FileTree::new(|_| Some(()), |_| Some(()));
        ft.load_dir(".").unwrap();
        for child in ft.iter_children_of("./target/release") {
            println!("{}", child.path().display());
        }
    }
}
