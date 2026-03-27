use std::path::Path;

use eframe::egui_wgpu;
use egui::{Color32, DragValue, Id, PointerButton, Pos2, Response, Sense, Stroke, Ui, Vec2};
use float_ord::FloatOrd;
use glam::{Affine2, Mat3, Vec3Swizzles};
use image_compute::file_image::FileImageBuffers;
use itertools::{izip, Itertools};

use crate::scan_view::{callbacks::FileImageCallback, view::ScanViewCtx, ImageEncoder};

pub struct FileImage {
    pub transform: Mat3,
    buffers: FileImageBuffers,
    pub name: String,
    pub local_points: [egui::Pos2; 4],
    pub world_points: [egui::Pos2; 4],
    id: egui::Id,
    editing: bool,
}
impl FileImage {
    pub fn new(
        id_salt: impl std::hash::Hash,
        image_encoder: &ImageEncoder,
        path: impl AsRef<Path>,
        transform: Mat3,
    ) -> Self {
        let img = image::open(&path).unwrap();
        let name = path.as_ref().file_name().unwrap().to_string_lossy();
        let buffers = FileImageBuffers::new(
            &image_encoder.wgpu_state.device,
            &image_encoder.wgpu_state.queue,
            &img,
        );
        let local_points = [
            egui::Pos2::new(-0.5, -0.5),
            egui::Pos2::new(0.5, -0.5),
            egui::Pos2::new(0.5, 0.5),
            egui::Pos2::new(-0.5, 0.5),
        ];
        let world_points = local_points
            .iter()
            .map(|p| transform.project_egui_pos(*p))
            .collect_array()
            .unwrap();
        Self {
            id: egui::Id::new(id_salt),
            transform,
            buffers,
            name: name.to_string(),
            local_points,
            world_points,
            editing: false,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let callback = egui_wgpu::Callback::new_paint_callback(
            ctx.rect,
            FileImageCallback {
                transform: self.transform,
                image_buffers: self.buffers.clone(),
            },
        );
        ui.painter().add(callback);
        if self.editing {
            for (i, c) in POINT_COLORS.into_iter().enumerate() {
                let p = &mut self.world_points[i];
                let center = ctx.world2egui().project_egui_pos(*p);
                let resp = drag_point((self.id, i), ui, center, 8., c)
                    .on_hover_cursor(egui::CursorIcon::Move);
                if resp.dragged_by(egui::PointerButton::Primary) {
                    *p += ctx
                        .world2egui()
                        .inverse()
                        .project_egui_vec(resp.drag_delta());
                    self.update_transform();
                } else if resp.dragged_by(egui::PointerButton::Secondary) {
                    *p += ctx
                        .world2egui()
                        .inverse()
                        .project_egui_vec(resp.drag_delta());
                    self.update_local_points();
                }
            }
        }
    }
    fn update_local_points(&mut self) {
        self.local_points = self
            .world_points
            .iter()
            .map(|p| self.transform.inverse().project_egui_pos(*p))
            .collect_array()
            .unwrap();
    }
    fn update_transform(&mut self) {
        let mut a = nalgebra::SMatrix::<f32, 8, 8>::zeros();
        let mut b = nalgebra::SVector::<f32, 8>::zeros();

        for i in 0..4 {
            let x = self.local_points[i].x;
            let y = self.local_points[i].y;
            let u = self.world_points[i].x;
            let v = self.world_points[i].y;

            let r0 = 2 * i;
            let r1 = r0 + 1;

            a[(r0, 0)] = x;
            a[(r0, 1)] = y;
            a[(r0, 2)] = 1.0;
            a[(r0, 6)] = -u * x;
            a[(r0, 7)] = -u * y;
            b[r0] = u;

            a[(r1, 3)] = x;
            a[(r1, 4)] = y;
            a[(r1, 5)] = 1.0;
            a[(r1, 6)] = -v * x;
            a[(r1, 7)] = -v * y;
            b[r1] = v;
        }

        if let Some(h) = a.lu().solve(&b) {
            self.transform =
                Mat3::from_cols_array(&[h[0], h[3], h[6], h[1], h[4], h[7], h[2], h[5], 1.0]);
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui) {
        if self.editing {
            if ui.button("Lock").clicked() {
                self.editing = false;
            }
            if ui.button("Flip X").clicked() {
                self.flip_x();
            }
            if ui.button("Flip Y").clicked() {
                self.flip_y();
            }
        } else {
            if ui.button("Edit").clicked() {
                self.editing = true;
            }
        }
    }
    fn flip_x(&mut self) {
        let c_x = self.world_points.iter().map(|p| p.x).sum::<f32>() / 4.;
        for p in &mut self.world_points {
            let del = p.x - c_x;
            p.x -= 2. * del;
        }
        self.update_transform();
    }
    fn flip_y(&mut self) {
        let c_y = self.world_points.iter().map(|p| p.y).sum::<f32>() / 4.;
        for p in &mut self.world_points {
            let del = p.y - c_y;
            p.y -= 2. * del;
        }
        self.update_transform();
    }
}

pub trait ProjectionTransform: Copy {
    #[inline]
    fn project_egui_pos(self, p: egui::Pos2) -> egui::Pos2 {
        let p = self.project_glam_pos(glam::Vec2::new(p.x, p.y));
        egui::pos2(p.x, p.y)
    }
    #[inline]
    fn project_egui_vec(self, p: egui::Vec2) -> egui::Vec2 {
        let p = self.project_glam_vec(glam::Vec2::new(p.x, p.y));
        egui::vec2(p.x, p.y)
    }
    #[inline]
    fn project_glam_pos(self, p: glam::Vec2) -> glam::Vec2 {
        let p = self.mat3() * p.extend(1.0);
        p.xy() / p.z
    }
    #[inline]
    fn project_glam_vec(self, p: glam::Vec2) -> glam::Vec2 {
        let p = self.mat3() * p.extend(0.0);
        p.xy()
    }
    fn mat3(self) -> Mat3;
}
impl ProjectionTransform for Mat3 {
    #[inline]
    fn mat3(self) -> Mat3 {
        self
    }
}
impl ProjectionTransform for Affine2 {
    fn mat3(self) -> Mat3 {
        self.into()
    }
    fn project_glam_pos(self, p: glam::Vec2) -> glam::Vec2 {
        self.transform_point2(p)
    }
    fn project_glam_vec(self, p: glam::Vec2) -> glam::Vec2 {
        self.transform_vector2(p)
    }
}

const POINT_COLORS: [Color32; 4] = [
    Color32::MAGENTA,
    Color32::MAGENTA,
    Color32::MAGENTA,
    Color32::MAGENTA,
];

fn drag_point(
    id_salt: impl std::hash::Hash,
    ui: &mut Ui,
    center: Pos2,
    radius: f32,
    color: impl Into<Color32>,
) -> Response {
    let resp = ui.interact(
        egui::Rect::from_center_size(
            center,
            egui::Vec2 {
                x: radius * 2.,
                y: radius * 2.,
            },
        ),
        egui::Id::new(id_salt),
        Sense::HOVER | Sense::DRAG,
    );

    if resp.hovered() {
        ui.painter()
            .circle_filled(center, radius * 1.3, Color32::CYAN);
    }
    ui.painter().circle_filled(center, radius, color);

    resp
}
