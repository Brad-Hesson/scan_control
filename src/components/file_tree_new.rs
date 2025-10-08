use std::{
    borrow::Cow,
    fmt::Debug,
    iter::repeat,
    ops::{Add, AddAssign, Deref, DerefMut, Index, IndexMut},
    path::{Path, PathBuf},
    sync::mpsc,
};

use egui::{
    AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, CollapsingHeader, Context, Frame,
    Image, IntoAtoms as _, Response, Sense, TextStyle, Ui,
};
use eyre::{Context as _, ContextCompat, Result};
use itertools::Itertools;
use notify_typed::{Event, EventWatcher, RecursiveMode};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    app::OkTraceExt as _,
    utils::response_group::{ResponseGroup, ResponseGroupExt, SyncResponse},
};

pub struct FileTree<T> {
    tree: Option<Folder<T>>,
    file_event_rx: mpsc::Receiver<Event>,
    watcher: EventWatcher,
    load_callback: Box<dyn Fn(&Path) -> Option<T>>,
    last_selected: Option<usize>,
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
            tree: None,
            watcher,
            file_event_rx: rx,
            load_callback: Box::new(load_callback),
            last_selected: None,
        })
    }
    pub fn load_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let Item::Folder { folder } = Item::load_path(&path, &self.load_callback)
            .with_context(|| format!("failed to load path `{}`", path.display()))?
            .unwrap()
        else {
            panic!()
        };
        if let Some(folder) = self.tree.as_ref() {
            let prev_path = &folder.path;
            self.watcher
                .unwatch(&prev_path)
                .with_context(|| format!("failed to unwatch the path `{}`", prev_path.display()))?;
        }
        self.watcher
            .watch(&path, RecursiveMode::Recursive)
            .with_context(|| format!("failed to start watching the path `{}`", path.display()))?;
        self.tree = Some(folder);
        Ok(())
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.tree
            .as_ref()
            .map(|folder| folder.file_count())
            .unwrap_or_default()
    }
    #[inline]
    pub fn get(&self, i: usize) -> Option<&File<T>> {
        self.tree.as_ref().and_then(|folder| folder.get_file(i))
    }
    #[inline]
    pub fn get_mut(&mut self, i: usize) -> Option<&mut File<T>> {
        self.tree.as_mut().and_then(|folder| folder.get_file_mut(i))
    }
    #[inline]
    pub fn show(&mut self, ui: &mut Ui) {
        self.update();
        self.show_list(
            ui,
            0,
            self.tree
                .as_ref()
                .map(|folder| folder.children.len())
                .unwrap_or_default(),
        );
    }
    fn show_item(&mut self, ui: &mut Ui, i: usize) -> SyncResponse {
        match self
            .tree
            .as_mut()
            .and_then(|folder| folder.get_item_mut(i))
            .unwrap()
        {
            Item::Folder { folder } => {
                let name = folder.path.file_stem().unwrap().to_string_lossy();
                let len = folder.children.len();
                let resp = CollapsingHeader::new(name)
                    .default_open(true)
                    .show(ui, |ui| {
                        self.show_list(ui, i + 1, len);
                    })
                    .header_response;
                SyncResponse {
                    orig: resp.clone(),
                    sync: resp,
                }
            }
            Item::File { file } => list_item(ui, file).synchronize(&mut file.resp_group),
        }
    }
    fn show_list(&mut self, ui: &mut Ui, start: usize, len: usize) {
        macro_rules! item {
            ($i:expr) => {
                self.tree.as_mut().unwrap().get_item_mut($i).unwrap()
            };
            ($i:expr; file) => {
                item!($i).as_file_mut().unwrap()
            };
            ($i:expr; folder) => {
                item!($i).as_folder_mut().unwrap()
            };
        }
        let mut i = start;
        for _ in 0..len {
            let mut resp = self.show_item(ui, i);
            if resp.sync.hovered() {
                resp.orig = resp.orig.highlight();
            }
            if resp.orig.clicked() && item!(i).as_folder().is_none() {
                if ui.input(|i| i.modifiers.shift) && self.last_selected.is_some() {
                    let mut i = i as isize;
                    let last = self.last_selected.unwrap() as isize;
                    let add = ((i < last) as isize) * 2 - 1;
                    self.clear_selected();
                    while i != last + add {
                        if let Some(file) = item!(i as usize).as_file_mut() {
                            file.selected = true;
                        }
                        i += add;
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    item!(i; file).selected = !item!(i; file).selected;
                } else {
                    self.clear_selected();
                    item!(i; file).selected = true;
                }
                if item!(i; file).selected && !ui.input(|i| i.modifiers.shift) {
                    self.last_selected = Some(i);
                }
            }
            i += item!(i).item_count() + 1;
        }
    }
    fn update(&mut self) {
        let Some(tree) = self.tree.as_mut() else {
            return;
        };
        for event in self.file_event_rx.try_iter() {
            info!("{event:?}");
            match event {
                Event::Create { path } => {
                    let Some(item) = Item::load_path(path, &self.load_callback)
                        .ok_trace()
                        .flatten()
                    else {
                        continue;
                    };
                    for i in 0..tree.item_count() {
                        if tree.get_item(i).unwrap().path() == item.path().parent().unwrap() {
                            tree.get_item_mut(i)
                                .unwrap()
                                .as_folder_mut()
                                .unwrap()
                                .children
                                .push(item);
                            break;
                        }
                    }
                }
                Event::Rename { from, to } => {
                    for i in 0..tree.file_count() {
                        if tree.get_item(i).unwrap().path() == from {
                            tree.get_item_mut(i).unwrap().rename(to);
                            break;
                        }
                    }
                }
                Event::Move { from, to } => {
                    let mut item = None;
                    for i in 0..tree.item_count() {
                        if tree.get_item(i).unwrap().path() == from.parent().unwrap() {
                            item = tree
                                .get_item_mut(i)
                                .unwrap()
                                .as_folder_mut()
                                .unwrap()
                                .children
                                .extract_if(.., |it| it.path() == from)
                                .exactly_one()
                                .ok_trace();
                            break;
                        }
                    }
                    let Some(mut item) = item else {
                        todo!("parent folder is not iterated so is never found as the parent");
                        error!("Never found parent during move event");
                        continue;
                    };
                    item.rename(&to);
                    for i in 0..tree.item_count() {
                        if tree.get_item(i).unwrap().path() == item.path().parent().unwrap() {
                            tree.get_item_mut(i)
                                .unwrap()
                                .as_folder_mut()
                                .unwrap()
                                .children
                                .push(item);
                            break;
                        }
                    }
                }
                Event::Delete { path } => {
                    for i in 0..tree.item_count() {
                        if tree.get_item(i).unwrap().path() == path.parent().unwrap() {
                            tree.get_item_mut(i)
                                .unwrap()
                                .as_folder_mut()
                                .unwrap()
                                .children
                                .retain(|it| it.path() != path);
                            break;
                        }
                    }
                }
            }
        }
    }
    pub fn clear_selected(&mut self) {
        for i in 0..self.len() {
            self[i].selected = false;
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &File<T>> {
        (0..self.len()).map(|i| &self[i])
    }
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut File<T>> {
        let ptr = self as *mut Self;
        (0..self.len()).map(move |i| &mut unsafe { &mut *ptr }[i])
    }
    pub fn get_hovered(&self, ctx: &Context) -> Option<&File<T>> {
        self.iter()
            .find_map(|file| file.resp_group.response(ctx)?.hovered().then_some(file))
    }
}
impl<T> Index<usize> for FileTree<T> {
    type Output = File<T>;
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).unwrap()
    }
}
impl<T> IndexMut<usize> for FileTree<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}
impl<'a, T> IntoIterator for &'a FileTree<T> {
    type Item = &'a File<T>;

