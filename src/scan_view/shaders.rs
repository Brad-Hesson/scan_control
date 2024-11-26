use copy_texture::Metadata;
use eframe::wgpu::{
    BlendState, Buffer, BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, Device,
    Extent3d, FilterMode, MultisampleState, PrimitiveState, PrimitiveTopology, Queue,
    RenderPipeline, RenderPipelineDescriptor, SamplerDescriptor, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
    COPY_BYTES_PER_ROW_ALIGNMENT,
};
use glam::{Affine2, Mat3, Mat4, Vec4};

mod bindings {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

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
            image_buffer: &ImageBuffer,
            image_texture: &ImageTexture,
        ) -> Self {
            let bindings = bindings::copy_texture::bind_groups::BindGroupLayout0 {
                met: metadata.0.as_entire_buffer_binding(),
                data: image_buffer.buffer.as_entire_buffer_binding(),
                texture: &image_texture
                    .0
                    .create_view(&TextureViewDescriptor::default()),
            };
            Self::from_bindings(device, bindings)
        }
    }
}

pub mod image_view {
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
        pub fn new(device: &Device, screen_transform_buf: &TransformBuffer) -> Self {
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
                    texture: &image_texture
                        .0
                        .create_view(&TextureViewDescriptor::default()),
                },
            )
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
        queue.write_buffer(
            &self.0,
            0,
            bytemuck::bytes_of(affine2_to_mat4(transform).as_ref()),
        );
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
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        Self(texture)
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
pub struct ImageBuffer {
    buffer: Buffer,
    width: u32,
    aligned_width: u32,
}
impl ImageBuffer {
    pub fn new(device: &Device, size: Extent3d) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: calc_aligned_width(size.width, ROW_ALIGN) as u64
                * size.height as u64
                * std::mem::size_of::<f32>() as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::COPY_SRC | BufferUsages::STORAGE,
            mapped_at_creation: true,
        });
        bytemuck::cast_slice_mut(buffer.slice(..).get_mapped_range_mut().as_mut()).fill(f32::NAN);
        buffer.unmap();
        Self {
            buffer,
            width: size.width,
            aligned_width: calc_aligned_width(size.width, ROW_ALIGN),
        }
    }
    pub fn set(&self, queue: &Queue, offset: usize, data: &[f32]) {
        if self.width == self.aligned_width {
            queue.write_buffer(
                &self.buffer,
                offset as u64 * size_of::<f32>() as u64,
                bytemuck::cast_slice(data),
            );
        } else {
            aligned_write(
                data,
                offset,
                self.width as usize,
                self.aligned_width as usize,
                |buf, off| {
                    queue.write_buffer(
                        &self.buffer,
                        off as u64 * size_of::<f32>() as u64,
                        bytemuck::cast_slice(buf),
                    );
                },
            );
        }
    }
}

const ROW_ALIGN: u32 = COPY_BYTES_PER_ROW_ALIGNMENT / std::mem::size_of::<f32>() as u32;

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

fn affine2_to_mat4(af: Affine2) -> Mat4 {
    let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(af.matrix2));
    let trans = af.translation;
    mat4.w_axis = Vec4::new(trans.x, trans.y, 0., 1.);
    mat4
}
