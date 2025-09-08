use core::f32;
use std::sync::Arc;

use eframe::egui_wgpu::{self, CallbackTrait};
use glam::Affine2;

use crate::scan_view::global::GlobalResources;

pub(super) struct ImageCallback {
    pub transform: Affine2,
    pub changes: Vec<(usize, Box<[f32]>)>,
    pub image_buffers: Arc<image_compute::ImageComputeBuffers>,
}
impl CallbackTrait for ImageCallback {
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
        global_res.scan_image_pipeline.draw(
            render_pass,
            &self.image_buffers,
            &global_res.scan_image_buffers,
        );
    }
}
