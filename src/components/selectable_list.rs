use std::{
    hash::Hash,
    ops::{Deref, DerefMut},
};

use egui::{
    AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, Atoms, Context, Frame, Response, Sense,
    TextStyle, Ui,
};

use crate::utils::response_group::{ResponseGroup, ResponseGroupExt};

pub struct SelectableList<T> {
    items: Vec<SelectableEntry<T>>,
    last_selected: Option<usize>,
}

impl<T> SelectableList<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            last_selected: None,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        for i in (0..self.items.len()).rev() {
            let mut resp = list_item(ui, &self.items[i]).synchronize(&mut self.items[i].resp_group);
            if resp.sync.hovered() {
                resp.orig = resp.orig.highlight();
            }
            if resp.orig.clicked() {
                if ui.input(|i| i.modifiers.shift) && self.last_selected.is_some() {
                    let mut i = i as isize;
                    let last = self.last_selected.unwrap() as isize;
                    let add = ((i < last) as isize) * 2 - 1;
                    while i != last {
                        self.items[i as usize].selected = true;
                        i += add;
                    }
                } else if ui.input(|i| i.modifiers.ctrl) {
                    self[i].selected = !self[i].selected;
                } else {
                    self.clear_selected();
                    self[i].selected = true;
                }
                if self[i].selected {
                    self.last_selected = Some(i);
                }
            }
        }
    }
    pub fn get_hovered(&self, ctx: &Context) -> Option<&SelectableEntry<T>> {
        self.items
            .iter()
            .find_map(|item| item.resp_group.response(ctx)?.hovered().then_some(item))
    }
    pub fn clear_selected(&mut self) {
        for item in &mut self.items {
            item.selected = false;
        }
        self.last_selected = None;
    }
    pub fn iter_selected_indexes<'a>(&'a self) -> impl DoubleEndedIterator<Item = usize> + 'a {
        self.iter()
            .enumerate()
            .filter_map(|(i, item)| item.selected.then_some(i))
    }
    pub fn iter_selected(&self) -> impl Iterator<Item = &SelectableEntry<T>> {
        self.iter().filter(|item| item.selected)
    }
    pub fn move_indexes_up(&mut self, indexes: &[usize]) -> Vec<usize> {
        let mut moved = Vec::new();
        for (i, index) in indexes.iter().copied().enumerate() {
            if i == index {
                continue;
            }
            self.swap(index, index - 1);
            moved.push(index - 1);
        }
        moved
    }
    pub fn move_indexes_down(&mut self, indexes: &[usize]) -> Vec<usize> {
        let mut moved = Vec::new();
        for (i, index) in indexes.iter().copied().rev().enumerate() {
            let i = self.len() - i - 1;
            if i == index {
                continue;
            }
            self.swap(index, index + 1);
            moved.push(index + 1);
        }
        moved.reverse();
        moved
    }
}
impl<T> Deref for SelectableList<T> {
    type Target = Vec<SelectableEntry<T>>;

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}
impl<T> DerefMut for SelectableList<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items
    }
}

pub struct SelectableEntry<T> {
    id: egui::Id,
    pub inner: T,
    pub selected: bool,
    pub resp_group: ResponseGroup,
    construct_fn: Box<dyn Fn(&T) -> Atoms>,
}
impl<T> SelectableEntry<T> {
    pub fn new(id_salt: impl Hash, data: T, construct_fn: impl Fn(&T) -> Atoms + 'static) -> Self {
        Self {
            id: egui::Id::new(id_salt),
            inner: data,
            selected: false,
            construct_fn: Box::new(construct_fn),
            resp_group: ResponseGroup::new(),
        }
    }
}
impl<T> Deref for SelectableEntry<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<T> DerefMut for SelectableEntry<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

fn list_item<'a, T>(ui: &mut Ui, item: &'a SelectableEntry<T>) -> Response {
    let atoms = (item.construct_fn)(&item.inner);
    let id = egui::Id::new(atoms.text());
    let mut layout = AtomLayout::new(atoms)
        .id(id)
        .sense(Sense::click())
        .wrap_mode(egui::TextWrapMode::Extend)
        .fallback_font(TextStyle::Button);

    let selected = item.selected;
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
