use egui::Color32;
use glam::Affine2;
use wgpu::{
    BlendState, ColorTargetState, ColorWrites, Device, FilterMode, MultisampleState,
    PrimitiveState, PrimitiveTopology, Queue, RenderPass, RenderPipeline, RenderPipelineDescriptor,
    SamplerDescriptor, TextureFormat,
};

use crate::{
    buffers::{ColorMapTexture, TransformBuffer},
    image_compute::ImageComputeBuffers,
    shaders,
};

pub struct ScanImageBuffers<const COLOR_MAP_SIZE: usize> {
    screen_transform_buffer: TransformBuffer,
    color_map_texture: ColorMapTexture<COLOR_MAP_SIZE>,
    pub bg: shaders::scan_image::bind_groups::BindGroup0,
}

impl<const COLOR_MAP_SIZE: usize> ScanImageBuffers<COLOR_MAP_SIZE> {
    pub fn new(device: &Device) -> Self {
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let screen_transform_buffer = TransformBuffer::new(device);
        let color_map_texture = ColorMapTexture::new(device);
        let bg = shaders::scan_image::bind_groups::BindGroup0::from_bindings(
            device,
            shaders::scan_image::bind_groups::BindGroupLayout0 {
                world2screen: screen_transform_buffer.as_entire_buffer_binding(),
                tex_sampler: &sampler,
                color_map: &color_map_texture.create_view(),
            },
        );
        Self {
            screen_transform_buffer,
            color_map_texture,
            bg,
        }
    }
    pub fn write_screen_transform(&self, queue: &Queue, transform: Affine2) {
        self.screen_transform_buffer.write(queue, transform);
    }
    pub fn write_color_map(&self, queue: &Queue, color_map: &[Color32; COLOR_MAP_SIZE]) {
        self.color_map_texture.write(queue, color_map);
    }
}

pub struct ScanImagePipeline {
    pipeline: RenderPipeline,
}
impl ScanImagePipeline {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader_module = shaders::scan_image::create_shader_module(device);
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&shaders::scan_image::create_pipeline_layout(device)),
            vertex: shaders::scan_image::vertex_state(
                &shader_module,
                &shaders::scan_image::vs_main_entry(),
            ),
            fragment: Some(shaders::scan_image::fragment_state(
                &shader_module,
                &shaders::scan_image::fs_main_entry([Some(ColorTargetState {
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
        image_buffers: &ImageComputeBuffers,
        scan_image_buffers: &ScanImageBuffers<COLOR_MAP_SIZE>,
    ) {
        pass.set_pipeline(&self.pipeline);
        scan_image_buffers.bg.set(pass);
        image_buffers.scan_image_bg.set(pass);
        pass.draw(0..4, 0..1);
    }
}
