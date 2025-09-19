use std::ops::{Deref, DerefMut};

use egui::{Atoms, Context, Ui, Widget};

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
        for i in (0..self.items.len()).into_iter().rev() {
            let mut response = ui
                .add(list_item(&self.items[i]))
                .synchronize(&mut self.items[i].resp_group);
            if response.hovered() {
                response = response.highlight();
            }
            if response.clicked() {
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
    pub fn iter_selected_indexes<'a>(&'a self) -> impl Iterator<Item = usize> + 'a {
        self.iter()
            .enumerate()
            .filter_map(|(i, item)| item.selected.then_some(i))
    }
    pub fn iter_selected(&self) -> impl Iterator<Item = &SelectableEntry<T>> {
        self.iter().filter_map(|item| item.selected.then_some(item))
    }
    pub fn move_indexes_up(&mut self, indexes: &[usize]) -> Vec<usize> {
        let mut moved = Vec::new();
        for (i, index) in indexes.into_iter().copied().enumerate() {
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
        for (i, index) in indexes.into_iter().copied().rev().enumerate() {
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
    pub inner: T,
    pub selected: bool,
    pub resp_group: ResponseGroup,
    construct_fn: Box<dyn Fn(&T) -> Atoms>,
}
impl<T> SelectableEntry<T> {
    pub fn new(data: T, construct_fn: impl Fn(&T) -> Atoms + 'static) -> Self {
        Self {
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

fn list_item<'a, T>(item: &'a SelectableEntry<T>) -> impl Widget + 'a {
    egui::widgets::Button::new((item.construct_fn)(&item.inner))
        .frame(true)
        .selected(item.selected)
        .wrap_mode(egui::TextWrapMode::Extend)
}
