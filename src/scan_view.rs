use core::f32;
use std::{mem::MaybeUninit, sync::Arc};

use eframe::{
    egui_wgpu::{self, Callback, RenderState},
    wgpu::TextureFormat,
};
use egui::{
    epaint::{ColorMode, PathShape, PathStroke},
    mutex::RwLock,
    Color32, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
};
use glam::{Affine2, Vec2};
use global::GlobalCallback;
use image::ImageCallback;
use image_compute::image_compute::{
    ImageComputeBuffers, ImageComputePipeline, NormalizeData, WriteLinesError,
};
use uuid::Uuid;

use crate::app::COLOR_MAP_SIZE;

mod global;
mod image;

// #[derive(Clone)]
pub struct ScanView {
    pub world_transform: Affine2,
    target_format: TextureFormat,
    new_color_map: Option<Box<[egui::Color32; COLOR_MAP_SIZE]>>,
}
impl ScanView {
    pub fn show<R>(
        &mut self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut ScanViewCtx) -> R,
    ) -> Response {
        egui::Frame::canvas(ui.style())
            .show(ui, |ui| {
                let (rect, response) =
                    ui.allocate_at_least(ui.available_size_before_wrap(), egui::Sense::all());
                let screen_transform = self.handle_inputs(ui, response.clone());
                ui.painter().add(Callback::new_paint_callback(
                    rect,
                    GlobalCallback {
                        target_format: self.target_format,
                        screen_transform,
                        new_color_map: std::mem::take(&mut self.new_color_map),
                    },
                ));
                let mut ctx = ScanViewCtx {
                    ui,
                    rect,
                    world_transform: self.world_transform,
                };
                add_contents(&mut ctx);
                response
            })
            .inner
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
            let cursor_pos = v2(response.interact_pointer_pos().unwrap() - rect.center());
            let drag_vec = v2(response.drag_delta());
            let angle = cursor_pos.perp_dot(drag_vec) / cursor_pos.length_squared();
            Affine2::from_angle(angle)
        } else {
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

        screen_transform * self.world_transform
    }
    pub fn new(wgpu: &RenderState) -> Self {
        let mut color_map: Box<MaybeUninit<[egui::Color32; COLOR_MAP_SIZE]>> = Box::new_uninit();
        for i in 0..COLOR_MAP_SIZE {
            let color = i as f32 / (COLOR_MAP_SIZE - 1) as f32;
            unsafe {
                color_map.assume_init_mut()[i] = Color32::from_gray((color * 255.) as u8);
            }
        }
        Self {
            new_color_map: Some(unsafe { color_map.assume_init() }),
            world_transform: Affine2::IDENTITY,
            target_format: wgpu.target_format,
        }
    }
    pub fn set_color_map(&mut self, color_map: Box<[egui::Color32; COLOR_MAP_SIZE]>) {
        self.new_color_map = Some(color_map);
    }
}

pub struct ScanViewCtx<'a> {
    pub ui: &'a mut egui::Ui,
    pub rect: egui::Rect,
    pub world_transform: Affine2,
}

