use std::{
    cell::RefCell,
    collections::HashMap,
    ops::{Deref, DerefMut, Index, IndexMut},
    path::{Path, PathBuf},
};

use egui::{
    AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, CollapsingHeader, Context, Frame,
    Image, IntoAtoms as _, Response, Sense, TextStyle, Ui,
};
use eyre::{Context as _, Result};
use filetree::{
    file_uid::{ContentID, FileHasher, IdentityHasher},
    folder_structure::{Child, File, Folder},
    handlers::{LoadHandler, UpdateHandler},
    FileTree,
};
use sxmfile::SXM;
use uuid::Uuid;

use crate::{
    app::{OkTraceExt, StaticImage},
    scan_view::ImageEncoder,
    utils::response_group::{ResponseGroup, ResponseGroupExt as _, SyncResponse},
};

pub struct ImageTree {
    pub tree: FileTree<ContentID, Option<usize>>,
    pub files: FileMap,
    hasher: FileHasher,
    encoder: ImageEncoder,
}
impl ImageTree {
    pub fn new(image_encoder: ImageEncoder) -> Self {
        Self {
            tree: FileTree::new().unwrap(),
            files: FileMap(HashMap::with_hasher(IdentityHasher)),
            hasher: FileHasher::default(),
            encoder: image_encoder,
        }
    }
    pub fn load_dir(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let handler = Handler {
            files: &mut self.files,
            hasher: &mut self.hasher,
            encoder: &self.encoder,
        };
        self.tree
            .load_path(&path, handler)
            .with_context(|| format!("failed to load path `{}`", path.as_ref().display()))
    }
    pub fn show(&mut self, ui: &mut Ui) {
        let handler = Handler {
            files: &mut self.files,
            hasher: &mut self.hasher,
            encoder: &self.encoder,
        };
        self.tree.process_updates(handler).ok_trace();
        if let Some(root) = self.tree.root_mut() {
            show_children_of(ui, &mut self.files, root);
        }
    }
}

fn show_child(
    ui: &mut Ui,
    files: &mut FileMap,
    child: &mut Child<ContentID, Option<usize>>,
) -> SyncResponse {
    match child {
        Child::File {
            file: File { data: id, .. },
        } => {
            let file_data = files.get_mut(&id).unwrap();
            list_item(ui, file_data).synchronize(&mut file_data.resp_group)
        }
        Child::Folder { folder } => {
            let name = folder.path.file_stem().unwrap().to_string_lossy();
            let resp = CollapsingHeader::new(name)
                .default_open(true)
                .show(ui, |ui| {
                    show_children_of(ui, files, folder);
                })
                .header_response;
            SyncResponse {
                orig: resp.clone(),
                sync: resp,
            }
        }
    }
}
fn show_children_of(
    ui: &mut Ui,
    file_map: &mut FileMap,
    folder: &mut Folder<ContentID, Option<usize>>,
) {
    for i in 0..folder.children.len() {
        let mut resp = show_child(ui, file_map, &mut folder.children[i]);
        if resp.sync.hovered() {
            resp.orig = resp.orig.highlight();
        }
        if let Some(id) = folder.children[i].as_file().map(|file| file.data) {
            if resp.orig.clicked() {
                if ui.input(|i| i.modifiers.shift) && folder.data.is_some() {
                    let mut i = i as isize;
                    let last = folder.data.unwrap() as isize;
                    let add = ((i < last) as isize) * 2 - 1;
                    file_map.clear_selected();
                    while i != last + add {
                        set_child_selected(file_map, &mut folder.children[i as usize]);
                        i += add;
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    file_map[id].selected = !file_map[id].selected;
                } else {
                    file_map.clear_selected();
                    file_map[id].selected = true;
                }
                if file_map[id].selected && !ui.input(|i| i.modifiers.shift) {
                    folder.data = Some(i);
                }
            }
        }
    }
}

fn set_child_selected(file_map: &mut FileMap, child: &mut Child<ContentID, Option<usize>>) {
    match child {
        Child::File {
            file: File { data: id, .. },
        } => file_map[*id].selected = true,
        Child::Folder { folder } => {
            for child in &mut folder.children {
                set_child_selected(file_map, child);
            }
        }
    }
}

pub struct FileMap(HashMap<ContentID, FileData, IdentityHasher>);
impl FileMap {
    pub fn clear_selected(&mut self) {
        self.values_mut().for_each(|file| file.selected = false);
    }
    pub fn get_hovered(&self, ctx: &Context) -> Option<&FileData> {
        self.values()
            .find(|file| file.resp_group.response(ctx).unwrap().hovered())
    }
    pub fn iter_selected(&self) -> impl Iterator<Item = &FileData> {
        self.values().filter(|file| file.selected)
    }
    pub fn iter_selected_mut(&mut self) -> impl Iterator<Item = &mut FileData> {
        self.values_mut().filter(|file| file.selected)
    }
}
impl Deref for FileMap {
    type Target = HashMap<ContentID, FileData, IdentityHasher>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for FileMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Index<ContentID> for FileMap {
    type Output = FileData;

    fn index(&self, index: ContentID) -> &Self::Output {
        self.0.get(&index).unwrap()
    }
}
impl IndexMut<ContentID> for FileMap {
    fn index_mut(&mut self, index: ContentID) -> &mut Self::Output {
        self.0.get_mut(&index).unwrap()
    }
}

pub struct FileData {
    uuid: Uuid,
    pub path: PathBuf,
    pub image: StaticImage,
    pub selected: bool,
    pub resp_group: ResponseGroup,
}
impl FileData {
    fn new(image: StaticImage, path: PathBuf) -> Self {
        Self {
            path,
            uuid: Uuid::new_v4(),
            image,
            selected: false,
            resp_group: ResponseGroup::new(),
        }
    }
}

struct Handler<'a> {
    files: &'a mut FileMap,
    hasher: &'a mut FileHasher,
    encoder: &'a ImageEncoder,
}
impl<'a> LoadHandler for Handler<'a> {
    type FileData = ContentID;
    type FolderData = Option<usize>;
    fn load_file(&mut self, path: &Path) -> Option<Self::FileData> {
        if !path.extension().is_some_and(|ext| ext == "sxm") {
            return None;
        }
        let sxm = SXM::parse_file(path)
            .with_context(|| format!("failed to load `{}`", path.display()))
            .ok_trace()?;
        let image = StaticImage::load_sxm(sxm, self.encoder).ok_trace()?;
        image.update_texture(self.encoder);
        let file_data = FileData::new(image, path.to_path_buf());
        let id = self.hasher.hash_file(path).unwrap();
        self.files.insert(id, file_data);
        Some(id)
    }
    fn load_folder(&mut self, _path: &Path) -> Option<Self::FolderData> {
        Some(None)
    }
}
impl<'a> UpdateHandler for Handler<'a> {
    fn rename_file(&mut self, _old: &Path, new: &Path, id: &mut Self::FileData) {
        self.files.get_mut(&id).unwrap().path = new.to_path_buf();
    }
    fn delete_file(&mut self, _path: &Path, id: Self::FileData) {
        self.files.remove(&id);
    }
}

fn list_item<'a>(ui: &mut Ui, file: &mut FileData) -> Response {
    let name = file.path.file_stem().unwrap().to_string_lossy();
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
