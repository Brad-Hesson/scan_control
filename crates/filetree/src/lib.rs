use std::{fmt::Debug, path::Path, sync::mpsc};

use itertools::Itertools;
use notify_typed::{Event, EventWatcher};

use crate::{
    dir_walk::DirEntry,
    folder_structure::{Child, File, Folder},
};

mod dir_walk;
mod file_uid;
mod folder_structure;

pub struct FileTree<T, U> {
    root: Option<Folder<T, U>>,
    watcher: notify_typed::EventWatcher,
    event_rx: std::sync::mpsc::Receiver<Event>,
}
impl<T, U> FileTree<T, U> {
    pub fn new() -> eyre::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel();
        Ok(Self {
            watcher: EventWatcher::new(event_tx)?,
            event_rx,
            root: None,
        })
    }
    pub fn load_path(
        &mut self,
        path: impl AsRef<Path>,
        mut load_file_fn: impl FnMut(&Path) -> Option<T>,
        mut load_folder_fn: impl FnMut(&Path) -> Option<U>,
    ) -> std::io::Result<()> {
        if let Some(tree) = Folder::load_path(path, &mut load_file_fn, &mut load_folder_fn)? {
            self.root = Some(tree);
        }
        Ok(())
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Child<T, U>> {
        self.root.iter().flat_map(Folder::iter_descendants)
    }
    pub fn iter_files(&self) -> impl DoubleEndedIterator<Item = &File<T>> {
        self.iter().filter_map(Child::<T, U>::as_file)
    }
    pub fn root(&self) -> Option<&Folder<T, U>> {
        self.root.as_ref()
    }
    pub fn update(
        &mut self,
        mut handler: impl FnMut(Event),
        mut load_file_fn: impl FnMut(&Path) -> Option<T>,
        mut load_folder_fn: impl FnMut(&Path) -> Option<U>,
    ) {
        let Some(root) = self.root.as_mut() else {
            return;
        };
        for event in self.event_rx.try_iter() {
            match &event {
                Event::Create { path } => {
                    let parent = root.get_parent_of_mut(&path).unwrap();
                    match DirEntry::try_from(path.to_path_buf()).unwrap() {
                        DirEntry::File { path } => {
                            if let Some(data) = load_file_fn(&path) {
                                parent.children.push(Child::File {
                                    file: File { path, data },
                                });
                            }
                        }
                        DirEntry::Dir { path } => {
                            if let Some(folder) =
                                Folder::load_path(path, &mut load_file_fn, &mut load_folder_fn)
                                    .unwrap()
                            {
                                parent.children.push(Child::Folder { folder });
                            }
                        }
                    }
                }
                Event::Rename { from, to } => {
                    root.get_descendant_mut(from)
                        .unwrap()
                        .rename_recursive(to.to_path_buf());
                }
                Event::Move { from, to } => {
                    let mut child = root
                        .get_parent_of_mut(&from)
                        .unwrap()
                        .children
                        .extract_if(.., |child| child.path() == from)
                        .exactly_one()
                        .ok()
                        .unwrap();
                    child.rename_recursive(to.to_path_buf());
                    root.get_parent_of_mut(to).unwrap().children.push(child);
                }
                Event::Delete { path } => {
                    root.get_parent_of_mut(&path)
                        .unwrap()
                        .children
                        .extract_if(.., |child| child.path() == path)
                        .exactly_one()
                        .ok()
                        .unwrap();
                }
            }
            handler(event);
        }
    }
}
impl<T: Debug, U: Debug> Debug for FileTree<T, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTree")
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::file_uid::{FileHasher, IdentityHasher};

    use super::*;

    #[test]
    fn iter_descendants() {
        let mut hasher = FileHasher::default();
        let mut ft = FileTree::new().unwrap();
        ft.load_path(
            "..\\..",
            |file_path| Some(hasher.hash_file(file_path).unwrap()),
            |folder_path| {
                (!folder_path.file_name().is_some_and(|name| {
                    ["target", ".git"].contains(&name.to_string_lossy().as_ref())
                }))
                .then_some(())
            },
        )
        .unwrap();
        for a in ft.iter() {
            match a {
                Child::File { file } => println!("File:   {}", file.path.display()),
                Child::Folder { folder } => println!("Folder: {}", folder.path.display()),
            }
        }
    }

    #[test]
    fn dump() {
        let mut hasher = FileHasher::default();
        let mut files = HashMap::with_hasher(IdentityHasher);
        let mut ft = FileTree::new().unwrap();
        ft.load_path(
            "..\\..",
            |path| {
                let id = hasher.hash_file(path).unwrap();
                files.insert(id, path.to_path_buf());
                Some(id)
            },
            |folder_path| {
                (!folder_path.file_name().is_some_and(|name| {
                    ["target", ".git"].contains(&name.to_string_lossy().as_ref())
                }))
                .then_some(())
            },
        )
        .unwrap();
        println!("{ft:#?}");
        println!("{files:#?}")
    }
}
