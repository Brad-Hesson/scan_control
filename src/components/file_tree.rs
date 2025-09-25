use std::{
    iter::FlatMap,
    path::{Display, Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use egui::{Button, CollapsingHeader, Context, Image, Ui};
use eyre::{Context as _, ContextCompat, Result, bail};
use itertools::Itertools;
use notify_typed::{Event, EventWatcher, RecursiveMode};
use sxmfile::SXM;
use tracing::{error, trace, info};

pub struct FileTree {
    top: Option<DirItem>,
    rx: mpsc::Receiver<Event>,
    _watcher: EventWatcher,
}
impl FileTree {
    pub fn new(ctx: &Context) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let mut _watcher = EventWatcher::new(move |event| {
            tx.send(event).ok();
            ctx.request_repaint_after(Duration::from_millis(100));
        })?;
        Ok(Self {
            top: None,
            _watcher,
            rx,
        })
    }
    pub fn load_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        if let Some(top) = self.top.take() {
            self._watcher.unwatch(top.path())?;
        }
        self.top = Some(DirItem::load_dir(&path)?);
        self._watcher.watch(&path, RecursiveMode::Recursive)?;
        Ok(())
    }
    pub fn show(&mut self, ui: &mut Ui) {
        self.update();
        let Some(top) = &mut self.top else {
            return;
        };
        let DirItem::Folder { items, .. } = &top else {
            unreachable!()
        };
        for item in items {
            item.show(ui);
        }
    }
    pub fn update(&mut self) {
        let Some(top) = &mut self.top else {
            return;
        };
        trace!("Updating FileTree");
        for modification in self.rx.try_iter() {
            if let Err(e) = || -> Result<()> {
                let top_path = top.path().to_path_buf();
                let shortened: &dyn for<'a> Fn(&'a PathBuf) -> Result<Display<'a>> =
                    &move |path: &PathBuf| Ok(path.strip_prefix(&top_path)?.display());
                match modification {
                    Event::Rename { from, to } => {
                        info!("Rename `{}` to `{}`", shortened(&from)?, shortened(&to)?);
                        *top.get_by_path_mut(from)?.path_mut() = to;
                    }
                    Event::Move { from, to } => {
                        info!("Move `{}` to `{}`", shortened(&from)?, shortened(&to)?);
                        let mut item = top.remove_by_path(from)?;
                        *item.path_mut() = to;
                        top.insert(item)?;
                    }
                    Event::Create { path } => {
                        info!("Create `{}`", shortened(&path)?);
                        if let Some(entry) = DirItem::load_entry(path)? {
                            top.insert(entry)?;
                        }
                    }
                    Event::Delete { path } => {
                        info!("Delete `{}`", shortened(&path)?);
                        top.remove_by_path(path)?;
                    }
                }
                Ok(())
            }() {
                error!("{e:#}")
            };
        }
    }
}
impl<'a> IntoIterator for &'a FileTree {
    type Item = &'a DirItem;

    type IntoIter = FileTreeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        match &self.top {
            Some(top) => top.iter(),
            None => FileTreeIter::empty(),
        }
    }
}
impl<'a> IntoIterator for &'a mut FileTree {
    type Item = &'a mut DirItem;

    type IntoIter = FileTreeIterMut<'a>;

    fn into_iter(self) -> Self::IntoIter {
        match &mut self.top {
            Some(top) => top.iter_mut(),
            None => FileTreeIterMut::empty(),
        }
    }
}

pub enum DirItem {
    Folder { path: PathBuf, items: Vec<DirItem> },
    File { path: PathBuf, src: Option<SXM> },
}
impl DirItem {
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
            .map_err(|mut items| match items.next() {
                Some(item) => eyre::Report::msg(format!(
                    "got multiple matches for `{}`",
                    item.path().display()
                )),
                None => eyre::Report::msg(format!("path not found: `{}`", path.display())),
            })?)
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
    pub fn path(&self) -> &Path {
        match self {
            DirItem::Folder { path, .. } => path,
            DirItem::File { path, .. } => path,
        }
    }
    pub fn iter_children<'a>(&'a self) -> ChildIter<'a> {
        ChildIter::new(self)
    }
    fn iter<'a>(&'a self) -> FileTreeIter<'a> {
        FileTreeIter::new(self)
    }
    fn iter_children_mut<'a>(&'a mut self) -> ChildIterMut<'a> {
        ChildIterMut::new(self)
    }
    fn iter_mut<'a>(&'a mut self) -> FileTreeIterMut<'a> {
        FileTreeIterMut::new(self)
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
        CollapsingHeader::new(path.file_stem().unwrap().to_string_lossy())
            .default_open(true)
            .show(ui, |ui| {
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
            .wrap_mode(egui::TextWrapMode::Truncate)
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
impl<'a> IntoIterator for &'a DirItem {
    type Item = &'a DirItem;

    type IntoIter = FileTreeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &'a mut DirItem {
    type Item = &'a mut DirItem;

    type IntoIter = FileTreeIterMut<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

pub struct ChildIter<'a> {
    iter: ChildIterType<'a>,
}
impl<'a> ChildIter<'a> {
    fn new(item: &'a DirItem) -> Self {
        let iter = match item {
            DirItem::Folder { items, .. } => ChildIterType::Folder(items.iter()),
            DirItem::File { .. } => ChildIterType::File(std::iter::once(item)),
        };
        Self { iter }
    }
}
enum ChildIterType<'a> {
    File(core::iter::Once<&'a DirItem>),
    Folder(core::slice::Iter<'a, DirItem>),
}
impl<'a> Iterator for ChildIter<'a> {
    type Item = &'a DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.iter {
            ChildIterType::File(once) => once.next(),
            ChildIterType::Folder(iter) => iter.next(),
        }
    }
}
impl<'a> DoubleEndedIterator for ChildIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.iter {
            ChildIterType::File(once) => once.next_back(),
            ChildIterType::Folder(iter) => iter.next_back(),
        }
    }
}
pub struct ChildIterMut<'a> {
    iter: ChildIterMutType<'a>,
}
impl<'a> ChildIterMut<'a> {
    fn new(item: &'a mut DirItem) -> Self {
        let iter = match item {
            DirItem::Folder { items, .. } => ChildIterMutType::Folder(items.iter_mut()),
            DirItem::File { .. } => ChildIterMutType::File(std::iter::once(item)),
        };
        Self { iter }
    }
}
enum ChildIterMutType<'a> {
    File(core::iter::Once<&'a mut DirItem>),
    Folder(core::slice::IterMut<'a, DirItem>),
}
impl<'a> Iterator for ChildIterMut<'a> {
    type Item = &'a mut DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.iter {
            ChildIterMutType::File(once) => once.next(),
            ChildIterMutType::Folder(iter) => iter.next(),
        }
    }
}
impl<'a> DoubleEndedIterator for ChildIterMut<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.iter {
            ChildIterMutType::File(once) => once.next_back(),
            ChildIterMutType::Folder(iter) => iter.next_back(),
        }
    }
}

