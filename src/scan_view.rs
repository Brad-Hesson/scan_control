use core::f32;

use eframe::{
    egui_wgpu::{self, Callback, RenderState},
    wgpu::{Extent3d, TextureFormat},
};
use egui::InnerResponse;
use glam::{Affine2, Mat3, Mat4, Vec2, Vec4};
use global::GlobalCallback;
use image::ImageCallback;
use uuid::Uuid;

mod copy_texture;
mod global;
mod image;
mod shaders;

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
                GlobalCallback {
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

        // calculate the screen transform
        let screen_transform =
            Affine2::from_scale(v2(rect.size()) * Vec2::new(0.5, -0.5)).inverse();

        screen_transform * self.world_transform
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

fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}
