use core::f32;
use std::borrow::Cow;

use bytemuck::{bytes_of, cast_slice, cast_slice_mut};
use eframe::{
    egui_wgpu::{self, Callback, CallbackTrait, RenderState},
    wgpu::{
        self, util::DeviceExt, BindGroup, BindGroupEntry, BindGroupLayout, Buffer,
        BufferDescriptor, BufferUsages, ColorWrites, Device, Extent3d, FilterMode, ImageCopyBuffer,
        ImageDataLayout, Queue, RenderPipeline, Texture, TextureDescriptor, TextureFormat,
        TextureUsages,
    },
};
use egui::{
    ahash::{HashMap, HashMapExt},
    InnerResponse,
};
use glam::{Affine2, Mat3, Mat4, Vec2, Vec4};
use uuid::Uuid;

fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}

fn affine2_to_mat4(af: Affine2) -> Mat4 {
    let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(af.matrix2));
    let trans = af.translation;
    // mat4.z_axis.as_mut()[2] = 1.;
    mat4.w_axis = Vec4::new(trans.x, trans.y, 0., 1.);
    mat4
}

#[derive(Clone)]
pub struct ScanView {
    world_transform: Affine2,
    rotate_center: Option<Vec2>,
    target_format: TextureFormat,
}
impl ScanView {
    pub fn show<R>(
        &mut self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut ScanViewCtx) -> R,
    ) -> InnerResponse<R> {
        egui::Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, response) = ui.allocate_at_least(
                ui.available_size_before_wrap(),
                egui::Sense {
                    click: true,
                    drag: true,
                    focusable: true,
                },
            );
            let screen_transform = self.handle_inputs(ui, response);
            ui.painter().add(Callback::new_paint_callback(
                rect,
                ScanViewCallback {
                    target_format: self.target_format,
                    screen_transform,
                },
            ));
            let mut ctx = ScanViewCtx { ui, rect };
            add_contents(&mut ctx)
        })
    }
    fn handle_inputs(&mut self, ui: &mut egui::Ui, response: egui::Response) -> Affine2 {
        let rect = response.rect;

        // Calculate the dragging transform
        let drag = if response.dragged_by(egui::PointerButton::Primary) {
            Affine2::from_translation(v2(response.drag_delta()))
        } else {
            Affine2::IDENTITY
        };
        // Calculate the rotation transform
        let rotate = if response.dragged_by(egui::PointerButton::Secondary) {
            let pos = v2(response.interact_pointer_pos().unwrap() - rect.center());
            if self.rotate_center.is_none() {
                self.rotate_center = Some(pos);
            }
            let center = self.rotate_center.unwrap();
            let drag = v2(response.drag_delta());
            let rad = pos - center;
            let angle = rad.perp_dot(drag) / rad.length_squared();
            if rad.length_squared() > 10. {
                let rot = Affine2::from_angle(angle);
                let trans = Affine2::from_translation(center);
                trans * rot * trans.inverse()
            } else {
                Affine2::IDENTITY
            }
        } else {
            self.rotate_center = None;
            Affine2::IDENTITY
        };

        // Calculate the Zooming transform
        let zoom = if let Some(window_pos) = response.hover_pos() {
            let scalar = (ui.input(|is| is.raw_scroll_delta).y / 100.).exp();
            let scale = Affine2::from_scale(Vec2::splat(scalar));
            let trans = Affine2::from_translation(v2(window_pos - rect.center()));
            trans * scale * trans.inverse()
        } else {
            Affine2::IDENTITY
        };

        // update the world transform using the calculated transforms
        self.world_transform = rotate * zoom * drag * self.world_transform;

        // calculate the screen transform as return
        let screen_transform =
            Affine2::from_scale(v2(rect.size()) * Vec2::new(0.5, -0.5)).inverse();
        screen_transform * self.world_transform

        // write the new transform to the GPU
        // self.queue.write_buffer(
        //     &self.world2screen_buf,
        //     0,
        //     bytes_of(affine2_to_mat4(screen_transform * self.world_transform).as_ref()),
        // );
    }
    pub fn new(wgpu: &RenderState) -> Self {
        Self {
            world_transform: Affine2::IDENTITY,
            rotate_center: None,
            target_format: wgpu.target_format,
        }
    }
}