pub struct FileTreeIter<'a> {
    iter: FlatMap<ChildIter<'a>, FileTreeIterType<'a>, fn(&'a DirItem) -> FileTreeIterType<'a>>,
}
impl<'a> FileTreeIter<'a> {
    fn new(item: &'a DirItem) -> Self {
        let map: fn(&'a DirItem) -> FileTreeIterType<'a> = |item| match item {
            DirItem::Folder { .. } => FileTreeIterType::Folder(Box::new(item.iter())),
            DirItem::File { .. } => FileTreeIterType::File(std::iter::once(item)),
        };
        let iter = item.iter_children().flat_map(map);
        Self { iter }
    }
    fn empty() -> Self {
        let children = ChildIter {
            iter: ChildIterType::Folder((&[]).iter()),
        };
        let map: fn(&'a DirItem) -> FileTreeIterType<'a> = |item| match item {
            DirItem::Folder { .. } => FileTreeIterType::Folder(Box::new(item.iter())),
            DirItem::File { .. } => FileTreeIterType::File(std::iter::once(item)),
        };
        let iter = children.flat_map(map);
        Self { iter }
    }
}
impl<'a> Iterator for FileTreeIter<'a> {
    type Item = &'a DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
impl<'a> DoubleEndedIterator for FileTreeIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}
enum FileTreeIterType<'a> {
    File(core::iter::Once<&'a DirItem>),
    Folder(Box<FileTreeIter<'a>>),
}
impl<'a> Iterator for FileTreeIterType<'a> {
    type Item = &'a DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FileTreeIterType::File(once) => once.next(),
            FileTreeIterType::Folder(iter) => iter.next(),
        }
    }
}
impl<'a> DoubleEndedIterator for FileTreeIterType<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            FileTreeIterType::File(once) => once.next_back(),
            FileTreeIterType::Folder(iter) => iter.next_back(),
        }
    }
}

pub struct FileTreeIterMut<'a> {
    iter: FlatMap<ChildIterMut<'a>, FileTreeIterMutType<'a>, fn(&'a mut DirItem) -> FileTreeIterMutType<'a>>,
}
impl<'a> FileTreeIterMut<'a> {
    fn new(item: &'a mut DirItem) -> Self {
        let map: fn(&'a mut DirItem) -> FileTreeIterMutType<'a> = |item| match item {
            DirItem::Folder { .. } => FileTreeIterMutType::Folder(Box::new(item.iter_mut())),
            DirItem::File { .. } => FileTreeIterMutType::File(std::iter::once(item)),
        };
        let iter = item.iter_children_mut().flat_map(map);
        Self { iter }
    }
    fn empty() -> Self {
        let children = ChildIterMut {
            iter: ChildIterMutType::Folder((&mut []).iter_mut()),
        };
        let map: fn(&'a mut DirItem) -> FileTreeIterMutType<'a> = |item| match item {
            DirItem::Folder { .. } => FileTreeIterMutType::Folder(Box::new(item.iter_mut())),
            DirItem::File { .. } => FileTreeIterMutType::File(std::iter::once(item)),
        };
        let iter = children.flat_map(map);
        Self { iter }
    }
}
impl<'a> Iterator for FileTreeIterMut<'a> {
    type Item = &'a mut DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
impl<'a> DoubleEndedIterator for FileTreeIterMut<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}
enum FileTreeIterMutType<'a> {
    File(core::iter::Once<&'a mut DirItem>),
    Folder(Box<FileTreeIterMut<'a>>),
}
impl<'a> Iterator for FileTreeIterMutType<'a> {
    type Item = &'a mut DirItem;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FileTreeIterMutType::File(once) => once.next(),
            FileTreeIterMutType::Folder(iter) => iter.next(),
        }
    }
}
impl<'a> DoubleEndedIterator for FileTreeIterMutType<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            FileTreeIterMutType::File(once) => once.next_back(),
            FileTreeIterMutType::Folder(iter) => iter.next_back(),
        }
    }
}
