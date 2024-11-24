use std::borrow::Cow;

use eframe::wgpu::{
    self, BindGroup, BindGroupLayout, Buffer, BufferDescriptor, ComputePipeline, Device, Queue,
    ShaderSource, Texture,
};

#[repr(C)]
#[derive(bytemuck::Pod, Clone, Copy, bytemuck::Zeroable)]
pub struct Metadata {
    width: u32,
    max: f32,
    min: f32,
}

pub struct CopyTextureResources {
    pub pipeline: ComputePipeline,
    pub bind_group_layout: BindGroupLayout,
}
impl CopyTextureResources {
    pub fn new(device: &Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("./copy_texture.wgsl"))),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let bind_group_layout = pipeline.get_bind_group_layout(0);

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}

pub struct CopyTextureBindGroup {
    pub metadata_buffer: Buffer,
    pub bind_group: BindGroup,
}
impl CopyTextureBindGroup {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        texture: &Texture,
        buffer: &Buffer,
    ) -> Self {
        let metadata_buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Metadata>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Copy Texture Bind Group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: metadata_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
            ],
        });
        Self {
            metadata_buffer,
            bind_group,
        }
    }
    pub fn set_metadata(&self, queue: &Queue, min: f32, max: f32, width: u32) {
        let data = Metadata { width, max, min };
        queue.write_buffer(&self.metadata_buffer, 0, bytemuck::bytes_of(&data));
    }
}
