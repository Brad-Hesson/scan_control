use core::f32;

use bytemuck::{bytes_of, cast_slice, cast_slice_mut};
use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{
        self, BindGroup, BindGroupLayout, Buffer, BufferDescriptor, BufferUsages, Device, Extent3d,
        ImageCopyBuffer, ImageDataLayout, Queue, Texture, TextureDescriptor, TextureUsages,
    },
};
use glam::{Affine2, Mat4};
use uuid::Uuid;

use crate::scan_view::global::GlobalResources;

use super::{affine2_to_mat4, copy_texture::CopyTextureBindGroup};

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
        let image_res = global_res.images.entry(self.uuid).or_insert_with(|| {
            ImageResources::new(
                device,
                &global_res.image_bgl,
                &global_res.copy_texture.bind_group_layout,
                self.size,
            )
        });
        image_res.set_transform(queue, self.transform);
        image_res.copy_texture.set_metadata(
            queue,
            0.,
            1.,
            calc_aligned_width(self.size.width, ROW_ALIGN),
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
            cpass.set_pipeline(&global_res.copy_texture.pipeline);
            cpass.set_bind_group(0, &image_res.copy_texture.bind_group, &[]);
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
        render_pass.set_bind_group(0, &global_res.global_bg, &[]);
        render_pass.set_bind_group(1, &image_res.local_bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}

pub(super) struct ImageResources {
    quad2world_buf: Buffer,
    texture: Texture,
    texture_staging_buffer: Buffer,
    local_bind_group: BindGroup,
    width: usize,
    aligned_width: usize,
    copy_texture: CopyTextureBindGroup,
}
impl ImageResources {
    pub fn new(
        device: &Device,
        image_bgl: &BindGroupLayout,
        meta_bgl: &BindGroupLayout,
        size: Extent3d,
    ) -> Self {
        let quad2world_buf = device.create_buffer(&BufferDescriptor {
            label: Some("quad2world uniform"),
            size: std::mem::size_of::<Mat4>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture_staging_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: calc_aligned_width(size.width, ROW_ALIGN) as u64
                * size.height as u64
                * std::mem::size_of::<f32>() as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::COPY_SRC | BufferUsages::STORAGE,
            mapped_at_creation: true,
        });
        cast_slice_mut(
            texture_staging_buffer
                .slice(..)
                .get_mapped_range_mut()
                .as_mut(),
        )
        .fill(f32::NAN);
        texture_staging_buffer.unmap();
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::STORAGE_BINDING,
            view_formats: &[wgpu::TextureFormat::R32Float],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let local_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: quad2world_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
            ],
            label: None,
        });
        Self {
            copy_texture: CopyTextureBindGroup::new(
                device,
                meta_bgl,
                &texture,
                &texture_staging_buffer,
            ),
            texture_staging_buffer,
            texture,
            local_bind_group,
            quad2world_buf,
            width: size.width as usize,
            aligned_width: calc_aligned_width(size.width, ROW_ALIGN) as usize,
        }
    }
    fn set_texture_data(&self, queue: &Queue, offset: usize, data: &[f32]) {
        if self.width == self.aligned_width {
            queue.write_buffer(
                &self.texture_staging_buffer,
                offset as u64 * size_of::<f32>() as u64,
                cast_slice(data),
            );
        } else {
            aligned_write(data, offset, self.width, self.aligned_width, |buf, off| {
                queue.write_buffer(
                    &self.texture_staging_buffer,
                    off as u64 * size_of::<f32>() as u64,
                    cast_slice(buf),
                );
            });
        }
    }
    fn set_transform(&self, queue: &Queue, transform: Affine2) {
        queue.write_buffer(
            &self.quad2world_buf,
            0,
            bytes_of(affine2_to_mat4(transform).as_ref()),
        );
    }
}

const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT / std::mem::size_of::<f32>() as u32;

fn calc_aligned_width(width: u32, alignment: u32) -> u32 {
    ((width + alignment - 1) / alignment) * alignment
}

fn aligned_write(
    mut data: &[f32],
    offset: usize,
    width: usize,
    aligned_width: usize,
    mut write: impl FnMut(&[f32], usize),
) {
    let mut aligned_offset = offset / width * aligned_width + offset % width;
    let (buf, rest) = data
        .split_at_checked(width - offset % width)
        .unwrap_or((data, &[]));
    data = rest;
    write(buf, aligned_offset);
    aligned_offset += aligned_width - offset % width;
    while !data.is_empty() {
        let (buf, rest) = data.split_at_checked(width).unwrap_or((data, &[]));
        data = rest;
        write(buf, aligned_offset);
        aligned_offset += aligned_width;
    }
}
