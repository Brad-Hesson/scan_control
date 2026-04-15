use std::collections::VecDeque;

use egui::{epaint::PathStroke, Color32, Id, Ui};
use glam::{DAffine2, DVec2};

use crate::{
    connection::{nanonis::ScanStatus, LiveImage},
    scan_view::{border::dashes_from_line, BorderRectangle, ImageEncoder, ScanViewCtx},
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
    pub stamp: VecDeque<LiveImage>,
    pub stamp_name_base: String,
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
            stamp: VecDeque::new(),
            stamp_name_base: String::new(),
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        self.show_image(ui);
        self.show_scan_line(ui);
        self.show_image_border(ui);
        self.show_area_border(ui);
        self.show_tip(ui);
    }
    fn show_image_border(&self, ui: &mut Ui) {
        let ctx = ui
            .data(|map| map.get_temp::<ScanViewCtx>(Id::new(())))
            .unwrap();
        let transform = self.world_transform * self.image_transform;
        BorderRectangle {
            transform,
            color: Color32::RED,
            dashed: false,
        }
        .show(ui);
        let bottom_left = ((ctx.world2egui() * transform).transform_point2(DVec2::new(-0.5, 0.5))
            + DVec2::new(-1., 1.))
        .to_egui_pos2();
        ui.painter().circle_filled(bottom_left, 3., Color32::RED);
    }
    fn show_image(&mut self, ui: &mut Ui) {
        self.live_image.transform = self.world_transform * self.image_transform;
        self.live_image.show_image(ui);
    }
    fn show_area_border(&self, ui: &mut Ui) {
        BorderRectangle {
            transform: self.world_transform * DAffine2::from_scale(self.area_size),
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

            ui.painter().extend(dashes_from_line(
                ctx.rect,
                &[p0, p1],
                PathStroke {
                    width: 1.0,
                    color: egui::epaint::ColorMode::Solid(Color32::BLUE),
                    kind: egui::StrokeKind::Middle,
                },
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
        self.show_stamp_button(ui, encoder);
    }
    fn show_stamp_button(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(
            self.stamp_name_base.clone(),
            font_id.clone(),
            ui.visuals().text_color(),
        );
        let padding = ui.spacing().button_padding.x * 2.0;
        let width = galley.size().x + padding;

        ui.horizontal(|ui| {
            if ui.button("Stamp").clicked() {
                self.stamp.push_front(self.live_image.stamp(encoder));
            }
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut self.stamp_name_base).desired_width(width),
            );
        });
    }
    fn show_channel_control(&mut self, ui: &mut Ui) {
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
