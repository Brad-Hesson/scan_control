use egui::Ui;
use glam::{DAffine2, DMat3, DVec2};

use crate::{
    connection::{LiveImage, ScanArea},
    scan_view::{FileImage, GDSImage, ImageEncoder},
};

pub enum Object {
    Gds(GDSImage),
    File(FileImage),
    ScanImage(LiveImage),
    ScanArea(ScanArea),
}

impl Object {
    pub fn name(&self) -> &str {
        match self {
            Object::Gds(gdsimage) => "gds_image",
            Object::File(file_image) => &file_image.name,
            Object::ScanImage(live_image) => "live_image",
            Object::ScanArea(scan_area) => "scan_area",
        }
    }
    pub fn is_scalable(&self) -> bool {
        match self {
            Object::Gds(_) => false,
            Object::File(_) => true,
            Object::ScanImage(_) => false,
            Object::ScanArea(_) => false,
        }
    }
    pub fn as_scan_area(&self) -> Option<&ScanArea> {
        match self {
            Object::ScanArea(scan_area) => Some(scan_area),
            _ => None,
        }
    }
    pub fn as_scan_area_mut(&mut self) -> Option<&mut ScanArea> {
        match self {
            Object::ScanArea(scan_area) => Some(scan_area),
            _ => None,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        match self {
            Object::Gds(gdsimage) => gdsimage.show(ui),
            Object::File(file_image) => file_image.show(ui),
            Object::ScanImage(live_image) => {
                live_image.show_image(ui);
            }
            Object::ScanArea(scan_area) => scan_area.show(ui),
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        match self {
            Object::Gds(gdsimage) => {
                ui.label("gds image menu");
            }
            Object::File(file_image) => file_image.show_menu(ui),
            Object::ScanImage(live_image) => live_image.show_menu(ui, encoder),
            Object::ScanArea(scan_area) => {}
        }
    }
    pub fn border_transform(&self) -> Option<DAffine2> {
        match self {
            Object::ScanImage(live_image) => Some(live_image.transform),
            Object::ScanArea(scan_area) => {
                Some(scan_area.world_transform * DAffine2::from_scale(scan_area.area_size))
            }
            _ => None,
        }
    }
    pub fn transform_center(&self) -> DVec2 {
        match self {
            Object::Gds(gdsimage) => gdsimage.transform.translation,
            Object::File(file_image) => file_image.center(),
            Object::ScanImage(live_image) => live_image.transform.translation,
            Object::ScanArea(scan_area) => scan_area.world_transform.translation,
        }
    }
    pub fn apply_transform(&mut self, tran: DAffine2) {
        match self {
            Object::Gds(gdsimage) => gdsimage.transform = tran * gdsimage.transform,
            Object::File(file_image) => file_image.transform_world_points(tran),
            Object::ScanImage(live_image) => live_image.transform = tran * live_image.transform,
            Object::ScanArea(scan_area) => {
                scan_area.world_transform = tran * scan_area.world_transform
            }
        }
    }
    pub fn goto_transform(&self) -> DAffine2 {
        match self {
            Object::Gds(gdsimage) => DAffine2::from_scale_angle_translation(
                DVec2::splat(gdsimage.scale),
                0.,
                gdsimage.transform.translation,
            ),
            Object::ScanImage(live_image) => {
                live_image.transform * DAffine2::from_scale(DVec2::new(1., -1.))
            }
            Object::ScanArea(scan_area) => {
                scan_area.world_transform
                    * DAffine2::from_scale(DVec2::splat(
                        (scan_area.area_size.x + scan_area.area_size.y) / 2.,
                    ))
            }
            Object::File(file_image) => {
                let center = file_image.center();
                let scale = file_image
                    .world_points
                    .iter()
                    .map(|wp| wp.distance(center))
                    .sum::<f64>()
                    / 4.
                    * 2.
                    / 2f64.sqrt();
                DAffine2::from_scale_angle_translation(DVec2::splat(scale), 0., center)
            }
        }
    }
}
