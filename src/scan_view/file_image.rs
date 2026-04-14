use std::path::Path;

use eframe::egui_wgpu;
use egui::{Color32, Id, Response, Sense, Ui};
use glam::{DAffine2, DMat3, DVec2};
use image_compute::file_image::FileImageBuffers;
use itertools::Itertools;

use crate::{
    scan_view::{ImageEncoder, callbacks::FileImageCallback, view::ScanViewCtx, world_delta_transform},
    utils::vec_interop::{IntoEgui as _, IntoGlam as _, Projection},
};

pub struct FileImage {
    pub transform: DMat3,
    buffers: FileImageBuffers,
    pub local_points: [glam::DVec2; 4],
    pub world_points: [glam::DVec2; 4],
    id: egui::Id,
    editing: bool,
}
impl FileImage {
    pub fn new(
        id_salt: impl std::hash::Hash,
        image_encoder: &ImageEncoder,
        path: impl AsRef<Path>,
        transform: DMat3,
    ) -> Self {
        let img = image::open(&path).unwrap();
        let buffers = FileImageBuffers::new(
            &image_encoder.wgpu_state.device,
            &image_encoder.wgpu_state.queue,
            &img,
        );
        let local_points = [
            glam::DVec2::new(-0.5, -0.5),
            glam::DVec2::new(0.5, -0.5),
            glam::DVec2::new(0.5, 0.5),
            glam::DVec2::new(-0.5, 0.5),
        ];
        let world_points = local_points
            .iter()
            .map(|p| transform.project_pos2(*p))
            .collect_array()
            .unwrap();
        Self {
            id: egui::Id::new(id_salt),
            transform,
            buffers,
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
                let screen_pos = ctx.world2egui().project_pos2(*p).to_egui_pos2();
                let resp = drag_point((self.id, i), ui, screen_pos, 8., c)
                    .on_hover_cursor(egui::CursorIcon::Move);
                if resp.dragged_by(egui::PointerButton::Primary) {
                    *p += ctx
                        .world2egui()
                        .inverse()
                        .project_vec2(resp.drag_delta().to_glam());
                    self.update_transform();
                } else if resp.dragged_by(egui::PointerButton::Secondary) {
                    *p += ctx
                        .world2egui()
                        .inverse()
                        .project_vec2(resp.drag_delta().to_glam());
                    self.update_local_points();
                }
            }
        }
    }
    fn update_local_points(&mut self) {
        self.local_points = self
            .world_points
            .iter()
            .map(|p| self.transform.inverse().project_pos2(*p))
            .collect_array()
            .unwrap();
    }
    fn update_transform(&mut self) {
        let mut a = nalgebra::SMatrix::<f64, 8, 8>::zeros();
        let mut b = nalgebra::SVector::<f64, 8>::zeros();

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
                DMat3::from_cols_array(&[h[0], h[3], h[6], h[1], h[4], h[7], h[2], h[5], 1.0]);
        }
    }
    pub fn show_menu(&mut self, ui: &mut Ui) {
        if self.editing {
            if ui.button("Lock").clicked() {
                self.editing = false;
            }
            if ui.button("Flip X").clicked() {
                let tran = DAffine2::from_translation(self.center());
                let tf = DAffine2::from_scale(DVec2::new(-1., 1.));
                let tf = tran * tf * tran.inverse();
                self.transform_world_points(tf);
            }
            if ui.button("Flip Y").clicked() {
                let tran = DAffine2::from_translation(self.center());
                let tf = DAffine2::from_scale(DVec2::new(1., -1.));
                let tf = tran * tf * tran.inverse();
                self.transform_world_points(tf);
            }
        } else {
            if ui.button("Edit").clicked() {
                self.editing = true;
            }
        }
        ui.label(format!("{:?}", self.transform));
        ui.label(format!("{:?}", self.world_points));
        ui.label(format!("{:?}", self.local_points));
    }
    pub fn center(&self) -> glam::DVec2 {
        self.world_points.iter().sum::<glam::DVec2>() / 4.
    }
    pub fn transform_world_points(&mut self, tf: DAffine2) {
        for p in &mut self.world_points {
            *p = tf.project_pos2(*p);
        }
        self.update_transform();
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
    center: egui::Pos2,
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
