use std::{borrow::Cow, sync::Arc};

use bytemuck::bytes_of;
use eframe::{
    egui_wgpu::{self, CallbackTrait, RenderState},
    wgpu::{
        self, util::DeviceExt, BindGroup, BindGroupEntry, BindGroupLayout, Buffer, Device,
        Extent3d, FilterMode, Queue, RenderPipeline, Texture, TextureDescriptor, TextureUsages,
    },
};
use egui::InnerResponse;
use glam::{Affine2, Mat3, Mat4, Vec2, Vec4};

fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}

fn affine2_to_mat4(af: Affine2) -> Mat4 {
    let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(af.matrix2));
    let trans = af.translation;
    mat4.w_axis = Vec4::new(trans.x, trans.y, 0., 1.);
    mat4
}

#[derive(Clone)]
pub struct ScanView {
    world_transform: Affine2,
    rotate_center: Option<Vec2>,
    pipeline: Arc<RenderPipeline>,
    queue: Arc<Queue>,
    global_bg: Arc<BindGroup>,
    image_bgl: Arc<BindGroupLayout>,
    world2screen_buf: Arc<Buffer>,
    device: Arc<Device>,
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
            self.handle_inputs(ui, response);
            let mut ctx = ScanViewCtx { ui, rect };
            add_contents(&mut ctx)
        })
    }
    fn handle_inputs(&mut self, ui: &mut egui::Ui, response: egui::Response) {
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

        // calculate the screen transform
            let screen_transform =
                Affine2::from_scale(v2(rect.size()) * Vec2::new(0.5, -0.5)).inverse();

        // write the new transform to the GPU
        self.queue.write_buffer(
            &self.world2screen_buf,
            0,
            bytes_of(affine2_to_mat4(screen_transform * self.world_transform).as_ref()),
        );
    }
    pub fn new(wgpu: &RenderState) -> Self {
        let shader = wgpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "./shaders/scan_image.wgsl"
                ))),
            });

        let global_bgl = wgpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let image_bgl = wgpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let pipeline_layout = wgpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&global_bgl, &image_bgl],
                push_constant_ranges: &[],
            });

        let pipeline = wgpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    targets: &[Some(wgpu.target_format.into())],
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
        let world_transform = Affine2::IDENTITY;
        let world2screen_buf = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("world2screen uniform"),
                contents: bytemuck::bytes_of(affine2_to_mat4(world_transform).as_ref()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let sampler = wgpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let global_bg = wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            world_transform,
            rotate_center: None,
            pipeline: Arc::new(pipeline),
            queue: wgpu.queue.clone(),
            global_bg: Arc::new(global_bg),
            image_bgl: Arc::new(image_bgl),
            world2screen_buf: Arc::new(world2screen_buf),
            device: wgpu.device.clone(),
        }
    }
}

pub struct ScanViewCtx<'a> {
    pub ui: &'a mut egui::Ui,
    pub rect: egui::Rect,
}

#[derive(Clone)]
pub struct ScanImage {
    pub transform: Affine2,
    quad2world_buf: Arc<Buffer>,
    texture: Arc<Texture>,
    local_bind_group: Arc<BindGroup>,
    global_bind_group: Arc<BindGroup>,
    pipeline: Arc<RenderPipeline>,
}
impl ScanImage {
    pub fn new(scan_view: &ScanView, data: &[f32], width: u32, transform: Affine2) -> ScanImage {
        let quad2world_buf =
            scan_view
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("quad2world uniform"),
                    contents: bytemuck::bytes_of(affine2_to_mat4(transform).as_ref()),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
        let texture = scan_view.device.create_texture_with_data(
            &scan_view.queue,
            &TextureDescriptor {
                label: None,
                size: Extent3d {
                    width,
                    height: data.len() as u32 / width,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[wgpu::TextureFormat::R32Float],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(data),
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let local_bind_group = scan_view
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &scan_view.image_bgl,
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
        ScanImage {
            texture: Arc::new(texture),
            global_bind_group: scan_view.global_bg.clone(),
            pipeline: scan_view.pipeline.clone(),
            transform,
            local_bind_group: Arc::new(local_bind_group),
            quad2world_buf: Arc::new(quad2world_buf),
        }
    }
    pub fn show(&self, ctx: &mut ScanViewCtx) {
        let callback = egui_wgpu::Callback::new_paint_callback(ctx.rect, self.clone());
        ctx.ui.painter().add(callback);
    }
}
impl CallbackTrait for ScanImage {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        queue.write_buffer(
            &self.quad2world_buf,
            0,
            bytes_of(affine2_to_mat4(self.transform).as_ref()),
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.global_bind_group, &[]);
        render_pass.set_bind_group(1, &self.local_bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}
