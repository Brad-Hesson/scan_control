use core::f32;

use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, BufferUsages, Device, Extent3d},
};
use egui::Color32;
use glam::{Affine2, Vec3};
use image_compute::{buffers::StorageBuffer, OutData};
use uuid::Uuid;

use crate::scan_view::{
    global::GlobalResources,
    shaders::{copy_texture, scan_image::NormalizeControl},
};

use super::shaders::{scan_image, ImageTexture, MetadataBuffer, TransformBuffer};

pub(super) struct ImageCallback {
    pub uuid: Uuid,
    pub transform: Affine2,
    pub size: [u32; 2],
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
        let image_res = global_res.images.entry(self.uuid).or_insert_with(|| {
            ImageResources::new(device, self.size, |data| {
                data.copy_from_slice(&self.changes[0].1[..]);
            })
        });

        // Set the new world transform
        image_res.world_transform_buf.set(queue, self.transform);

        // If there are changes to the image data, write the image
        // normalization data to the buffer
        if !self.changes.is_empty() {
            image_res.metadata_buffer.set(
                queue,
                &copy_texture::Metadata {
                    width: self.size[0],
                    max: 1.,
                    min: 0.,
                },
            );
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

        // if there are changes to the image, normalize the image and
        // copy it to the texture
        if !self.changes.is_empty() {
            let mut cpass = egui_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_res.image_buffer.set(&mut cpass);
            image_res.out_data.set(&mut cpass);
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
        let image_res = global_res
            .images
            .get(&self.uuid)
            .expect("ImageResources not initialized");

        // Draw the image to the screen
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
    image_buffer: image_compute::Image,
    local_bind_group: scan_image::LocalBindGroup,
    metadata_buffer: MetadataBuffer,
    image_copy_bind_group: copy_texture::BindGroup,
    image_texture: ImageTexture,
    out_data: OutData,
    normalize_control: StorageBuffer<NormalizeControl>,
}
impl ImageResources {
    pub fn new(device: &Device, size: [u32; 2], init_fn: impl FnOnce(&mut [f32])) -> Self {
        let world_transform_buf = TransformBuffer::new(device);
        let image_buffer = StorageBuffer::new(
            device,
            None,
            BufferUsages::COPY_DST | BufferUsages::STORAGE,
            (size[0] * size[1]) as usize,
            |_| {},
        );
        let image_texture = ImageTexture::new(
            device,
            Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        let out_data = OutData::new(device, size, &image_texture.0);
        let normalize_control = StorageBuffer::new(
            device,
            None,
            BufferUsages::UNIFORM | BufferUsages::COPY_SRC,
            1,
            |data| {
                data[0] = NormalizeControl {
                    max_min: 1,
                    _pad: 0,
                    std_dev_mul: 5.,
                }
            },
        );
        let local_bind_group = scan_image::LocalBindGroup::new(
            device,
            &world_transform_buf,
            &image_texture,
            &out_data.normalize_out,
            &normalize_control,
        );
        let metadata_buffer = MetadataBuffer::new(device);
        let image_copy_bind_group =
            copy_texture::BindGroup::new(device, &metadata_buffer, &image_buffer, &image_texture);
        Self {
            image_buffer: image_compute::Image::new(device, None, size, init_fn),
            local_bind_group,
            world_transform_buf,
            metadata_buffer,
            image_copy_bind_group,
            image_texture,
            out_data,
            normalize_control,
        }
    }
}
