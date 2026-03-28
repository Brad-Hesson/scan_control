use egui::{
    epaint::{RectShape, TextShape},
    Align, Color32, CornerRadius, Id, Pos2, Shape, Ui, Vec2,
};

use crate::scan_view::{
    file_image::ProjectionTransform, static_image::MetersFmt, view::ScanViewCtx,
};

pub struct ScaleBar {}
impl ScaleBar {
    pub fn new() -> Self {
        Self {}
    }
    pub fn show(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let mut nm = ctx.world2egui().project_egui_vec(Vec2::new(1., 0.)).x;
        let pivot = 500.0;
        let mut mul = 0;
        while nm < pivot {
            nm *= 10.;
            mul += 1;
        }
        while nm > pivot {
            nm /= 10.;
            mul -= 1;
        }
        let pos = ctx.rect.right_bottom() - Vec2::new(5., 5.);
        let size = egui::Vec2 { x: nm, y: 5. };
        let rect = egui::Rect::from_center_size(pos - size / 2., size);
        // let rect = egui::Rect::from_center_size(ctx.rect.center(), egui::Vec2 { x: nm, y: 5. });
        ui.painter().add(Shape::Rect(RectShape::filled(
            rect,
            CornerRadius::ZERO,
            Color32::WHITE,
        )));
        let text = format!("{:.0}", MetersFmt(10f32.powi(mul - 9)));
        ui.painter().text(
            pos - Vec2::new(0., 5.),
            egui::Align2::RIGHT_BOTTOM,
            text,
            egui::FontId::monospace(24.),
            Color32::WHITE,
        );
    }
}
