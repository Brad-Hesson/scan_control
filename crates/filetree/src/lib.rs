use std::{fmt::Debug, path::Path, sync::mpsc};

use eyre::{Context, ContextCompat, eyre};
use itertools::Itertools;
use notify_typed::{Event, EventWatcher, RecursiveMode};

use crate::{
    dir_walk::DirEntry,
    folder_structure::{Child, File, Folder},
    handlers::{LoadHandler, UpdateHandler},
};

mod dir_walk;
pub mod file_uid;
pub mod folder_structure;
pub mod handlers;

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
        mut load_handler: impl LoadHandler<FileData = T, FolderData = U>,
    ) -> eyre::Result<()> {
        if let Some(old_root) = self.root.take() {
            self.watcher
                .unwatch(old_root.path)
                .context("failed to unwatch old path")?;
        }
        if let Some(new_root) =
            Folder::load_path(&path, &mut load_handler).context("failed to load new path")?
        {
            self.watcher
                .watch(&new_root.path, RecursiveMode::Recursive)
                .context("failed to watch new path")?;
            self.root = Some(new_root);
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
    pub fn root_mut(&mut self) -> Option<&mut Folder<T, U>> {
        self.root.as_mut()
    }
    pub fn process_updates(
        &mut self,
        mut update_handler: impl UpdateHandler<FileData = T, FolderData = U>,
    ) -> eyre::Result<()> {
        let Some(root) = self.root.as_mut() else {
            return Ok(());
        };
        for event in self.event_rx.try_iter() {
            || -> eyre::Result<()> {
                match &event {
                    Event::Create { path } => {
                        let parent = root
                            .get_folder_mut(
                                path.parent().context("new path does not have a parent")?,
                            )
                            .context("parent of new path was not found")?;
                        match DirEntry::try_from(path.to_path_buf())
                            .context("failed to parse new path")?
                        {
                            DirEntry::File { path } => {
                                if let Some(data) = update_handler.load_file(&path) {
                                    parent.children.push(Child::File {
                                        file: File { path, data },
                                    });
                                }
                            }
                            DirEntry::Dir { path } => {
                                if let Some(folder) = Folder::load_path(path, &mut update_handler)
                                    .context("failed to load new folder")?
                                {
                                    parent.children.push(Child::Folder { folder });
                                }
                            }
                        }
                    }
                    Event::Rename { from, to } => {
                        let to_rename = root
                            .get_descendant_mut(from)
                            .context("old path was not found")?;
                        to_rename
                            .rename_recursive(to.to_path_buf(), &mut update_handler)
                            .context("failed to rename child")?;
                    }
                    Event::Move { from, to } => {
                        let mut to_move = root
                            .get_folder_mut(
                                from.parent().context("old path does not have a parent")?,
                            )
                            .context("old parent was not found")?
                            .children
                            .extract_if(.., |child| child.path() == from)
                            .exactly_one()
                            .map_err(|err| {
                                eyre!("expected one child to match old path, got {}", err.count())
                            })?;
                        to_move
                            .rename_recursive(to.to_path_buf(), &mut update_handler)
                            .context("failed to rename child")?;
                        root.get_folder_mut(to.parent().context("new path does not have parent")?)
                            .context("new parent was not found")?
                            .children
                            .push(to_move);
                    }
                    Event::Delete { path } => {
                        let to_delete = root
                            .get_folder_mut(path.parent().context("path does not have a parent")?)
                            .context("parent was not found")?
                            .children
                            .extract_if(.., |child| child.path() == path)
                            .exactly_one()
                            .map_err(|err| {
                                eyre!("expected one child to match path, got {}", err.count())
                            })?;
                        to_delete.delete_recursive(&mut update_handler);
                    }
                }
                Ok(())
            }()
            .with_context(|| format!("failed to process event {:?}", &event))?;
        }
        Ok(())
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
    use std::time::Duration;

    use super::*;

    fn print_child(child: &Child<(), ()>, prefix: &Path, depth: usize) {
        match child {
            Child::File { file } => {
                for _ in 0..depth {
                    print!("  ");
                }
                println!("{}", file.path.strip_prefix(prefix).unwrap().display());
            }
            Child::Folder { folder } => print_folder(folder, prefix, depth),
        };
    }
    fn print_folder(folder: &Folder<(), ()>, prefix: &Path, depth: usize) {
        for _ in 0..depth {
            print!("  ");
        }
        println!("{}", folder.path.strip_prefix(prefix).unwrap().display());
        for child in &folder.children {
            print_child(child, &folder.path, depth + 1);
        }
    }

    #[test]
    fn updating() {
        let mut ft = FileTree::new().unwrap();
        let path = Path::new("./data").canonicalize().unwrap();
        ft.load_path(&path, ()).unwrap();
        let prefix = path.parent().unwrap();
        print_folder(ft.root.as_ref().unwrap(), prefix, 0);
        loop {
            let mut updated = false;
            ft.process_updates(((), |event| {
                updated = true;
                println!("{event:?}")
            }));
            if updated {
                println!("----------------------------------------");
                print_folder(ft.root.as_ref().unwrap(), prefix, 0);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
