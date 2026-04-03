use eframe::egui_wgpu::{self, CallbackTrait};
use egui::Color32;
use glam::DAffine2;
use image_compute::gds_image::GDSImageBuffers;

use crate::scan_view::callbacks::global::GlobalResources;

pub struct GDSImageCallback {
    pub transform: DAffine2,
    pub color: Color32,
    pub image_buffers: GDSImageBuffers,
}
impl CallbackTrait for GDSImageCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // Set the new world transform
        self.image_buffers
            .write_world_transform(queue, self.transform);
        self.image_buffers.write_color(queue, self.color);
        vec![]
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let global_res = callback_resources
            .get::<GlobalResources>()
            .expect("GlobalResources not initialized");

        // Draw the image to the screen
        global_res.gds_image_pipeline.draw(
            render_pass,
            &self.image_buffers,
            &global_res.scan_image_buffers,
        );
    }
}
