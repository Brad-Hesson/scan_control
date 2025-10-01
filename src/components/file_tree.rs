use std::{
    fmt::Debug,
    iter::{once, DoubleEndedIterator, FlatMap, Once},
    ops::{Deref, DerefMut, Index, IndexMut},
    path::{Display, Path, PathBuf},
    slice,
    sync::mpsc,
};

use egui::{
    AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, CollapsingHeader, Context, Frame,
    Image, IntoAtoms, Response, Sense, TextStyle, Ui,
};
use eyre::{bail, Context as _, ContextCompat, Result};
use itertools::Itertools;
use notify_typed::{Event, EventWatcher, RecursiveMode};
use tracing::{error, info, trace};

use crate::utils::response_group::{ResponseGroup, ResponseGroupExt as _, SyncResponse};

pub struct FileTree<T> {
    top: Option<Item<T>>,
    rx: mpsc::Receiver<Event>,
    watcher: EventWatcher,
    load_callback: Box<dyn Fn(&Path) -> Option<T>>,
}
impl<T> FileTree<T> {
    pub fn new(
        ctx: &Context,
        load_callback: impl Fn(&Path) -> Option<T> + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let watcher = EventWatcher::new(move |event| {
            tx.send(event).ok();
            ctx.request_repaint();
        })?;
        Ok(Self {
            top: None,
            watcher,
            rx,
            load_callback: Box::new(load_callback),
        })
    }
    pub fn load_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let new_top = Item::load_dir(&path, &self.load_callback)
            .with_context(|| format!("failed to load path `{}`", path.display()))?;
        self.top
            .as_ref()
            .map(|top| {
                self.watcher.unwatch(top.path()).with_context(|| {
                    format!("failed to unwatch the path `{}`", top.path().display())
                })
            })
            .transpose()?;
        self.watcher
            .watch(&path, RecursiveMode::Recursive)
            .with_context(|| format!("failed to start watching the path `{}`", path.display()))?;
        self.top = Some(new_top);
        Ok(())
    }
    pub fn show(&mut self, ui: &mut Ui) {
        self.update();
        if self.top.is_none() {
            return;
        }
        self.show_list(ui, Indexer::new());
    }
    fn show_item(&mut self, ui: &mut Ui, mut i: Indexer) -> SyncResponse {
        let top = self.top.as_ref().unwrap();
        match &top[i] {
            folder @ Item::Folder { .. } => {
                let name = folder.path().file_stem().unwrap().to_string_lossy();
                let resp = CollapsingHeader::new(name)
                    .default_open(true)
                    .show(ui, |ui| {
                        self.show_list(ui, i);
                    })
                    .header_response;
                SyncResponse {
                    orig: resp.clone(),
                    sync: resp,
                }
            }
            Item::File { .. } => self[i].show(ui),
        }
    }
    fn show_list(&mut self, ui: &mut Ui, ind: Indexer) {
        macro_rules! fldr {
            () => {
                self.top.as_mut().unwrap()[ind]
            };
        }
        for i in (0..fldr!().items().len()).rev() {
            let mut lower_ind = ind.clone();
            lower_ind.push_front(i);
            let mut resp = self.show_item(ui, lower_ind);
            if resp.sync.hovered() {
                resp.orig = resp.orig.highlight();
            }
            if resp.orig.clicked() && fldr!().items()[i].is_file() {
                if ui.input(|i| i.modifiers.shift) && fldr!().last_selected().is_some() {
                    let mut i = i as isize;
                    let last = fldr!().last_selected().unwrap() as isize;
                    let add = ((i < last) as isize) * 2 - 1;
                    while i != last {
                        *fldr!().items()[i as usize].selected() = true;
                        i += add;
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    *fldr!().items()[i].selected() = !*fldr!().items()[i].selected();
                } else {
                    self.clear_selected();
                    *fldr!().items()[i].selected() = true;
                }
                if *fldr!().items()[i].selected() {
                    *fldr!().last_selected() = Some(i);
                }
            }
        }
    }
    fn update(&mut self) {
        let Some(top) = &mut self.top else {
            return;
        };
        trace!("Updating FileTree");
        for event in self.rx.try_iter() {
            if let Err(e) = || -> Result<()> {
                let top_path = top.path().to_path_buf();
                let shortened: &dyn for<'a> Fn(&'a PathBuf) -> Result<Display<'a>> =
                    &move |path: &PathBuf| Ok(path.strip_prefix(&top_path)?.display());
                match event {
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
                        if let Some(entry) = Item::load_entry(path, &self.load_callback)? {
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
    pub fn iter<'a>(&'a self) -> FilesIter<'a, T> {
        match &self.top {
            Some(top) => top.iter_files(),
            None => FilesIter::empty(),
        }
    }
    pub fn iter_mut<'a>(&'a mut self) -> FilesIterMut<'a, T> {
        match &mut self.top {
            Some(top) => top.iter_files_mut(),
            None => FilesIterMut::empty(),
        }
    }
    pub fn clear_selected(&mut self) {
        for file in self {
            file.selected = false;
        }
    }
    pub fn iter_selected(&self) -> impl Iterator<Item = &File<T>> {
        self.iter().filter(|file| file.selected)
    }
    pub fn get_hovered(&self, ctx: &Context) -> Option<&File<T>> {
        self.iter()
            .find_map(|file| file.resp_group.response(ctx)?.hovered().then_some(file))
    }
    pub fn get(&self, indexer: Indexer) -> Option<&File<T>> {
        if let Item::File { inner } = self.top.as_ref()?.get(indexer)? {
            Some(inner)
        } else {
            None
        }
    }
    pub fn get_mut(&mut self, indexer: Indexer) -> Option<&mut File<T>> {
        if let Item::File { inner } = self.top.as_mut()?.get_mut(indexer)? {
            Some(inner)
        } else {
            None
        }
    }
    pub fn iter_indexers(&self) -> Box<dyn DoubleEndedIterator<Item = Indexer> + '_> {
        match self.top.as_ref() {
            Some(top) => Box::new(top.generate_indexers()),
            None => Box::new(std::iter::empty()),
        }
    }
    pub fn iter_selected_indexes(&self) -> impl DoubleEndedIterator<Item = Indexer> + '_ {
        self.iter_indexers().filter(|ind| self[*ind].selected)
    }
}
impl<'a, T> IntoIterator for &'a FileTree<T> {
    type Item = &'a File<T>;
    type IntoIter = FilesIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut FileTree<T> {
    type Item = &'a mut File<T>;
    type IntoIter = FilesIterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl<T> Index<Indexer> for FileTree<T> {
    type Output = File<T>;

    fn index(&self, index: Indexer) -> &Self::Output {
        self.get(index).unwrap()
    }
}
impl<T> IndexMut<Indexer> for FileTree<T> {
    fn index_mut(&mut self, index: Indexer) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}

#[derive(Clone, Copy)]
pub struct Indexer {
    indexes: [usize; 15],
    size: usize,
}
impl Indexer {
    fn new() -> Self {
        Self {
            indexes: [0; 15],
            size: 0,
        }
    }
    fn push(&mut self, i: usize) {
        self.indexes[self.size] = i;
        self.size += 1;
    }
    fn push_front(&mut self, i: usize) {
        self.size += 1;
        for i in (1..self.size).rev() {
            self.indexes[i] = self.indexes[i - 1];
        }
        self.indexes[0] = i;
    }
    fn is_empty(&self) -> bool {
        self.size == 0
    }
    fn pop(&mut self) -> usize {
        self.size -= 1;
        self.indexes[self.size]
    }
}
impl Debug for Indexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Indexer")
            .field("indexes", &&self.indexes[..self.size])
            .finish()
    }
}

