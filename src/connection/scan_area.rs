use egui::{Color32, DragValue, Ui};
use glam::{DAffine2, DVec2};

use crate::{
    connection::LiveImage,
    scan_view::{BorderRectangle, ImageEncoder, world_delta_transform},
};

pub struct ScanArea {
    pub world_transform: DAffine2,
    pub area_transform: DAffine2,
    pub image_transform: DAffine2,
    pub live_image: LiveImage,
    pub channel_opts: Vec<String>,
    pub channel_selected: Option<String>,
}
impl ScanArea {
    pub fn new(
        encoder: &ImageEncoder,
        area_transform: DAffine2,
        image_transform: DAffine2,
    ) -> Self {
        let live_image = LiveImage::new(encoder, image_transform);
        Self {
            world_transform: DAffine2::IDENTITY,
            area_transform,
            image_transform,
            live_image,
            channel_opts: vec![],
            channel_selected: None,
        }
    }
    pub fn show(&mut self, ui: &mut Ui) {
        self.live_image.transform = self.world_transform * self.image_transform;
        self.live_image.show_image(ui);
        BorderRectangle {
            transform: self.world_transform * self.area_transform,
            color: Color32::YELLOW,
            dashed: false,
        }
        .show(ui);
        if ui.input(|i| i.modifiers.ctrl) {
            let tf = world_delta_transform(ui, self.live_image.transform.translation);
            self.image_transform = tf * self.image_transform;
        }
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
