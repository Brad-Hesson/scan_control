use core::f32;

use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, Device, Extent3d},
};
use glam::Affine2;
use uuid::Uuid;

use crate::scan_view::{global::GlobalResources, shaders::copy_texture};

use super::shaders::{scan_image, ImageBuffer, ImageTexture, MetadataBuffer, TransformBuffer};

pub(super) struct ImageCallback {
    pub uuid: Uuid,
    pub transform: Affine2,
    pub size: Extent3d,
    pub changes: Vec<(usize, Box<[f32]>)>,
}
impl CallbackTrait for ImageCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let global_res = callback_resources
            .get_mut::<GlobalResources>()
            .expect("GlobalResources not initialized");
        let image_res = global_res
            .images
            .entry(self.uuid)
            .or_insert_with(|| ImageResources::new(device, self.size));

        image_res.world_transform_buf.set(queue, self.transform);
        if !self.changes.is_empty() {
            image_res.metadata_buffer.set(
                queue,
                &copy_texture::Metadata {
                    width: self.size.width,
                    max: 1.,
                    min: 0.1,
                },
            );
        }
        for (offset, data) in &self.changes {
            image_res.image_buffer.set(queue, *offset, &data);
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
        let image_res = global_res
            .images
            .get(&self.uuid)
            .expect("ImageResources not initialized");

        if !self.changes.is_empty() {
            let mut cpass = egui_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            cpass.set_pipeline(&global_res.image_copy_pipeline);
            copy_texture::set_bind_groups(&mut cpass, &image_res.image_copy_bind_group);
            cpass.dispatch_workgroups(self.size.width, self.size.height, 1);
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
        let image_res = global_res
            .images
            .get(&self.uuid)
            .expect("ImageResources not initialized");

        render_pass.set_pipeline(&global_res.scan_image_pipeline);
        scan_image::set_bind_groups(
            render_pass,
            &global_res.global_bind_group,
            &image_res.local_bind_group,
        );
        render_pass.draw(0..4, 0..1);
    }
}

pub(super) struct ImageResources {
    world_transform_buf: TransformBuffer,
    image_buffer: ImageBuffer,
    local_bind_group: scan_image::LocalBindGroup,
    metadata_buffer: MetadataBuffer,
    image_copy_bind_group: copy_texture::BindGroup,
}
impl ImageResources {
    pub fn new(device: &Device, size: Extent3d) -> Self {
        let world_transform_buf = TransformBuffer::new(device);
        let image_buffer = ImageBuffer::new(device, size);
        let image_texture = ImageTexture::new(device, size);
        let local_bind_group =
            scan_image::LocalBindGroup::new(device, &world_transform_buf, &image_texture);
        let metadata_buffer = MetadataBuffer::new(device);
        let image_copy_bind_group =
            copy_texture::BindGroup::new(device, &metadata_buffer, &image_buffer, &image_texture);
        Self {
            image_buffer,
            local_bind_group,
            world_transform_buf,
            metadata_buffer,
            image_copy_bind_group,
        }
    }
}