pub struct File<T> {
    path: PathBuf,
    uuid: uuid::Uuid,
    pub data: T,
    pub selected: bool,
    pub resp_group: ResponseGroup,
}
impl<T> File<T> {
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn show(&mut self, ui: &mut Ui) -> SyncResponse {
        list_item(ui, self).synchronize(&mut self.resp_group)
    }
    fn load_path(
        path: impl AsRef<Path>,
        load_callback: &impl Fn(&Path) -> Option<T>,
    ) -> Option<Self> {
        let path = path.as_ref();
        load_callback(path).map(|data| Self {
            uuid: uuid::Uuid::new_v4(),
            path: path.into(),
            data,
            selected: false,
            resp_group: ResponseGroup::new(),
        })
    }
}
impl<T> Deref for File<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<T> DerefMut for File<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

enum Item<T> {
    Folder {
        path: PathBuf,
        items: Vec<Self>,
        last_selected: Option<usize>,
    },
    File {
        inner: File<T>,
    },
}
impl<T> Item<T> {
    fn is_file(&self) -> bool {
        match self {
            Self::Folder { .. } => false,
            Self::File { .. } => true,
        }
    }
    fn is_folder(&self) -> bool {
        match self {
            Self::Folder { .. } => true,
            Self::File { .. } => false,
        }
    }
    fn items(&mut self) -> &mut [Self] {
        match self {
            Item::Folder { items, .. } => items,
            Item::File { .. } => panic!(),
        }
    }
    fn last_selected(&mut self) -> &mut Option<usize> {
        match self {
            Item::Folder { last_selected, .. } => last_selected,
            Item::File { .. } => panic!(),
        }
    }
    fn selected(&mut self) -> &mut bool {
        match self {
            Item::Folder { .. } => panic!(),
            Item::File {
                inner: File { selected, .. },
            } => selected,
        }
    }
    fn generate_indexers(&self) -> Box<dyn DoubleEndedIterator<Item = Indexer> + '_> {
        match self {
            Item::File { .. } => Box::new(once(Indexer::new())),
            Item::Folder { items, .. } => {
                Box::new(items.iter().enumerate().flat_map(|(i, item)| {
                    item.generate_indexers()
                        .into_iter()
                        .map(move |mut indexer| {
                            indexer.push(i);
                            indexer
                        })
                }))
            }
        }
    }
    fn get(&self, mut indexer: Indexer) -> Option<&Self> {
        if indexer.is_empty() {
            Some(self)
        } else {
            let i = indexer.pop();
            match self {
                Item::Folder { items, .. } => items[i].get(indexer),
                Item::File { .. } => None,
            }
        }
    }
    fn get_mut(&mut self, mut indexer: Indexer) -> Option<&mut Self> {
        if indexer.is_empty() {
            Some(self)
        } else {
            let i = indexer.pop();
            match self {
                Item::Folder { items, .. } => items[i].get_mut(indexer),
                Item::File { .. } => None,
            }
        }
    }
    fn insert(&mut self, item: Self) -> Result<()> {
        let path = item.path();
        let parent = self.get_by_path_mut(path.parent().context("path had no parent")?)?;
        let Item::Folder { items, .. } = parent else {
            bail!("parent was not a folder");
        };
        items.push(item);
        Ok(())
    }
    fn remove_by_path(&mut self, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let parent = self.get_by_path_mut(path.parent().context("path had no parent")?)?;
        let Item::Folder { items, .. } = parent else {
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
    fn get_by_path_mut(&mut self, path: impl AsRef<Path>) -> Result<&mut Self> {
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
            Item::Folder { path, .. } => path,
            Item::File {
                inner: File { path, .. },
            } => path,
        }
    }
    fn path(&self) -> &Path {
        match self {
            Item::Folder { path, .. } => path,
            Item::File {
                inner: File { path, .. },
            } => path,
        }
    }
    fn iter_children<'a>(&'a self) -> slice::Iter<'a, Item<T>> {
        match self {
            Item::Folder { items, .. } => items.iter(),
            Item::File { .. } => (&[]).iter(),
        }
    }
    fn iter_files<'a>(&'a self) -> FilesIter<'a, T> {
        FilesIter::new(self)
    }
    fn iter_files_mut<'a>(&'a mut self) -> FilesIterMut<'a, T> {
        FilesIterMut::new(self)
    }
    fn iter_children_mut<'a>(&'a mut self) -> slice::IterMut<'a, Item<T>> {
        match self {
            Item::Folder { items, .. } => items.iter_mut(),
            Item::File { .. } => (&mut []).iter_mut(),
        }
    }
    fn load_entry(
        path: impl AsRef<Path>,
        load_callback: &impl Fn(&Path) -> Option<T>,
    ) -> Result<Option<Self>> {
        let file_type = std::fs::metadata(&path)?.file_type();
        Ok(if file_type.is_file() {
            File::load_path(path, load_callback).map(|inner| Self::File { inner })
        } else if file_type.is_dir() {
            Some(Self::load_dir(path, load_callback)?)
        } else {
            error!("Encountered a symlink");
            None
        })
    }
    fn load_dir(
        path: impl AsRef<Path>,
        load_callback: &impl Fn(&Path) -> Option<T>,
    ) -> Result<Self> {
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if let Some(item) = Self::load_entry(entry.path(), load_callback)? {
                items.push(item);
            }
        }
        Ok(Self::Folder {
            items,
            path: path.as_ref().into(),
            last_selected: None,
        })
    }
}
impl<T> Index<Indexer> for Item<T> {
    type Output = Item<T>;

