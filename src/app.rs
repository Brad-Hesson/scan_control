use std::{borrow::Cow, sync::Arc};

use bytemuck::bytes_of;
use eframe::{
    egui_wgpu::{self, CallbackTrait, RenderState},
    wgpu::{self, util::DeviceExt, BindGroup, Buffer, Device, Queue, RenderPipeline},
};
use egui::Vec2;
use glam::Mat4;

pub struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    world_transform: glam::Mat4,
    triangle: ImageCallback,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        Self {
            world_transform: glam::Affine3A::IDENTITY.into(),
            triangle: ImageCallback::new(wgpu),
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
        let (rect, response) = ui.allocate_at_least(
            ui.available_size_before_wrap() - Vec2::Y * 20.,
            egui::Sense::drag(),
        );
        let del = Mat4::from_translation(
            glam::Vec3::new(response.drag_motion().x, -response.drag_motion().y, 0.0) / 100.,
        );
        self.world_transform = del * self.world_transform;

        // Clone locals so we can move them into the paint callback:
        self.triangle.queue.write_buffer(
            &self.triangle.uniform_buf,
            0,
            bytes_of(self.world_transform.as_ref()),
        );
        self.triangle.queue.submit([]);
        let callback = egui_wgpu::Callback::new_paint_callback(rect, self.triangle.clone());
        ui.painter().add(callback);
    }
}

#[derive(Clone)]
struct ImageCallback {
    device: Arc<Device>,
    pipeline: Arc<RenderPipeline>,
    queue: Arc<Queue>,
    bind_group: Arc<BindGroup>,
    uniform_buf: Arc<Buffer>,
}
impl ImageCallback {
    fn new(wgpu: &RenderState) -> Self {
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
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(64),
                        },
                        count: None,
                    }],
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

        let uniform_buf = wgpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Uniform Buffer"),
                contents: bytemuck::bytes_of(Mat4::IDENTITY.as_ref()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        // Create bind group
        let bind_group = wgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
            label: None,
        });
        Self {
            device: wgpu.device.clone(),
            pipeline: Arc::new(pipeline),
            queue: wgpu.queue.clone(),
            bind_group: Arc::new(bind_group),
            uniform_buf: Arc::new(uniform_buf),
        }
    }
}
impl CallbackTrait for ImageCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_pipeline(&self.pipeline);
        render_pass.draw(0..4, 0..1);
    }
}

