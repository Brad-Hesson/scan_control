use std::path::Path;

use eframe::egui_wgpu;
use egui::{DragValue, Id, Ui};
use glam::Mat3;
use image_compute::file_image::FileImageBuffers;

use crate::scan_view::{callbacks::FileImageCallback, view::ScanViewCtx, ImageEncoder};

pub struct FileImage {
    pub transform: Mat3,
    buffers: FileImageBuffers,
    pub name: String,
}
impl FileImage {
    pub fn new(image_encoder: &ImageEncoder, path: impl AsRef<Path>, transform: Mat3) -> Self {
        let img = image::open(&path).unwrap();
        let name = path.as_ref().file_name().unwrap().to_string_lossy();
        let buffers = FileImageBuffers::new(
            &image_encoder.wgpu_state.device,
            &image_encoder.wgpu_state.queue,
            &img,
        );
        Self {
            transform,
            buffers,
            name: name.to_string(),
        }
    }
    pub fn show(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            FileImageCallback {
                transform: self.transform,
                image_buffers: self.buffers.clone(),
            },
        );
        ui.painter().add(callback);
    }
    pub fn show_menu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut self.transform.x_axis.x).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.y_axis.x).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.z_axis.x).speed(0.01));
        });
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut self.transform.x_axis.y).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.y_axis.y).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.z_axis.y).speed(0.01));
        });
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut self.transform.x_axis.z).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.y_axis.z).speed(0.01));
            ui.add(DragValue::new(&mut self.transform.z_axis.z).speed(0.01));
        });
    }
}
