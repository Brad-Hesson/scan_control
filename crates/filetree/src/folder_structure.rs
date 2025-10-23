use std::{
    fmt::Debug,
    iter::once,
    path::{Path, PathBuf},
};

use crate::dir_walk::{self, DirEntry};

#[derive(Debug)]
pub struct File<T> {
    pub path: PathBuf,
    pub data: T,
}

pub enum Child<T, U> {
    File { file: File<T> },
    Folder { folder: Folder<T, U> },
}
impl<T, U> Child<T, U> {
    pub fn path(&self) -> &Path {
        match self {
            Child::File {
                file: File { path, .. },
            } => path,
            Child::Folder {
                folder: Folder { path, .. },
            } => path,
        }
    }
    pub fn as_file(&self) -> Option<&File<T>> {
        match self {
            Child::File { file } => Some(file),
            Child::Folder { .. } => None,
        }
    }
    pub fn as_file_mut(&mut self) -> Option<&mut File<T>> {
        match self {
            Child::File { file } => Some(file),
            Child::Folder { .. } => None,
        }
    }
    pub fn as_folder(&self) -> Option<&Folder<T, U>> {
        match self {
            Child::File { .. } => None,
            Child::Folder { folder } => Some(folder),
        }
    }
    pub fn as_folder_mut(&mut self) -> Option<&mut Folder<T, U>> {
        match self {
            Child::File { .. } => None,
            Child::Folder { folder } => Some(folder),
        }
    }
    pub(crate) fn rename_recursive(&mut self, path: PathBuf) {
        match self {
            Child::File { file } => file.path = path,
            Child::Folder { folder } => folder.rename_recursive(path),
        }
    }
}
impl<T: Debug, U: Debug> Debug for Child<T, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File { file } => file.fmt(f),
            Self::Folder { folder } => folder.fmt(f),
        }
    }
}

#[derive(Debug)]
pub struct Folder<T, U> {
    pub path: PathBuf,
    pub data: U,
    pub children: Vec<Child<T, U>>,
}
impl<T, U> Folder<T, U> {
    pub fn iter_descendants(&self) -> impl DoubleEndedIterator<Item = &Child<T, U>> {
        self.children.iter().flat_map(
            |child| -> Box<dyn DoubleEndedIterator<Item = &Child<T, U>>> {
                match child {
                    Child::Folder { folder } => {
                        Box::new(once(child).chain(folder.iter_descendants()))
                    }
                    Child::File { .. } => Box::new(once(child)),
                }
            },
        )
    }
    pub fn load_path(
        path: impl AsRef<Path>,
        load_file_fn: &mut impl FnMut(&Path) -> Option<T>,
        load_folder_fn: &mut impl FnMut(&Path) -> Option<U>,
    ) -> std::io::Result<Option<Self>> {
        let Some(data) = load_folder_fn(path.as_ref()) else {
            return Ok(None);
        };
        let mut children = vec![];
        for entry in dir_walk::read_dir(&path)? {
            match entry? {
                DirEntry::Dir { path: folder_path } => {
                    if let Some(folder) =
                        Folder::load_path(folder_path, load_file_fn, load_folder_fn)?
                    {
                        children.push(Child::Folder { folder });
                    }
                }
                DirEntry::File { path: file_path } => {
                    if let Some(data) = load_file_fn(&file_path) {
                        children.push(Child::File {
                            file: File {
                                path: file_path,
                                data,
                            },
                        });
                    }
                }
            }
        }
        Ok(Some(Self {
            data,
            path: path.as_ref().to_path_buf(),
            children,
        }))
    }
    pub fn get_parent_of(&self, path: impl AsRef<Path>) -> Option<&Folder<T, U>> {
        let path = path.as_ref().parent().expect("path should have a parent");
        if self.path == path {
            Some(self)
        } else {
            self.children
                .iter()
                .filter_map(Child::as_folder)
                .find(|f| path.starts_with(&f.path))
                .and_then(|f| f.get_parent_of(path))
        }
    }
    pub fn get_parent_of_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut Folder<T, U>> {
        let path = path.as_ref().parent().expect("path should have a parent");
        if self.path == path {
            Some(self)
        } else {
            self.children
                .iter_mut()
                .filter_map(Child::as_folder_mut)
                .find(|f| path.starts_with(&f.path))
                .and_then(|f| f.get_parent_of_mut(path))
        }
    }
    pub fn get_descendant_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut Child<T, U>> {
        let path = path.as_ref();
        self.get_parent_of_mut(path)?
            .children
            .iter_mut()
            .find(|child| child.path() == path)
    }
    pub(crate) fn rename_recursive(&mut self, path: PathBuf) {
        let old_base = std::mem::replace(&mut self.path, path.clone());
        let new_base = path;
        for child in &mut self.children {
            let new_path = new_base.join(child.path().strip_prefix(&old_base).unwrap());
            child.rename_recursive(new_path);
        }
    }
}
