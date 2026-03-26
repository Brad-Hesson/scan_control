use std::sync::Arc;

use egui::Color32;
use glam::{Affine2, Mat3};
use image::DynamicImage;
use wgpu::{
    BlendState, ColorTargetState, ColorWrites, Device, Extent3d, FilterMode, MultisampleState,
    PrimitiveState, PrimitiveTopology, Queue, RenderPass, RenderPipeline, RenderPipelineDescriptor,
    SamplerDescriptor, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, util::DeviceExt as _, wgt::TextureDataOrder,
};

use crate::{
    buffers::{ColorMapTexture, TransformBuffer},
    image_compute::ImageComputeBuffers,
    scan_image::ScanImageBuffers,
    shaders,
};

#[derive(Clone)]
pub struct FileImageBuffers {
    world_transform_buffer: TransformBuffer,
    bg: Arc<shaders::file_image::bind_groups::BindGroup1>,
}

impl FileImageBuffers {
    pub fn new(device: &Device, queue: &Queue, src: &DynamicImage) -> Self {
        let img = src.to_rgba8();
        let size = [img.width() as u32, img.height() as u32];
        let world_transform_buffer = TransformBuffer::new(device);
        let image_texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: None,
                size: Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(img.as_raw()),
        );
        let bg = Arc::new(shaders::file_image::bind_groups::BindGroup1::from_bindings(
            device,
            shaders::file_image::bind_groups::BindGroupLayout1 {
                image_tex: &image_texture.create_view(&TextureViewDescriptor::default()),
                quad2world: world_transform_buffer.as_entire_buffer_binding(),
            },
        ));
        Self {
            world_transform_buffer,
            bg,
        }
    }
    pub fn write_world_transform(&self, queue: &Queue, transform: Mat3) {
        self.world_transform_buffer.write_mat3(queue, transform);
    }
}

pub struct FileImagePipeline {
    pipeline: RenderPipeline,
}
impl FileImagePipeline {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader_module = shaders::file_image::create_shader_module(device);
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&shaders::file_image::create_pipeline_layout(device)),
            vertex: shaders::file_image::vertex_state(
                &shader_module,
                &shaders::file_image::vs_main_entry(),
            ),
            fragment: Some(shaders::file_image::fragment_state(
                &shader_module,
                &shaders::file_image::fs_main_entry([Some(ColorTargetState {
                    format: target_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })]),
            )),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: true,
            },
            multiview: None,
            cache: None,
        });
        Self { pipeline }
    }
    pub fn draw<const COLOR_MAP_SIZE: usize>(
        &self,
        pass: &mut RenderPass,
        image_buffers: &FileImageBuffers,
        scan_image_buffers: &ScanImageBuffers<COLOR_MAP_SIZE>,
    ) {
        pass.set_pipeline(&self.pipeline);
        scan_image_buffers.bg.set(pass);
        image_buffers.bg.set(pass);
        pass.draw(0..4, 0..1);
    }
}