pub struct ScanImage {
    uuid: Uuid,
    pub transform: Affine2,
    changes: Vec<(usize, Box<[f32]>)>,
    image_data: Arc<RwLock<ImageComputeBuffers>>,
    pub fit_data: Arc<RwLock<Option<FitData>>>,
    pub norm_data: Arc<RwLock<Option<NormalizeData>>>,
}
impl ScanImage {
    pub fn new(
        wgpu_state: &RenderState,
        size: [u32; 2],
        lines: u32,
        transform: Affine2,
        init_fn: impl FnOnce(&mut [f32]),
    ) -> Self {
        let image_data = Arc::new(RwLock::new(ImageComputeBuffers::new(
            &wgpu_state.device,
            &wgpu_state.queue,
            None,
            size,
            lines,
            init_fn,
        )));
        Self {
            uuid: Uuid::new_v4(),
            transform,
            image_data,
            changes: vec![],
            fit_data: Arc::new(RwLock::new(None)),
            norm_data: Arc::new(RwLock::new(None)),
        }
    }
    pub fn show(&mut self, ctx: &mut ScanViewCtx) -> Response {
        let resp = ctx
            .ui
            .input(|i| i.pointer.latest_pos())
            .and_then(|pos| {
                let [x, y] =
                    (Affine2::from_translation(<[f32; 2]>::from(ctx.rect.center()).into())
                        * ctx.world_transform
                        * self.transform)
                        .inverse()
                        .transform_point2(<[f32; 2]>::from(pos).into())
                        .abs()
                        .into();
                (x < 1. && y < 1.).then(|| {
                    ctx.ui.interact(
                        ctx.rect,
                        egui::Id::new(self.uuid),
                        Sense::focusable_noninteractive() | Sense::click(),
                    )
                })
            })
            .unwrap_or_else(|| neutral_response(ctx.ui, egui::Id::new(self.uuid)));
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            ImageCallback {
                transform: self.transform,
                changes: std::mem::take(&mut self.changes),
                image_buffers: self.image_data.clone(),
            },
        );
        ctx.ui.painter().add(callback);
        resp
    }
    pub fn write_texture_mean_subtract(
        &self,
        wgpu_state: &RenderState,
        image_encoder: &mut ImageEncoder,
    ) {
        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::wgt::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch_mean_subtract(
                &wgpu_state.device,
                &mut pass,
                &self.image_data.read(),
            );
        }
        wgpu_state.queue.submit([encoder.finish()]);
    }
    pub fn write_texture_plane_fit_subtract(
        &self,
        wgpu_state: &RenderState,
        image_encoder: &mut ImageEncoder,
    ) {
        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::wgt::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch_plane_fit_subtract(
                &wgpu_state.device,
                &mut pass,
                &self.image_data.read(),
            );
        }
        wgpu_state.queue.submit([encoder.finish()]);
        let norm_data = self.norm_data.clone();
        let fit_data = self.fit_data.clone();
        let image_data = self.image_data.clone();
        let device = wgpu_state.device.clone();
        let queue = wgpu_state.queue.clone();
        let current_size = self.current_size()[0] * self.current_size()[1];
        wgpu_state.queue.on_submitted_work_done(move || {
            image_data
                .read()
                .download_normalize_data(&device, &queue, move |data| {
                    *norm_data.write() = Some(*data)
                });
            image_data
                .read()
                .download_planarize_data(&device, &queue, ..3, move |data| {
                    *fit_data.write() = Some(FitData::PlaneFit {
                        mean: data[0] / current_size as f64,
                        x_slope: data[1],
                        y_slope: data[2],
                    })
                });
        });
    }
    pub fn write_texture_line_fit_subtract(
        &self,
        wgpu_state: &RenderState,
        image_encoder: &mut ImageEncoder,
    ) {
        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::wgt::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch_line_fit_subtract(
                &wgpu_state.device,
                &mut pass,
                &self.image_data.read(),
            );
        }
        wgpu_state.queue.submit([encoder.finish()]);
    }
    pub fn write_texture_line_mean_subtract(
        &self,
        wgpu_state: &RenderState,
        image_encoder: &mut ImageEncoder,
    ) {
        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::wgt::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            image_encoder.pipeline.dispatch_line_mean_subtract(
                &wgpu_state.device,
                &mut pass,
                &self.image_data.read(),
            );
        }
        wgpu_state.queue.submit([encoder.finish()]);
    }
    pub fn write_line(
        &mut self,
        wgpu_state: &RenderState,
        line: &[f32],
    ) -> Result<(), WriteLinesError> {
        self.image_data.write().write_line(&wgpu_state.queue, line)
    }
    pub fn current_size(&self) -> [u32; 2] {
        self.image_data.read().current_size()
    }
    pub fn is_full(&self) -> bool {
        self.current_size() == self.image_data.read().capacity()
    }
}

