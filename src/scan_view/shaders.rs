use std::marker::PhantomData;

use bytemuck::AnyBitPattern;
use copy_texture::Metadata;
use eframe::wgpu::{
    BlendState, Buffer, BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, Device,
    Extent3d, FilterMode, MultisampleState, PrimitiveState, PrimitiveTopology, Queue,
    RenderPipeline, RenderPipelineDescriptor, SamplerDescriptor, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use glam::{Affine2, Mat3, Mat4};

mod bindings {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(dead_code)]

    include!(concat!(env!("OUT_DIR"), "/shader_bindings.rs"));
}

pub mod copy_texture {
    use super::*;

    pub use bindings::copy_texture::bind_groups::BindGroup0 as BindGroup;
    pub use bindings::copy_texture::compute::create_main_pipeline;
    pub use bindings::copy_texture::set_bind_groups;
    pub use bindings::copy_texture::Metadata;

    impl BindGroup {
        pub fn new(
            device: &Device,
            metadata: &MetadataBuffer,
            image_buffer: &StorageBuffer<f32>,
            image_texture: &ImageTexture,
        ) -> Self {
            let bindings = bindings::copy_texture::bind_groups::BindGroupLayout0 {
                met: metadata.0.as_entire_buffer_binding(),
                data: image_buffer.0.as_entire_buffer_binding(),
                texture: &image_texture
                    .0
                    .create_view(&TextureViewDescriptor::default()),
            };
            Self::from_bindings(device, bindings)
        }
    }
}

pub mod scan_image {
    use super::*;

    pub use bindings::scan_image::bind_groups::BindGroup0 as GlobalBindGroup;
    pub use bindings::scan_image::bind_groups::BindGroup1 as LocalBindGroup;
    pub use bindings::scan_image::set_bind_groups;

    pub fn create_main_pipeline(device: &Device, target_format: TextureFormat) -> RenderPipeline {
        let shader_module = bindings::scan_image::create_shader_module(device);
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&bindings::scan_image::create_pipeline_layout(device)),
            vertex: bindings::scan_image::vertex_state(
                &shader_module,
                &bindings::scan_image::vs_main_entry(),
            ),
            fragment: Some(bindings::scan_image::fragment_state(
                &shader_module,
                &bindings::scan_image::fs_main_entry([Some(ColorTargetState {
                    format: target_format,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })]),
            )),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    }
    impl GlobalBindGroup {
        pub fn new(
            device: &Device,
            screen_transform_buf: &TransformBuffer,
            color_map_texture: &ColorMapTexture,
        ) -> Self {
            let sampler = device.create_sampler(&SamplerDescriptor {
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..Default::default()
            });
            bindings::scan_image::bind_groups::BindGroup0::from_bindings(
                device,
                bindings::scan_image::bind_groups::BindGroupLayout0 {
                    world2screen: screen_transform_buf.0.as_entire_buffer_binding(),
                    tex_sampler: &sampler,
                    color_map: &color_map_texture
                        .0
                        .create_view(&TextureViewDescriptor::default()),
                },
            )
        }
    }
    impl LocalBindGroup {
        pub fn new(
            device: &Device,
            world_transform_buf: &TransformBuffer,
            image_texture: &ImageTexture,
        ) -> Self {
            bindings::scan_image::bind_groups::BindGroup1::from_bindings(
                device,
                bindings::scan_image::bind_groups::BindGroupLayout1 {
                    quad2world: world_transform_buf.0.as_entire_buffer_binding(),
                    height_map: &image_texture
                        .0
                        .create_view(&TextureViewDescriptor::default()),
                },
            )
        }
    }
}

pub mod transform {
    use super::*;

    pub use bindings::transform::bind_groups::BindGroup0 as BindGroup;
    pub use bindings::transform::compute::create_sum_pipeline;
    pub use bindings::transform::compute::create_col_sum_pipeline;
    pub use bindings::transform::compute::create_row_sum_pipeline;
    pub use bindings::transform::set_bind_groups;

    impl BindGroup {
        pub fn new(
            device: &Device,
            size: &StorageBuffer<u32>,
            data_buffer: &StorageBuffer<f32>,
        ) -> Self {
            let bindings = bindings::transform::bind_groups::BindGroupLayout0 {
                size: size.0.as_entire_buffer_binding(),
                data: data_buffer.0.as_entire_buffer_binding(),
            };
            Self::from_bindings(device, bindings)
        }
    }
}

