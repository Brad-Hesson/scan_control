use egui::Ui;

use crate::{
    connection::{LiveImage, ScanArea},
    scan_view::{FileImage, GDSImage, ImageEncoder},
};

pub enum Object {
    Gds(GDSImage),
    File(FileImage),
    Scan(LiveImage),
    Area(ScanArea),
}

impl Object {
    pub fn name(&self) -> &str {
        match self {
            Object::Gds(gdsimage) => "gds_image",
            Object::File(file_image) => &file_image.name,
            Object::Scan(live_image) => "live_image",
            Object::Area(scan_area) => "scan_area",
        }
    }
    pub fn as_area(&self) -> Option<&ScanArea> {
        match self {
            Object::Area(scan_area) => Some(scan_area),
            _ => None,
        }
    }
    pub fn as_area_mut(&mut self) -> Option<&mut ScanArea> {
        match self {
            Object::Area(scan_area) => Some(scan_area),
            _ => None,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        match self {
            Object::Gds(gdsimage) => gdsimage.show(ui),
            Object::File(file_image) => file_image.show(ui),
            Object::Scan(live_image) => {
                live_image.show_image(ui);
            }
            Object::Area(scan_area) => scan_area.show(ui),
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        match self {
            Object::Gds(gdsimage) => {
                ui.label("gds image menu");
            }
            Object::File(file_image) => file_image.show_menu(ui),
            Object::Scan(live_image) => live_image.show_menu(ui, encoder),
            Object::Area(scan_area) => scan_area.show_menu(ui, encoder),
        }
    }
}
