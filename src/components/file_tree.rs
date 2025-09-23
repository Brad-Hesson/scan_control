use std::{
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    time::Duration,
};

use egui::{Button, CollapsingHeader, Context, Image, Ui};
use eyre::{bail, Context as _, ContextCompat, Result};
use itertools::Itertools;
use notify::{event::ModifyKind, Event, EventKind, RecommendedWatcher, Watcher};
use sxmfile::SXM;
use tracing::{error, trace};

pub struct FileTree {
    working_path: PathBuf,
    top: Arc<Mutex<DirItem>>,
    watcher: RecommendedWatcher,
}
impl FileTree {
    pub fn load_path(ctx: &Context, path: impl AsRef<Path>) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let top = Arc::new(Mutex::new(DirItem::load_dir(&path)?));
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(path.as_ref(), notify::RecursiveMode::Recursive)?;
        std::thread::spawn(watcher_job(rx, &top, ctx.clone()));
        Ok(Self {
            working_path: path
                .as_ref()
                .canonicalize()
                .context("failed to canonicalize path")?,
            top,
            watcher, // eb,
        })
    }
    pub fn show(&mut self, ui: &mut Ui) {
        let DirItem::Folder { items, .. } = &*self.top.lock().unwrap() else {
            unreachable!()
        };
        for item in items {
            item.show(ui);
        }
    }
}
fn watcher_job(
    rx: Receiver<notify::Result<Event>>,
    top: &Arc<Mutex<DirItem>>,
    ctx: Context,
) -> impl FnOnce() {
    let top = top.clone();
    move || {
        for modification in ModificationIter::new(rx) {
            let top = &mut *top.lock().unwrap();
            match modification {
                Modification::Rename { from, to } => {
                    trace!("Rename {} to {}", from.display(), to.display());
                    *top.get_by_path_mut(from).unwrap().path_mut() = to;
                }
                Modification::Move { from, to } => {
                    trace!("Move {} to {}", from.display(), to.display());
                    let mut item = top.remove_by_path(from).unwrap();
                    *item.path_mut() = to;
                    top.insert(item).unwrap();
                }
                Modification::Create { path } => {
                    trace!("Create {}", path.display());
                    if let Some(entry) = DirItem::load_entry(path).unwrap() {
                        top.insert(entry).unwrap();
                    }
                }
                Modification::Delete { path } => {
                    trace!("Delete {}", path.display());
                    top.remove_by_path(path).unwrap();
                }
            }
            ctx.request_repaint();
        }
        trace!("Exiting watcher thread");
    }
}

struct ModificationIter {
    rx: Receiver<notify::Result<Event>>,
    buffered: Option<Event>,
}
impl ModificationIter {
    fn new(rx: Receiver<notify::Result<Event>>) -> Self {
        Self { rx, buffered: None }
    }
}
impl Iterator for ModificationIter {
    type Item = Modification;

    fn next(&mut self) -> Option<Self::Item> {
        let mut event = match self.buffered.take() {
            Some(event) => event,
            None => match self.rx.recv().ok()? {
                Ok(event) => event,
                Err(e) => {
                    error!("{e:#}");
                    return self.next();
                }
            },
        };
        let modification = match event.kind {
            EventKind::Create(_) => {
                let Some(path) = event.paths.pop() else {
                    error!("got event with no paths");
                    return self.next();
                };
                Some(Modification::Create { path })
            }
            EventKind::Modify(ModifyKind::Name(_)) => {
                let Some(from) = event.paths.pop() else {
                    error!("got event with no paths");
                    return self.next();
                };
                let mut event = match self.rx.recv().ok()? {
                    Ok(event) => event,
                    Err(e) => {
                        error!("{e:#}");
                        return self.next();
                    }
                };
                let modification = match event.kind {
                    EventKind::Modify(ModifyKind::Name(_)) => {
                        let Some(to) = event.paths.pop() else {
                            error!("got event with no paths");
                            return self.next();
                        };
                        Some(Modification::Rename { from, to })
                    }
                    _ => {
                        error!("expected EventKind::Modify(ModifyKind::Name(_) got {event:?}");
                        self.next()
                    }
                };
                if event.paths.len() != 0 {
                    error!("Extra paths in event");
                }
                modification
            }
            EventKind::Remove(_) => {
                let Some(from) = event.paths.pop() else {
                    error!("got event with no paths");
                    return self.next();
                };
                let mut event = match self.rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(Ok(event)) => event,
                    Ok(Err(e)) => {
                        error!("{e:#}");
                        return self.next();
                    }
                    Err(RecvTimeoutError::Disconnected) => return None,
                    Err(RecvTimeoutError::Timeout) => {
                        return Some(Modification::Delete { path: from })
                    }
                };
                let modification = match event.kind {
                    EventKind::Create(_) => {
                        let Some(to) = event.paths.pop() else {
                            error!("got event with no paths");
                            return self.next();
                        };
                        Some(Modification::Move { from, to })
                    }
                    _ => {
                        self.buffered = Some(event.clone());
                        Some(Modification::Delete { path: from })
                    }
                };
                if event.paths.len() != 0 {
                    error!("Extra paths in event");
                }
                modification
            }
            _ => self.next(),
        };
        if event.paths.len() != 0 {
            error!("Extra paths in event");
        }
        modification
    }
}

