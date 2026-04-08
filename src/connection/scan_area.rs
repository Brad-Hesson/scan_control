use egui::{Color32, DragValue, Id, Shape, Stroke, Ui};
use glam::{DAffine2, DVec2};

use crate::{
    connection::{nanonis::ScanStatus, LiveImage},
    scan_view::{world_delta_transform, BorderRectangle, ImageEncoder, ScanViewCtx},
    utils::vec_interop::{IntoEgui, Projection},
};

pub struct ScanArea {
    pub world_transform: DAffine2,
    pub area_size: DVec2,
    pub image_transform: DAffine2,
    pub live_image: LiveImage,
    pub channel_opts: Vec<String>,
    pub channel_selected: Option<String>,
    pub scan_status: ScanStatus,
    pub tip_pos: DVec2,
}
impl ScanArea {
    pub fn new(
        encoder: &ImageEncoder,
        area_size: DVec2,
        image_transform: DAffine2,
        tip_pos: DVec2,
    ) -> Self {
        let live_image = LiveImage::new(encoder, image_transform);
        Self {
            world_transform: DAffine2::IDENTITY,
            area_size,
            image_transform,
            live_image,
            channel_opts: vec![],
            channel_selected: None,
            scan_status: ScanStatus::default(),
            tip_pos,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        self.show_image(ui);
        self.show_scan_line(ui);
        self.show_area_border(ui);
        self.show_tip(ui);
        if ui.input(|i| i.modifiers.ctrl) {
            let tf = world_delta_transform(ui, self.live_image.transform.translation);
            self.image_transform = tf * self.image_transform;
        }
    }
    fn show_image(&mut self, ui: &mut Ui) {
        self.live_image.transform = self.world_transform * self.image_transform;
        self.live_image.show_image(ui);
    }
    fn show_area_border(&self, ui: &mut Ui) {
        BorderRectangle {
            transform: DAffine2::from_scale(self.area_size),
            color: Color32::YELLOW,
            dashed: false,
        }
        .show(ui);
    }
    fn show_scan_line(&self, ui: &mut Ui) {
        if let Some(y) = self
            .scan_status
            .scan_line_position(self.live_image.size(), self.live_image.line_dir)
        {
            let ctx = ui
                .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
                .unwrap();
            let tf = ctx.world2egui() * self.world_transform * self.image_transform;
            let p0 = tf.project_pos2(DVec2::new(-0.5, y)).to_egui_pos2();
            let p1 = tf.project_pos2(DVec2::new(0.5, y)).to_egui_pos2();
            ui.painter().extend(Shape::dashed_line(
                &[p0, p1],
                Stroke::new(1.0, Color32::BLUE),
                3. * 5.,
                1. * 5.,
            ));
        };
    }
    fn show_tip(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let tf = ctx.world2egui() * self.world_transform;
        let center = tf.project_pos2(self.tip_pos).to_egui_pos2();
        ui.painter().circle_filled(center, 3., Color32::BLUE);
    }
    pub fn show_menu(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        self.show_channel_control(ui);
        self.live_image.show_menu(ui, encoder);
    }
    pub fn show_channel_control(&mut self, ui: &mut Ui) {
        let mut selection = self.channel_selected.as_ref().map(|s| s.as_str());
        if egui::ComboBox::new((self.live_image.uuid(), "combo_box"), "")
            .selected_text(selection.unwrap_or_default())
            .show_ui(ui, |ui| {
                self.channel_opts
                    .iter()
                    .map(|opt| ui.selectable_value(&mut selection, Some(opt), opt))
                    .any(|resp| resp.clicked())
            })
            .inner
            .is_some_and(|clicked| clicked)
        {
            self.channel_selected = selection.map(|s| s.to_string());
        }
    }
}
