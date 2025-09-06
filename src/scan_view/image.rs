use core::f32;
use std::sync::Arc;

use eframe::egui_wgpu::{self, CallbackTrait};
use glam::Affine2;

use crate::scan_view::global::GlobalResources;

pub(super) struct ImageCallback {
    pub transform: Affine2,
    pub changes: Vec<(usize, Box<[f32]>)>,
    pub image_data: Arc<image_compute::Image>,
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
        self.image_data
            .world_transform_buffer
            .set(queue, self.transform);

        // If there are changes to the image data, write the image
        // normalization data to the buffer
        if !self.changes.is_empty() {
            // image_res.metadata_buffer.set(
            //     queue,
            //     &copy_texture::Metadata {
            //         width: self.size[0],
            //         max: 1.,
            //         min: 0.,
            //     },
            // );
        }
        vec![]
    }
    fn finish_prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let global_res = callback_resources
            .get::<GlobalResources>()
            .expect("GlobalResources not initialized");

        // if there are changes to the image, normalize the image and
        // copy it to the texture
        if !self.changes.is_empty() {
            let mut cpass = egui_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            self.image_data.set(&mut cpass);
            global_res
                .plane_fitter
                .run_mean_subtract(&mut cpass, &global_res.scratch_buffers);
        }
        Vec::new()
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
        render_pass.set_pipeline(&global_res.scan_image_pipeline);
        image_compute::shaders::scan_image::set_bind_groups(
            render_pass,
            &global_res.global_bind_group,
            &self.image_data.scan_image_bg,
        );
        render_pass.draw(0..4, 0..1);
    }
}