pub struct ScanViewCtx<'a> {
    pub ui: &'a mut egui::Ui,
    pub rect: egui::Rect,
}

#[derive(Clone)]
pub struct ScanImage {
    uuid: Uuid,
    pub transform: Affine2,
    size: [usize; 2],
    changes: Vec<(usize, Box<[f32]>)>,
}
impl ScanImage {
    pub fn new(width: usize, data: Box<[f32]>, transform: Affine2) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            transform,
            size: [width, data.len() / width],
            changes: vec![(0, data)],
        }
    }
    pub fn show(&mut self, ctx: &mut ScanViewCtx) {
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            ImageCallback {
                uuid: self.uuid,
                transform: self.transform,
                size: Extent3d {
                    width: self.size[0] as u32,
                    height: self.size[1] as u32,
                    depth_or_array_layers: 1,
                },
                changes: std::mem::take(&mut self.changes),
            },
        );
        ctx.ui.painter().add(callback);
    }
    pub fn set_image_data(&mut self, offset: usize, data: Box<[f32]>) {
        self.changes.push((offset, data));
    }
}

struct ScanViewCallback {
    target_format: TextureFormat,
    screen_transform: Affine2,
}
impl CallbackTrait for ScanViewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let global_res = callback_resources
            .entry::<GlobalResources>()
            .or_insert_with(|| GlobalResources::new(device, self.target_format));
        global_res.set_screen_transform(queue, self.screen_transform);
        Vec::new()
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
    }
}

struct ImageCallback {
    uuid: Uuid,
    transform: Affine2,
    size: Extent3d,
    changes: Vec<(usize, Box<[f32]>)>,
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
            .or_insert_with(|| ImageResources::new(device, &global_res.image_bgl, self.size));
        image_res.set_transform(queue, self.transform);
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
            egui_encoder.copy_buffer_to_texture(
                ImageCopyBuffer {
                    buffer: &image_res.texture_staging_buffer,
                    layout: ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(
                            calc_aligned_width(self.size.width, ROW_ALIGN)
                                * std::mem::size_of::<f32>() as u32,
                        ),
                        rows_per_image: Some(self.size.height),
                    },
                },
                image_res.texture.as_image_copy(),
                self.size,
            );
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

struct GlobalResources {
    pipeline: RenderPipeline,
    global_bg: BindGroup,
    image_bgl: BindGroupLayout,
    world2screen_buf: Buffer,
    images: HashMap<Uuid, ImageResources>,
}
impl GlobalResources {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "./shaders/scan_image.wgsl"
            ))),
        });

        let global_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&global_bgl, &image_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let world_transform = Affine2::from_scale(Vec2::splat(3.));
        let world2screen_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world2screen uniform"),
            contents: bytemuck::bytes_of(affine2_to_mat4(world_transform).as_ref()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let global_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &global_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: world2screen_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: None,
        });
        Self {
            pipeline,
            global_bg,
            image_bgl,
            world2screen_buf,
            images: HashMap::new(),
        }
    }
    fn set_screen_transform(&self, queue: &Queue, transform: Affine2) {
        queue.write_buffer(
            &self.world2screen_buf,
            0,
            bytes_of(affine2_to_mat4(transform).as_ref()),
        );
    }
}

struct ImageResources {
    quad2world_buf: Buffer,
    texture: Texture,
    texture_staging_buffer: Buffer,
    local_bind_group: BindGroup,
    width: usize,
    aligned_width: usize,
}
impl ImageResources {
    pub fn new(device: &Device, image_bgl: &BindGroupLayout, size: Extent3d) -> Self {
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
            usage: BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
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
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
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
                offset as u64,
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

#[test]
fn writing() {
    let width = 8;
    let height = 4;
    let align = 5;
    let width_align = calc_aligned_width(width as u32, align) as usize;
    let input = vec![1f32; 32];
    let mut out = vec![0f32; width_align * height];
    aligned_write(&input, 0, width, width_align, |b, o| {
        out[o..][..b.len()].copy_from_slice(b)
    });
    for i in 0..height {
        println!("{:?}", &out[i * width_align..][..width_align])
    }
}

#[test]
fn width() {
    dbg!(calc_aligned_width(100, 64));
}
