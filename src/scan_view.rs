use core::f32;
use std::mem::MaybeUninit;

use eframe::{
    egui_wgpu::{self, Callback, RenderState},
    wgpu::{Extent3d, TextureFormat},
};
use egui::{
    emath::Rot2,
    epaint::{PathShape, PathStroke},
    Color32, InnerResponse, Pos2, Rect, Response, Sense, Stroke,
};
use glam::{Affine2, Vec2, Vec3};
use global::GlobalCallback;
use image::ImageCallback;
use shaders::ColorMapTexture;
use uuid::Uuid;

use crate::utils::SelectableMember;

mod global;
mod image;
mod shaders;

#[derive(Clone)]
pub struct ScanView {
    pub world_transform: Affine2,
    target_format: TextureFormat,
    new_color_map: Option<Box<[egui::Color32; ColorMapTexture::SIZE]>>,
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
        let mut color_map: Box<MaybeUninit<[egui::Color32; ColorMapTexture::SIZE]>> =
            Box::new_uninit();
        for i in 0..ColorMapTexture::SIZE {
            let color = i as f32 / (ColorMapTexture::SIZE - 1) as f32;
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
    pub const COLOR_MAP_SIZE: usize = ColorMapTexture::SIZE;
    pub fn set_color_map(&mut self, color_map: Box<[egui::Color32; Self::COLOR_MAP_SIZE]>) {
        self.new_color_map = Some(color_map);
    }
}

pub struct ScanViewCtx<'a> {
    pub ui: &'a mut egui::Ui,
    pub rect: egui::Rect,
    pub world_transform: Affine2,
}

#[derive(Clone)]
pub struct ScanImage {
    uuid: Uuid,
    pub transform: Affine2,
    size: [u32; 2],
    changes: Vec<(usize, Box<[f32]>)>,
    selected: bool,
}
impl ScanImage {
    pub fn new(width: u32, data: Box<[f32]>, transform: Affine2) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            transform,
            size: [width, data.len() as u32 / width],
            changes: vec![(0, data)],
            selected: false,
        }
    }
    pub fn show(&mut self, ctx: &mut ScanViewCtx) -> Response {
        let resp = ctx
            .ui
            .input(|i| i.pointer.latest_pos())
            .map(|pos| {
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
            .flatten()
            .unwrap_or_else(|| neutral_response(ctx.ui, egui::Id::new(self.uuid)));
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            ImageCallback {
                uuid: self.uuid,
                transform: self.transform,
                size: self.size,
                changes: std::mem::take(&mut self.changes),
            },
        );
        ctx.ui.painter().add(callback);
        resp
    }
    pub fn set_image_data(&mut self, offset: usize, data: Box<[f32]>) {
        self.changes.push((offset, data));
    }
}
impl PartialEq for ScanImage {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}
impl Eq for ScanImage {}
impl SelectableMember for ScanImage {
    fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

pub struct BorderRectangle {
    pub transform: Affine2,
    pub color: Color32,
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
        ctx.ui.painter().add(PathShape {
            points: vec![p0.into(), p1.into(), p2.into(), p3.into()],
            closed: true,
            fill: Color32::TRANSPARENT,
            stroke: PathStroke {
                width: 2.,
                color: egui::epaint::ColorMode::Solid(self.color),
                kind: egui::StrokeKind::Outside,
            },
        });
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
