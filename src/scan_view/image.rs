use core::f32;

use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, Device, Extent3d, Queue},
};
use glam::Affine2;
use uuid::Uuid;

use crate::scan_view::{global::GlobalResources, shaders::copy_texture};

use super::shaders::{image_view, ImageBuffer, ImageTexture, MetadataBuffer, TransformBuffer};

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
        image_res.set_transform(queue, self.transform);
        image_res.metadata_buffer.set(
            queue,
            &copy_texture::Metadata {
                width: calc_aligned_width(self.size.width, ROW_ALIGN),
                max: 1.,
                min: 0.1,
            },
        );
        for (offset, data) in &self.changes {
            image_res.set_texture_data(queue, *offset, &data);
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
            cpass.set_pipeline(&global_res.copy_texture_pipeline);
            copy_texture::set_bind_groups(&mut cpass, &image_res.copy_texture_bind_group);
            cpass.dispatch_workgroups(self.size.width as u32, self.size.height, 1);
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
        render_pass.set_pipeline(&global_res.pipeline);
        image_view::set_bind_groups(
            render_pass,
            &global_res.global_bg,
            &image_res.local_bind_group,
        );
        render_pass.draw(0..4, 0..1);
    }
}

pub(super) struct ImageResources {
    world_transform_buf: TransformBuffer,
    image_buffer: ImageBuffer,
    local_bind_group: image_view::LocalBindGroup,
    metadata_buffer: MetadataBuffer,
    copy_texture_bind_group: copy_texture::BindGroup,
}
impl ImageResources {
    pub fn new(device: &Device, size: Extent3d) -> Self {
        let world_transform_buf = TransformBuffer::new(device);
        let image_buffer = ImageBuffer::new(device, size);
        let image_texture = ImageTexture::new(device, size);
        let local_bind_group =
            image_view::LocalBindGroup::new(device, &world_transform_buf, &image_texture);
        let metadata_buffer = MetadataBuffer::new(device);
        let copy_texture_bind_group =
            copy_texture::BindGroup::new(device, &metadata_buffer, &image_buffer, &image_texture);
        Self {
            image_buffer,
            local_bind_group,
            world_transform_buf,
            metadata_buffer,
            copy_texture_bind_group,
        }
    }
    fn set_texture_data(&self, queue: &Queue, offset: usize, data: &[f32]) {
        self.image_buffer.set(queue, offset, data);
    }
    fn set_transform(&self, queue: &Queue, transform: Affine2) {
        self.world_transform_buf.set(queue, transform);
    }
}

const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT / std::mem::size_of::<f32>() as u32;

fn calc_aligned_width(width: u32, alignment: u32) -> u32 {
    ((width + alignment - 1) / alignment) * alignment
}
