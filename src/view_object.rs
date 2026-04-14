use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use egui::{Atoms, Image, IntoAtoms, Ui};
use glam::{DAffine2, DMat3, DVec2};
use tracing::error;

use crate::{
    connection::{LiveImage, ScanArea},
    scan_view::{FileImage, GDSImage, ImageEncoder},
};

pub enum Object {
    Gds { image: GDSImage, path: PathBuf },
    File { image: FileImage, path: PathBuf },
    ScanImage(LiveImage),
    ScanArea(ScanArea),
}

impl Object {
    pub fn import(path: PathBuf, encoder: &ImageEncoder) -> Option<Self> {
        match path.extension().and_then(|os| os.to_str()) {
            Some("gds") | Some("GDS") => {
                let image = GDSImage::new(encoder, &path, DAffine2::IDENTITY);
                Some(Self::Gds { image, path })
            }
            Some("png") | Some("jpeg") | Some("PNG") | Some("JPEG") => {
                let image = FileImage::new(
                    uuid::Uuid::new_v4(),
                    encoder,
                    &path,
                    DAffine2::IDENTITY.into(),
                );
                Some(Self::File { image, path })
            }
            Some(_) => {
                error!("tried to import invalid file type: {}", path.display());
                None
            }
            None => {
                error!(
                    "tried to import file with invalid extension: {}",
                    path.display()
                );
                None
            }
        }
    }
    pub fn list_atoms<'a>(&'a self) -> Atoms<'a> {
        let name = self.name();
        let image = match self {
            Object::Gds { .. } => Image::new(egui::include_image!("../assets/gds_file_icon.png")),
            Object::File{ .. } => Image::new(egui::include_image!("../assets/file_image_icon.png")),
            Object::ScanImage(_) => {
                Image::new(egui::include_image!("../assets/scan_image_icon.png"))
            }
            Object::ScanArea(_) => Image::new(egui::include_image!("../assets/scan_area_icon.png")),
        };
        (image, name).into_atoms()
    }
    pub fn name(&self) -> &str {
        match self {
            Object::Gds { path, .. } => path.file_stem().and_then(|os| os.to_str()).unwrap(),
            Object::File{ path, .. } => path.file_stem().and_then(|os| os.to_str()).unwrap(),
            Object::ScanImage(live_image) => "live_image",
            Object::ScanArea(_) => "Scan Region",
        }
    }
    pub fn is_scalable(&self) -> bool {
        match self {
            Object::Gds { .. } => false,
            Object::File{ .. } => true,
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
            Object::Gds { image, .. } => image.show(ui),
            Object::File{ image, .. } => image.show(ui),
            Object::ScanImage(live_image) => {
                live_image.show_image(ui);
            }
            Object::ScanArea(scan_area) => scan_area.show(ui),
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        match self {
            Object::Gds { path, .. } => {
                ui.label(format!("Path: {}", path.display()));
            }
            Object::File{ image, .. } => image.show_menu(ui),
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
            Object::Gds { image, .. } => image.transform.translation,
            Object::File{ image, .. } => image.center(),
            Object::ScanImage(live_image) => live_image.transform.translation,
            Object::ScanArea(scan_area) => scan_area.world_transform.translation,
        }
    }
    pub fn apply_transform(&mut self, tran: DAffine2) {
        match self {
            Object::Gds { image, .. } => image.transform = tran * image.transform,
            Object::File{ image, .. } => image.transform_world_points(tran),
            Object::ScanImage(live_image) => live_image.transform = tran * live_image.transform,
            Object::ScanArea(scan_area) => {
                scan_area.world_transform = tran * scan_area.world_transform
            }
        }
    }
    pub fn goto_transform(&self) -> DAffine2 {
        match self {
            Object::Gds { image, .. } => DAffine2::from_scale_angle_translation(
                DVec2::splat(image.scale),
                0.,
                image.transform.translation,
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
            Object::File{ image, .. } => {
                let center = image.center();
                let scale = image
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