    fn index(&self, index: Indexer) -> &Self::Output {
        self.get(index).unwrap()
    }
}
impl<T> IndexMut<Indexer> for Item<T> {
    fn index_mut(&mut self, index: Indexer) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}

pub struct FilesIter<'a, T> {
    inner: FilesIterInner<'a, T>,
}
enum FilesIterInner<'a, T> {
    Empty,
    File(Once<&'a File<T>>),
    Folder(
        Box<
            FlatMap<
                slice::Iter<'a, Item<T>>,
                FilesIter<'a, T>,
                fn(&'a Item<T>) -> FilesIter<'a, T>,
            >,
        >,
    ),
}
impl<'a, T> FilesIter<'a, T> {
    fn new(item: &'a Item<T>) -> Self {
        let inner = match item {
            Item::File { inner } => FilesIterInner::File(once(inner)),
            Item::Folder { items, .. } => {
                FilesIterInner::Folder(Box::new(items.iter().flat_map(Item::iter_files)))
            }
        };
        Self { inner }
    }
    fn empty() -> Self {
        Self {
            inner: FilesIterInner::Empty,
        }
    }
}
impl<'a, T> Iterator for FilesIter<'a, T> {
    type Item = &'a File<T>;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            FilesIterInner::Empty => None,
            FilesIterInner::File(once) => once.next(),
            FilesIterInner::Folder(flat_map) => flat_map.next(),
        }
    }
}
impl<'a, T> DoubleEndedIterator for FilesIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            FilesIterInner::Empty => None,
            FilesIterInner::File(once) => once.next(),
            FilesIterInner::Folder(flat_map) => flat_map.next(),
        }
    }
}

