use std::{cell::RefCell, path::Path};

use egui::{
    AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, CollapsingHeader, Frame, Image,
    IntoAtoms as _, Response, Sense, TextStyle, Ui,
};
use eyre::{Context, Result};
use filetree::{EntryRef, EntryRefMut, FileRef, FileRefMut, FileTree};
use sxmfile::SXM;
use uuid::Uuid;

use crate::{
    app::{OkTraceExt, StaticImage},
    scan_view::ImageEncoder,
    utils::response_group::{ResponseGroup, ResponseGroupExt as _, SyncResponse},
};

pub struct ImageTree {
    tree: FileTree<FileData, ()>,
    last_selected: RefCell<Option<usize>>,
}
impl ImageTree {
    pub fn new(image_encoder: ImageEncoder) -> Self {
        let load_fn = move |path: &Path| {
            if !path.extension().is_some_and(|ext| ext == "sxm") {
                return None;
            }
            let sxm = SXM::parse_file(path)
                .with_context(|| format!("failed to load `{}`", path.display()))
                .ok_trace()?;
            let out = StaticImage::load_sxm(sxm, &image_encoder).ok_trace()?;
            out.update_texture(&image_encoder);
            Some(FileData::new(out))
        };
        Self {
            last_selected: RefCell::new(None),
            tree: FileTree::new(load_fn, |_| Some(())),
        }
    }
    pub fn load_dir(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.tree
            .load_dir(&path)
            .with_context(|| format!("failed to load path `{}`", path.as_ref().display()))
    }
    pub fn iter_images(&self) -> impl DoubleEndedIterator<Item = FileRefMut<'_, FileData>> {
        self.tree.iter_files_mut()
    }
    pub fn show(&self, ui: &mut Ui) {
        let Some(root) = self.tree.root() else { return };
        self.show_children_of(ui, root.path);
    }
    fn show_entry(&self, ui: &mut Ui, path: impl AsRef<Path>) -> SyncResponse {
        match self.tree.get_mut(path).unwrap() {
            EntryRefMut::Dir { dir } => {
                let name = dir.path.file_stem().unwrap().to_string_lossy();
                let resp = CollapsingHeader::new(name)
                    .default_open(true)
                    .show(ui, |ui| {
                        self.show_children_of(ui, dir.path);
                    })
                    .header_response;
                SyncResponse {
                    orig: resp.clone(),
                    sync: resp,
                }
            }
            EntryRefMut::File { mut file } => {
                list_item(ui, &file).synchronize(&mut file.data.resp_group)
            }
        }
    }
    fn show_children_of(&self, ui: &mut Ui, path: impl AsRef<Path>) {
        for child in self.tree.iter_children_of_mut(path) {
            let mut resp = self.show_entry(ui, child.path());
            if resp.sync.hovered() {
                resp.orig = resp.orig.highlight();
            }
            if let EntryRefMut::File { file: mut child } = child {
                if resp.orig.clicked() {
                    if ui.input(|i| i.modifiers.shift) && self.last_selected.borrow().is_some() {
                        let mut i = self.tree.index_of(child.path).unwrap() as isize;
                        let last = self.last_selected.borrow().unwrap() as isize;
                        let add = ((i < last) as isize) * 2 - 1;
                        self.clear_selected();
                        while i != last + add {
                            if let Some(file) =
                                self.tree.get_index_mut(i as usize).unwrap().as_file()
                            {
                                file.data.selected = true;
                            }
                            i += add;
                        }
                    } else if ui.input(|i| i.modifiers.ctrl) {
                        child.data.selected = !child.data.selected;
                    } else {
                        self.clear_selected();
                        child.data.selected = true;
                    }
                    if child.data.selected && !ui.input(|i| i.modifiers.shift) {
                        *self.last_selected.borrow_mut() =
                            Some(self.tree.index_of(child.path).unwrap());
                    }
                }
            }
        }
    }
    pub fn clear_selected(&self) {
        for mut file in self.tree.iter_files_mut() {
            file.data.selected = false;
        }
    }
}

pub struct FileData {
    uuid: Uuid,
    pub image: StaticImage,
    pub selected: bool,
    pub resp_group: ResponseGroup,
}
impl FileData {
    fn new(image: StaticImage) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            image,
            selected: false,
            resp_group: ResponseGroup::new(),
        }
    }
}

fn list_item<'a>(ui: &mut Ui, file: &FileRefMut<FileData>) -> Response {
    let name = file.path.file_stem().unwrap().to_string_lossy();
    let atoms = (
        Image::new(egui::include_image!("../../assets/scan_image_icon.png")),
        egui::WidgetText::Text(name.to_string()),
    )
        .into_atoms();

    let id = egui::Id::new(file.data.uuid);
    let mut layout = AtomLayout::new(atoms)
        .id(id)
        .sense(Sense::click())
        .wrap_mode(egui::TextWrapMode::Extend)
        .fallback_font(TextStyle::Button);

    let selected = file.data.selected;
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