enum Modification {
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Create { path: PathBuf },
    Delete { path: PathBuf },
}

pub enum DirItem {
    Folder { path: PathBuf, items: Vec<DirItem> },
    File { path: PathBuf, src: Option<SXM> },
}
impl DirItem {
    fn refresh(&mut self) -> Result<()> {
        *self = Self::load_entry(self.path())?.unwrap();
        Ok(())
    }
    fn insert(&mut self, item: DirItem) -> Result<()> {
        let path = item.path();
        let parent = self.get_by_path_mut(path.parent().context("path had no parent")?)?;
        let DirItem::Folder { items, .. } = parent else {
            bail!("parent was not a folder");
        };
        items.push(item);
        Ok(())
    }
    fn remove_by_path(&mut self, path: impl AsRef<Path>) -> Result<DirItem> {
        let path = path.as_ref();
        let parent = self.get_by_path_mut(path.parent().context("path had no parent")?)?;
        let DirItem::Folder { items, .. } = parent else {
            bail!("parent was not a folder");
        };
        Ok(items
            .extract_if(.., |item| item.path() == path)
            .exactly_one()
            .ok()
            .expect("only one item with the same path"))
    }
    fn get_by_path_mut(&mut self, path: impl AsRef<Path>) -> Result<&mut DirItem> {
        let path = path.as_ref();
        let relative = path.strip_prefix(self.path()).context(format!(
            "requested path {} was not a member of the working directory {}",
            path.display(),
            self.path().display()
        ))?;
        let mut item = self;
        for folder_name in relative.iter() {
            item = item
                .iter_children_mut()
                .find(|item| item.path().file_name().unwrap() == folder_name)
                .context("path was not found")?;
        }
        Ok(item)
    }
    fn path_mut(&mut self) -> &mut PathBuf {
        match self {
            DirItem::Folder { path, .. } => path,
            DirItem::File { path, .. } => path,
        }
    }
    fn path(&self) -> &Path {
        match self {
            DirItem::Folder { path, .. } => path,
            DirItem::File { path, .. } => path,
        }
    }
    fn iter_children(&self) -> impl DoubleEndedIterator<Item = &DirItem> {
        match self {
            DirItem::Folder { items, .. } => items.iter(),
            DirItem::File { .. } => (&[]).iter(),
        }
    }
    fn iter_children_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut DirItem> {
        match self {
            DirItem::Folder { items, .. } => items.iter_mut(),
            DirItem::File { .. } => (&mut []).iter_mut(),
        }
    }
    fn show(&self, ui: &mut Ui) {
        match self {
            DirItem::Folder { .. } => self.show_folder(ui),
            DirItem::File { .. } => self.show_file(ui),
        }
    }
    fn show_folder(&self, ui: &mut Ui) {
        let Self::Folder { path, items } = self else {
            unreachable!()
        };
        CollapsingHeader::new(path.file_stem().unwrap().to_string_lossy()).show(ui, |ui| {
            for item in items {
                item.show(ui);
            }
        });
    }
    fn show_file(&self, ui: &mut Ui) {
        let Self::File { path, .. } = self else {
            unreachable!()
        };
        let name = path.file_stem().unwrap().to_string_lossy();
        ui.add(
            Button::opt_image_and_text(
                Some(Image::new(egui::include_image!(
                    "../../assets/scan_image_icon.png"
                ))),
                Some(egui::WidgetText::Text(name.to_string())),
            )
            .image_tint_follows_text_color(true)
            .frame_when_inactive(false),
        );
    }
    fn load_entry(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let file_type = std::fs::metadata(&path)?.file_type();
        if file_type.is_dir() {
            Ok(Some(Self::load_dir(path)?))
        } else if file_type.is_file() {
            Self::load_file(path)
        } else {
            error!("Encountered a symlink");
            Ok(None)
        }
    }
    fn load_dir(path: impl AsRef<Path>) -> Result<Self> {
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if let Some(item) = Self::load_entry(entry.path())? {
                items.push(item);
            }
        }
        Ok(Self::Folder {
            items,
            path: path.as_ref().into(),
        })
    }
    fn load_file(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let file_extension = path
            .as_ref()
            .extension()
            .unwrap_or_default()
            .to_str()
            .context("file extension not valid utf-8")?;
        if file_extension != "sxm" {
            return Ok(None);
        }
        Ok(Some(Self::File {
            src: None,
            path: path.as_ref().into(),
        }))
    }
}