pub struct FilesIterMut<'a, T> {
    inner: FilesIterMutInner<'a, T>,
}
enum FilesIterMutInner<'a, T> {
    Empty,
    File(Once<&'a mut File<T>>),
    Folder(
        Box<
            FlatMap<
                slice::IterMut<'a, Item<T>>,
                FilesIterMut<'a, T>,
                fn(&'a mut Item<T>) -> FilesIterMut<'a, T>,
            >,
        >,
    ),
}
impl<'a, T> FilesIterMut<'a, T> {
    fn new(item: &'a mut Item<T>) -> Self {
        let inner = match item {
            Item::File { inner } => FilesIterMutInner::File(once(inner)),
            Item::Folder { items, .. } => FilesIterMutInner::Folder(Box::new(
                items.iter_mut().flat_map(Item::iter_files_mut),
            )),
        };
        Self { inner }
    }
    fn empty() -> Self {
        Self {
            inner: FilesIterMutInner::Empty,
        }
    }
}
impl<'a, T> Iterator for FilesIterMut<'a, T> {
    type Item = &'a mut File<T>;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            FilesIterMutInner::Empty => None,
            FilesIterMutInner::File(once) => once.next(),
            FilesIterMutInner::Folder(flat_map) => flat_map.next(),
        }
    }
}
impl<'a, T> DoubleEndedIterator for FilesIterMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            FilesIterMutInner::Empty => None,
            FilesIterMutInner::File(once) => once.next(),
            FilesIterMutInner::Folder(flat_map) => flat_map.next(),
        }
    }
}

fn list_item<'a, T>(ui: &mut Ui, file: &'a File<T>) -> Response {
    let name = file.path().file_stem().unwrap().to_string_lossy();
    let atoms = (
        Image::new(egui::include_image!("../../assets/scan_image_icon.png")),
        egui::WidgetText::Text(name.to_string()),
    )
        .into_atoms();

    let id = egui::Id::new(file.uuid);
    let mut layout = AtomLayout::new(atoms)
        .id(id)
        .sense(Sense::click())
        .wrap_mode(egui::TextWrapMode::Extend)
        .fallback_font(TextStyle::Button);

    let selected = file.selected;
    let min_size = egui::Vec2::new(0., ui.spacing().interact_size.y);

    layout.map_atoms(|atom| {
        if matches!(&atom.kind, AtomKind::Image(_)) {
            atom.atom_max_height_font_size(ui)
        } else {
            atom
        }
    });

    let button_padding = ui.spacing().button_padding;

    let mut prepared = layout
        .frame(Frame::new().inner_margin(button_padding))
        .min_size(min_size)
        .allocate(ui);

    if ui.is_rect_visible(prepared.response.rect) {
        let visuals = ui.style().interact_selectable(&prepared.response, selected);

        prepared.map_images(|image| image.tint(visuals.text_color()));

        prepared.fallback_text_color = visuals.text_color();

        prepared.frame = prepared
            .frame
            .inner_margin(
                button_padding + egui::Vec2::splat(visuals.expansion)
                    - egui::Vec2::splat(visuals.bg_stroke.width),
            )
            .outer_margin(-egui::Vec2::splat(visuals.expansion))
            .fill(visuals.weak_bg_fill)
            .stroke(visuals.bg_stroke)
            .corner_radius(visuals.corner_radius);

        prepared.paint(ui)
    } else {
        AtomLayoutResponse::empty(prepared.response)
    }
    .response
}
