use std::{
    fmt::Debug,
    iter::once,
    path::{Path, PathBuf},
};

use eyre::Context;

use crate::{
    dir_walk::{self, DirEntry},
    handlers::{LoadHandler, UpdateHandler},
};

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
    pub(crate) fn rename_recursive(
        &mut self,
        path: PathBuf,
        handler: &mut impl UpdateHandler<FileData = T, FolderData = U>,
    ) -> eyre::Result<()> {
        match self {
            Child::File { file } => {
                handler.rename_file(&file.path, &path, &mut file.data);
                file.path = path;
            }
            Child::Folder { folder } => folder.rename_recursive(path, handler)?,
        }
        Ok(())
    }
    pub(crate) fn delete_recursive(
        self,
        handler: &mut impl UpdateHandler<FileData = T, FolderData = U>,
    ) {
        match self {
            Child::File { file } => handler.delete_file(&file.path, file.data),
            Child::Folder { folder } => folder.delete_recursive(handler),
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
        load_handler: &mut impl LoadHandler<FileData = T, FolderData = U>,
    ) -> eyre::Result<Option<Self>> {
        let path = path
            .as_ref()
            .canonicalize()
            .context("failed to canonicalize the path")?;
        let Some(data) = load_handler.load_folder(path.as_ref()) else {
            return Ok(None);
        };
        let mut children = vec![];
        for entry in dir_walk::read_dir(&path)? {
            match entry? {
                DirEntry::Dir { path: folder_path } => {
                    if let Some(folder) = Folder::load_path(&folder_path, load_handler)
                        .with_context(|| format!("failed to load {}", folder_path.display()))?
                    {
                        children.push(Child::Folder { folder });
                    }
                }
                DirEntry::File { path: file_path } => {
                    if let Some(data) = load_handler.load_file(&file_path) {
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
            path,
            children,
        }))
    }
    pub fn get_descendant_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut Child<T, U>> {
        let target_path = path.as_ref();
        self.children
            .iter_mut()
            .find(|child| target_path.starts_with(child.path()))
            .and_then(|child| match child {
                Child::File { file } if file.path == target_path => Some(child),
                Child::Folder { folder } if folder.path == target_path => Some(child),
                Child::Folder { folder } => folder.get_descendant_mut(target_path),
                _ => None,
            })
    }
    pub fn get_folder_mut(&mut self, path: impl AsRef<Path>) -> Option<&mut Folder<T, U>> {
        let target_path = path.as_ref();
        if self.path == target_path {
            return Some(self);
        }
        self.children
            .iter_mut()
            .filter_map(Child::as_folder_mut)
            .find(|folder| target_path.starts_with(&folder.path))
            .and_then(|folder| folder.get_folder_mut(target_path))
    }
    pub(crate) fn rename_recursive(
        &mut self,
        path: PathBuf,
        handler: &mut impl UpdateHandler<FileData = T, FolderData = U>,
    ) -> eyre::Result<()> {
        handler.rename_folder(&self.path, &path, &mut self.data);
        let old_base = std::mem::replace(&mut self.path, path.clone());
        let new_base = path;
        for child in &mut self.children {
            let new_path = new_base.join(
                child
                    .path()
                    .strip_prefix(&old_base)
                    .context("child path was not a child of the parent path")?,
            );
            child.rename_recursive(new_path, handler)?;
        }
        Ok(())
    }
    pub(crate) fn delete_recursive(
        self,
        handler: &mut impl UpdateHandler<FileData = T, FolderData = U>,
    ) {
        for child in self.children {
            child.delete_recursive(handler);
        }
        handler.delete_folder(&self.path, self.data);
    }
}