pub enum FitData {
    PlaneFit {
        mean: f64,
        x_slope: f64,
        y_slope: f64,
    },
}

pub struct ImageEncoder {
    pipeline: ImageComputePipeline,
}
impl ImageEncoder {
    pub fn new(wgpu_state: &RenderState) -> Self {
        let pipeline = ImageComputePipeline::new(&wgpu_state.device);
        Self { pipeline }
    }
}

pub struct BorderRectangle {
    pub transform: Affine2,
    pub color: Color32,
    pub dashed: bool,
}
impl BorderRectangle {
    pub fn show(&mut self, ctx: &mut ScanViewCtx) {
        let t = Affine2::from_translation(v2(ctx.rect.center().to_vec2()))
            * ctx.world_transform
            * self.transform;
        let p0: [f32; 2] = t.transform_point2(Vec2::new(-1.0, -1.0)).into();
        let p1: [f32; 2] = t.transform_point2(Vec2::new(1.0, -1.0)).into();
        let p2: [f32; 2] = t.transform_point2(Vec2::new(1.0, 1.0)).into();
        let p3: [f32; 2] = t.transform_point2(Vec2::new(-1.0, 1.0)).into();
        let mut points = vec![p0.into(), p1.into(), p2.into(), p3.into()];
        let stroke = PathStroke {
            width: 2.,
            color: ColorMode::Solid(self.color),
            kind: StrokeKind::Outside,
        };
        if self.dashed {
            points.push(p0.into());
            let mut shapes = Vec::new();
            dashes_from_line(&points, stroke, &[6.], &[3.], &mut shapes, 0.);
            ctx.ui.painter().add(shapes);
        } else {
            ctx.ui.painter().add(PathShape {
                points,
                closed: true,
                fill: Color32::TRANSPARENT,
                stroke,
            });
        }
    }
}

fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}

fn neutral_response(ui: &egui::Ui, id: egui::Id) -> Response {
    ui.interact(
        Rect::from_center_size(Pos2::ZERO, egui::Vec2::ZERO),
        id,
        Sense::empty(),
    )
}

/// Creates dashes from a line.
fn dashes_from_line(
    path: &[Pos2],
    stroke: PathStroke,
    dash_lengths: &[f32],
    gap_lengths: &[f32],
    shapes: &mut Vec<Shape>,
    dash_offset: f32,
) {
    assert_eq!(
        dash_lengths.len(),
        gap_lengths.len(),
        "Mismatched dash and gap lengths, got dash_lengths: {}, gap_lengths: {}",
        dash_lengths.len(),
        gap_lengths.len()
    );
    let mut position_on_segment = dash_offset;
    let mut drawing_dash = false;
    let mut step = 0;
    let steps = dash_lengths.len();
    for window in path.windows(2) {
        let (start, end) = (window[0], window[1]);
        let vector = end - start;
        let segment_length = vector.length();

        let mut start_point = start;
        while position_on_segment < segment_length {
            let new_point = start + vector * (position_on_segment / segment_length);
            if drawing_dash {
                // This is the end point.
                shapes.push(Shape::Path(PathShape {
                    points: [start_point, new_point].into(),
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: stroke.clone(),
                }));
                position_on_segment += gap_lengths[step];
                // Increment step counter
                step += 1;
                if step >= steps {
                    step = 0;
                }
            } else {
                // Start a new dash.
                start_point = new_point;
                position_on_segment += dash_lengths[step];
            }
            drawing_dash = !drawing_dash;
        }

        // If the segment ends and the dash is not finished, add the segment's end point.
        if drawing_dash {
            shapes.push(Shape::Path(PathShape {
                points: [start_point, end].into(),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: stroke.clone(),
            }));
        }

        position_on_segment -= segment_length;
    }
}
