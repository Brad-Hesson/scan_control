use std::ops::{Bound, Deref, DerefMut, RangeBounds};

use egui::{Atoms, Response, Ui, Widget};
use itertools::Itertools;

pub struct SelectableList<T> {
    items: Vec<SelectableEntry<T>>,
}

impl<T> SelectableList<T> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        for item in &mut self.items {
            item.hovered = false;
        }
        for i in (0..self.items.len()).into_iter().rev() {
            let mut response = ui.add(list_item(&self.items[i]));
            if response.hovered() {
                self.items[i].hovered = true;
                response = response.highlight();
            }
            if response.clicked() {
                if !ui.input(|i| i.modifiers.ctrl){
                    self.clear_selected();
                }
                self[i].selected = true;
            }
            self.items[i].response = Some(response);
        }
    }
    pub fn clear_selected(&mut self) {
        for item in &mut self.items {
            item.selected = false;
        }
    }
    pub fn iter_selected_indexes<'a>(&'a self) -> impl Iterator<Item = usize> + 'a {
        self.iter()
            .enumerate()
            .filter_map(|(i, item)| item.selected.then_some(i))
    }
    pub fn set_hovered(&mut self, i: usize) {
        self.items[i].hovered = true;
        if let Some(resp) = self.items[i].response.take() {
            self.items[i].response = Some(resp.highlight());
        }
    }
    pub fn get_hovered(&self) -> Option<&SelectableEntry<T>> {
        self.iter().find(|e| e.hovered)
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
        for (i, index) in indexes.into_iter().copied().enumerate().rev() {
            dbg!(i, index);
            if i == index {
                continue;
            }
            self.swap(index, index + 1);
            moved.push(index + 1);
        }
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
    hovered: bool,
    response: Option<Response>,
    construct_fn: Box<dyn Fn(&T) -> Atoms>,
}
impl<T> SelectableEntry<T> {
    pub fn new(data: T, construct_fn: impl Fn(&T) -> Atoms + 'static) -> Self {
        Self {
            inner: data,
            selected: false,
            hovered: false,
            response: None,
            construct_fn: Box::new(construct_fn),
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