pub struct TransformBuffer(Buffer);
impl TransformBuffer {
    pub fn new(device: &Device) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("quad2world uniform"),
            size: std::mem::size_of::<glam::Mat4>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self(buffer)
    }
    pub fn set(&self, queue: &Queue, transform: Affine2) {
        let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(transform.matrix2));
        mat4.w_axis.x = transform.translation.x;
        mat4.w_axis.y = transform.translation.y;
        queue.write_buffer(&self.0, 0, bytemuck::bytes_of(mat4.as_ref()));
    }
}
pub struct ImageTexture(Texture);
impl ImageTexture {
    pub fn new(device: &Device, size: Extent3d) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        Self(texture)
    }
}

pub struct ColorMapTexture(Texture);
impl ColorMapTexture {
    pub const SIZE: usize = 1024;
    pub fn new(device: &Device) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: Self::SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D1,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[TextureFormat::Rgba8UnormSrgb],
        });
        Self(texture)
    }
    pub fn set(&self, queue: &Queue, color_map: &[egui::Color32; Self::SIZE]) {
        queue.write_texture(
            self.0.as_image_copy(),
            bytemuck::cast_slice(color_map),
            eframe::wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(Self::SIZE as u32 * std::mem::size_of::<u8>() as u32 * 4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: Self::SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}
pub struct MetadataBuffer(Buffer);
impl MetadataBuffer {
    pub fn new(device: &Device) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Metadata>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self(buffer)
    }
    pub fn set(&self, queue: &Queue, data: &Metadata) {
        queue.write_buffer(&self.0, 0, bytemuck::bytes_of(data));
    }
}
pub struct StorageBuffer<T: Clone + bytemuck::NoUninit + AnyBitPattern>(Buffer, PhantomData<T>);
impl<T: Clone + bytemuck::NoUninit + AnyBitPattern> StorageBuffer<T> {
    pub fn new_with(device: &Device, size: usize, fill: T, usage: BufferUsages) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: (size * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: true,
        });
        bytemuck::cast_slice_mut(buffer.slice(..).get_mapped_range_mut().as_mut()).fill(fill);
        buffer.unmap();
        Self(buffer, PhantomData)
    }
    pub fn new_as(device: &Device, data: &[T], usage: BufferUsages) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: (data.len() * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: true,
        });
        bytemuck::cast_slice_mut(buffer.slice(..).get_mapped_range_mut().as_mut())
            .copy_from_slice(data);
        buffer.unmap();
        Self(buffer, PhantomData)
    }
    pub fn new(device: &Device, size: usize, usage: BufferUsages) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: (size * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: false,
        });
        Self(buffer, PhantomData)
    }
    pub fn set(&self, queue: &Queue, offset: usize, data: &[T]) {
        queue.write_buffer(
            &self.0,
            offset as u64 * size_of::<T>() as u64,
            bytemuck::cast_slice(data),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eframe::wgpu::{
        self, CommandEncoderDescriptor, DeviceDescriptor, RequestAdapterOptionsBase,
    };

    use super::*;

    #[test]
    fn sum_shader() {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&RequestAdapterOptionsBase::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&DeviceDescriptor::default(), None)).unwrap();
        let pipeline = transform::create_col_sum_pipeline(&device);
        let width = 5;
        let height = 7;
        let buf_size = width * height;
        let data_buf = StorageBuffer::new_with(
            &device,
            buf_size,
            1.,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        );
        let staging_buf = Arc::new(StorageBuffer::<f32>::new(
            &device,
            buf_size,
            BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        ));
        let size_buf = StorageBuffer::new_as(&device, &[width as u32, height as u32], BufferUsages::UNIFORM);
        let bind_group = transform::BindGroup::new(&device, &size_buf, &data_buf);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);
            transform::set_bind_groups(&mut pass, &bind_group);
            pass.dispatch_workgroups(width as u32, height as u32, 1);
        }
        encoder.copy_buffer_to_buffer(
            &data_buf.0,
            0,
            &staging_buf.0,
            0,
            (buf_size * std::mem::size_of::<f32>()) as u64,
        );
        queue.submit([encoder.finish()]);
        while !device.poll(wgpu::MaintainBase::Wait).is_queue_empty() {}
        let data_buf_clone = staging_buf.clone();
        staging_buf
            .0
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |_a| {
                let raw = data_buf_clone.0.slice(..).get_mapped_range();
                let data = bytemuck::cast_slice::<_, f32>(&raw);
                for i in 0..height{
                    println!("{:?}", &data[i*width..][..width])
                }
            });
        while !device.poll(wgpu::MaintainBase::Wait).is_queue_empty() {}
    }
}
