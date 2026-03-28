use std::{io::Cursor, sync::Arc, u32};

use egui::Color32;
use encase::ShaderSize;
use glam::{Affine2, Mat3};
use wgpu::{
    BlendState, BufferUsages, ColorTargetState, ColorWrites, Device, IndexFormat, MultisampleState,
    PrimitiveState, PrimitiveTopology, Queue, RenderPass, RenderPipelineDescriptor, TextureFormat,
    VertexAttribute, VertexBufferLayout,
};

use crate::{
    buffers::{StorageBuffer, TransformBuffer},
    scan_image::ScanImageBuffers,
    shaders::{self, border_line::VertexEntry},
};

#[derive(Clone)]
pub struct GDSImageBuffers {
    bg: Arc<shaders::border_line::bind_groups::BindGroup1>,
    transform_buffer: TransformBuffer,
    border_color_buffer: StorageBuffer<f32>,
    vertex_buffer: StorageBuffer<glam::Vec2>,
    index_buffer: StorageBuffer<u32>,
    num_indices: u32,
}
impl GDSImageBuffers {
    pub fn new(device: &Device, polys: Vec<Vec<glam::Vec2>>) -> Self {
        let mut vert_buf_len = 0;
        let mut index_buf_len = 0;
        for poly in &polys {
            vert_buf_len += poly.len();
            index_buf_len += poly.len() + 2; // one for closing vert, one for separator
        }
        dbg!(vert_buf_len);
        dbg!(index_buf_len);
        let mut vertex_buffer_uninit = StorageBuffer::<glam::Vec2>::new_init_handle(
            device,
            None,
            BufferUsages::VERTEX,
            vert_buf_len,
        );
        let mut index_buffer_uninit = StorageBuffer::<u32>::new_init_handle(
            device,
            None,
            BufferUsages::INDEX,
            index_buf_len,
        );
        {
            let mut verts_view = vertex_buffer_uninit.view_mut();
            let mut verts = SliceWriter::new(verts_view.as_mut());
            let mut inds_view = index_buffer_uninit.view_mut();
            let mut inds = SliceWriter::new(inds_view.as_mut());
            for poly in polys {
                let start_ind = verts.position() as u32;
                verts.write_many(&poly);
                for i in 0..(poly.len() as u32) {
                    inds.write(start_ind + i);
                }
                inds.write(start_ind);
                inds.write(u32::MAX);
            }
        }
        let num_indices = (index_buf_len - 1) as u32;
        let vertex_buffer = vertex_buffer_uninit.finish();
        let index_buffer = index_buffer_uninit.finish();
        let border_color_buffer = StorageBuffer::<f32>::new(
            device,
            Some("border_color_buffer"),
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            3,
            |_| {},
        );
        let transform_buffer = TransformBuffer::new(device);
        let bg = Arc::new(
            shaders::border_line::bind_groups::BindGroup1::from_bindings(
                device,
                shaders::border_line::bind_groups::BindGroupLayout1 {
                    quad2world: transform_buffer.as_entire_buffer_binding(),
                    border_color: border_color_buffer.as_entire_buffer_binding(),
                },
            ),
        );
        Self {
            bg,
            border_color_buffer,
            transform_buffer,
            index_buffer,
            vertex_buffer,
            num_indices,
        }
    }
    pub fn write_world_transform(&self, queue: &Queue, transform: Affine2) {
        self.transform_buffer.write(queue, transform);
    }
    pub fn write_color(&self, queue: &Queue, color: Color32) {
        self.border_color_buffer
            .queue_write(queue, 0, 3, |buf| {
                buf[0] = (color.r() as f32) / 255.;
                buf[1] = (color.g() as f32) / 255.;
                buf[2] = (color.b() as f32) / 255.;
            })
            .unwrap();
    }
}

pub struct GDSImagePipeline {
    pipeline: wgpu::RenderPipeline,
}
impl GDSImagePipeline {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader_module = shaders::border_line::create_shader_module(device);
        let buffer_layout = VertexBufferLayout {
            array_stride: glam::Vec2::SHADER_SIZE.get(),
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            }],
        };
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&shaders::border_line::create_pipeline_layout(device)),
            vertex: shaders::border_line::vertex_state(
                &shader_module,
                &VertexEntry {
                    entry_point: shaders::border_line::ENTRY_VS_MAIN,
                    buffers: [buffer_layout],
                    constants: vec![],
                },
            ),
            fragment: Some(shaders::border_line::fragment_state(
                &shader_module,
                &shaders::border_line::fs_main_entry([Some(ColorTargetState {
                    format: target_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })]),
            )),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::LineStrip,
                strip_index_format: Some(IndexFormat::Uint32),
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
        gds_buffers: &GDSImageBuffers,
        scan_image_buffers: &ScanImageBuffers<COLOR_MAP_SIZE>,
    ) {
        pass.set_pipeline(&self.pipeline);
        scan_image_buffers.bg.set(pass);
        gds_buffers.bg.set(pass);
        pass.set_vertex_buffer(0, gds_buffers.vertex_buffer.buffer_ref().slice(..));
        pass.set_index_buffer(
            gds_buffers.index_buffer.buffer_ref().slice(..),
            IndexFormat::Uint32,
        );
        pass.draw_indexed(0..gds_buffers.num_indices, 0, 0..1);
    }
}

struct SliceWriter<'a, T> {
    slice: &'a mut [T],
    cursor: usize,
}
impl<'a, T: Copy> SliceWriter<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Self {
        Self { slice, cursor: 0 }
    }
    pub fn write_many(&mut self, vals: &[T]) {
        let len = vals.len();
        self.slice[self.cursor..][..len].copy_from_slice(vals);
        self.cursor += len;
    }
    pub fn write(&mut self, val: T) {
        self.slice[self.cursor] = val;
        self.cursor += 1;
    }
    pub fn position(&self) -> usize {
        self.cursor
    }
}
