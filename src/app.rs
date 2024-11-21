use std::{borrow::Cow, sync::Arc};

use bytemuck::bytes_of;
use eframe::{
    egui_wgpu::{self, CallbackTrait, RenderState},
    wgpu::{self, util::DeviceExt, BindGroup, Buffer, Queue, RenderPipeline},
};
use glam::{Affine2, Mat3, Mat4, Vec2, Vec4};

pub struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    world_transform: Affine2,
    rotate_center: Option<Vec2>,
    image_callback: ImageCallback,
    images: Vec<Image>,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        let images = vec![Image::new(Vec2::ONE * 100., 1., Vec2::ZERO)];
        Self {
            world_transform: Affine2::IDENTITY,
            rotate_center: None,
            image_callback: ImageCallback::new(wgpu, images[0].to_owned()),
            images,
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("The triangle is being painted using ");
                ui.hyperlink_to("glow", "https://github.com/grovesNL/glow");
                ui.label(" (OpenGL).");
            });

            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                self.custom_painting(ui);
            });
            ui.label("Drag to rotate!");
        });
    }
}

impl MyApp {
    fn custom_painting(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_at_least(ui.available_size_before_wrap(), egui::Sense::drag());
        let drag = if response.dragged_by(egui::PointerButton::Primary) {
            Affine2::from_translation(v2(response.drag_delta()))
        } else {
            Affine2::IDENTITY
        };
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
        let zoom = if let Some(window_pos) = response.hover_pos() {
            let scalar = (ui.input(|is| is.raw_scroll_delta).y / 100.).exp();
            let scale = Affine2::from_scale(Vec2::splat(scalar));
            let trans = Affine2::from_translation(v2(window_pos - rect.center()));
            trans * scale * trans.inverse()
        } else {
            Affine2::IDENTITY
        };
        self.world_transform = rotate * zoom * drag * self.world_transform;

        let screen_transform =
            Affine2::from_scale(v2(rect.size()) * Vec2::new(0.5, -0.5)).inverse();

        let mat4 = affine2_to_mat4(screen_transform * self.world_transform);

        self.image_callback.queue.write_buffer(
            &self.image_callback.world2screen_buf,
            0,
            bytes_of(mat4.as_ref()),
        );
        let callback = egui_wgpu::Callback::new_paint_callback(rect, self.image_callback.clone());
        ui.painter().add(callback);
    }
}

fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}

fn affine2_to_mat4(af: Affine2) -> Mat4 {
    let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(af.matrix2));
    let trans = af.translation;
    mat4.w_axis = Vec4::new(trans.x, trans.y, 0., 1.);
    mat4
}

// #[derive(Clone)]
// struct ImageCallback {
//     pipeline: Arc<RenderPipeline>,
//     queue: Arc<Queue>,
//     bind_group: Arc<BindGroup>,
//     world2screen_buf: Arc<Buffer>,
//     quad2world_buf: Arc<Buffer>,
//     image: Image,
// }
#[derive(Clone)]
struct ImageCallback {
    pipeline: Arc<RenderPipeline>,
    queue: Arc<Queue>,
    bind_group: Arc<BindGroup>,
    world2screen_buf: Arc<Buffer>,
    quad2world_buf: Arc<Buffer>,
    image: Image,
}
impl ImageCallback {
    fn new(wgpu: &RenderState, image: Image) -> Self {
        let shader = wgpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("./image.wgsl"))),
            });

        let bind_group_layout =
            wgpu.device
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
                            visibility: wgpu::ShaderStages::VERTEX,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(64),
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = wgpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&bind_group_layout],
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

        let world2screen_buf = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("world2screen uniform"),
                contents: bytemuck::bytes_of(Mat4::IDENTITY.as_ref()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let quad2world_buf = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("quad2world uniform"),
                contents: bytemuck::bytes_of(Mat4::IDENTITY.as_ref()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        // Create bind group
        let bind_group = wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: world2screen_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: quad2world_buf.as_entire_binding(),
                },
            ],
            label: None,
        });
        Self {
            pipeline: Arc::new(pipeline),
            queue: wgpu.queue.clone(),
            bind_group: Arc::new(bind_group),
            world2screen_buf: Arc::new(world2screen_buf),
            quad2world_buf: Arc::new(quad2world_buf),
            image,
        }
    }
}
impl CallbackTrait for ImageCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let mat4 = affine2_to_mat4(self.image.transform);
        queue.write_buffer(&self.quad2world_buf, 0, bytes_of(mat4.as_ref()));
        Vec::new()
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..4, 0..1);
    }
}

#[derive(Clone)]
struct Image {
    transform: Affine2,
}
impl Image {
    fn new(scale: Vec2, angle: f32, translation: Vec2) -> Self {
        Self {
            transform: Affine2::from_scale_angle_translation(scale, angle, translation),
        }
    }
}