    type IntoIter = Box<dyn DoubleEndedIterator<Item = &'a File<T>> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}
impl<'a, T> IntoIterator for &'a mut FileTree<T> {
    type Item = &'a mut File<T>;

    type IntoIter = Box<dyn DoubleEndedIterator<Item = &'a mut File<T>> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter_mut())
    }
}

pub struct File<T> {
    path: PathBuf,
    uuid: Uuid,
    pub data: T,
    pub selected: bool,
    pub resp_group: ResponseGroup,
}
impl<T> File<T> {
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }
    #[inline]
    pub fn file_stem(&self) -> Cow<'_, str> {
        self.path.file_stem().unwrap().to_string_lossy()
    }
    fn new(path: PathBuf, data: T) -> Self {
        Self {
            path,
            uuid: Uuid::new_v4(),
            data,
            selected: false,
            resp_group: ResponseGroup::new(),
        }
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

struct Folder<T> {
    path: PathBuf,
    children: Vec<Item<T>>,
}
impl<T> Folder<T> {
    fn new(path: PathBuf, children: Vec<Item<T>>) -> Self {
        Self { path, children }
    }
    fn item_count(&self) -> usize {
        self.children
            .iter()
            .map(|child| match child {
                item @ Item::Folder { .. } => item.item_count() + 1,
                Item::File { .. } => 1,
            })
            .sum()
    }
    fn get_item(&self, mut i: usize) -> Option<&Item<T>> {
        for child in &self.children {
            if i == 0 {
                return Some(child);
            }
            i -= 1;
            if matches!(child, Item::Folder { .. }) {
                let len = child.item_count();
                if i < len {
                    return child.get_item(i);
                }
                i -= len;
            }
        }
        None
    }
    fn get_item_mut(&mut self, mut i: usize) -> Option<&mut Item<T>> {
        for child in &mut self.children {
            if i == 0 {
                return Some(child);
            }
            i -= 1;
            if matches!(child, Item::Folder { .. }) {
                let len = child.item_count();
                if i < len {
                    return child.get_item_mut(i);
                }
                i -= len;
            }
        }
        None
    }
    fn file_count(&self) -> usize {
        self.children
            .iter()
            .map(|child| match child {
                Item::Folder { folder: inner } => inner.file_count(),
                Item::File { .. } => 1,
            })
            .sum()
    }
    fn get_file(&self, mut i: usize) -> Option<&File<T>> {
        for item in &self.children {
            match item {
                Item::File { file } => {
                    if i == 0 {
                        return Some(file);
                    }
                    i -= 1;
                }
                Item::Folder { folder } => {
                    let len = folder.file_count();
                    if i < len {
                        return folder.get_file(i);
                    }
                    i -= len;
                }
            }
        }
        None
    }
    fn get_file_mut(&mut self, mut i: usize) -> Option<&mut File<T>> {
        for item in &mut self.children {
            match item {
                Item::File { file } => {
                    if i == 0 {
                        return Some(file);
                    }
                    i -= 1;
                }
                Item::Folder { folder } => {
                    let len = folder.file_count();
                    if i < len {
                        return folder.get_file_mut(i);
                    }
                    i -= len;
                }
            }
        }
        None
    }
}

enum Item<T> {
    Folder { folder: Folder<T> },
    File { file: File<T> },
}
impl<T> Item<T> {
    #[inline]
    fn as_folder(&self) -> Option<&Folder<T>> {
        match self {
            Item::Folder { folder } => Some(folder),
            Item::File { .. } => None,
        }
    }
    #[inline]
    fn as_folder_mut(&mut self) -> Option<&mut Folder<T>> {
        match self {
            Item::Folder { folder } => Some(folder),
            Item::File { .. } => None,
        }
    }
    #[inline]
    fn as_file_mut(&mut self) -> Option<&mut File<T>> {
        match self {
            Item::Folder { .. } => None,
            Item::File { file } => Some(file),
        }
    }
    #[inline]
    fn item_count(&self) -> usize {
        self.as_folder().map(|f| f.item_count()).unwrap_or(0)
    }
    #[inline]
    fn get_item(&self, i: usize) -> Option<&Self> {
        self.as_folder().map(|f| f.get_item(i)).flatten()
    }
    #[inline]
    fn get_item_mut(&mut self, i: usize) -> Option<&mut Self> {
        self.as_folder_mut().map(|f| f.get_item_mut(i)).flatten()
    }
    #[inline]
    fn path(&self) -> &Path {
        match self {
            Item::Folder { folder } => &folder.path,
            Item::File { file } => &file.path,
        }
    }
    #[inline]
    fn path_mut(&mut self) -> &mut PathBuf {
        match self {
            Item::Folder { folder } => &mut folder.path,
            Item::File { file } => &mut file.path,
        }
    }
    fn rename(&mut self, path: impl AsRef<Path>) {
        *self.path_mut() = path.as_ref().to_path_buf();
        if let Some(folder) = self.as_folder_mut() {
            for child in &mut folder.children {
                child.rename(folder.path.join(child.path().file_name().unwrap()));
            }
        }
    }
    fn load_path(
        path: impl AsRef<Path>,
        load_fn: &dyn Fn(&Path) -> Option<T>,
    ) -> std::io::Result<Option<Self>> {
        let path = path.as_ref().to_path_buf();
        let meta = std::fs::metadata(&path)?;
        if meta.is_file() {
            let Some(data) = load_fn(&path) else {
                return Ok(None);
            };
            return Ok(Some(Self::File {
                file: File::new(path, data),
            }));
        }
        if meta.is_dir() {
            let children = std::fs::read_dir(&path)?
                .filter_map(|dir| dir.ok_trace())
                .filter_map(|dir| Self::load_path(&dir.path(), load_fn).ok_trace().flatten())
                .collect_vec();
            return Ok(Some(Self::Folder {
                folder: Folder::new(path, children),
            }));
        }
        return Err(std::io::Error::other("not a file or directory"));
    }
}
