use core::f32;
use std::sync::Arc;

use egui::{DragValue, Response, Ui, WidgetText};
use glam::DAffine2;
use image_compute::image_compute::{FitData, FitType};
use nanonis_tcp::LineDir;

use crate::{
    components::combo_box::{combo_box, ComboBoxType},
    scan_view::{
        static_image::{MetersFmt, NormType},
        ImageEncoder, ScanViewImage,
    },
};

pub struct LiveImage {
    image_view: ScanViewImage,
    pub transform: DAffine2,
    pub std_dev: f32,
    pub fit_type: FitType,
    pub norm_type: NormType,
    pub line_dir: LineDir,
    pub forward_data: FrameData,
    pub backward_data: FrameData,
    pub channel_opts: Vec<String>,
    pub channel_selected: Option<String>,
}

impl LiveImage {
    pub fn new(encoder: &ImageEncoder, transform: DAffine2) -> Self {
        let norm_type = NormType::FullScale;
        let std_dev = 3.;
        let empty_data = FrameData::default();
        Self {
            image_view: ScanViewImage::new(
                encoder,
                empty_data.size,
                transform,
                norm_type.combined(std_dev),
            ),
            norm_type,
            std_dev,
            line_dir: LineDir::Forward,
            fit_type: FitType::MeanSubtract,
            forward_data: empty_data.clone(),
            backward_data: empty_data,
            channel_opts: vec![],
            channel_selected: None,
            transform,
        }
    }
    pub fn show_image(&mut self, ui: &mut Ui) -> Response {
        self.image_view.transform = self.transform;
        self.image_view.norm_type = self.norm_type.combined(self.std_dev);
        self.image_view.show(ui)
    }
    pub fn show_menu(&mut self, ui: &mut Ui, encoder: &mut ImageEncoder) {
        let vis = &mut ui.style_mut().visuals.widgets.inactive;
        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
        self.show_channel_control(ui);
        self.show_fit_control(ui, encoder);
        self.show_normalization_control(ui);
        self.show_line_dir_control(ui, encoder);
        self.show_metadata(ui);
    }
    pub fn show_metadata(&self, ui: &mut Ui) {
        let norm = self.image_view.norm_data.read();
        let fit = self.image_view.fit_data.read();
        if let (Some(norm), Some(fit)) = (norm.as_ref(), fit.as_ref()) {
            ui.label(format!("Range:    {:.2}", MetersFmt(norm.max - norm.min)));
            ui.label(format!("Std Dev: {:.2}", MetersFmt(norm.stddev)));
            if let FitData::PlaneFitSubtract {
                x_slope, y_slope, ..
            } = fit
            {
                ui.label(format!("X Slope: {:.2}", MetersFmt(*x_slope)));
                ui.label(format!("Y Slope: {:.2}", MetersFmt(*y_slope)));
            }
        }
    }
    pub fn show_fit_control(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        if combo_box(
            ui,
            (self.image_view.uuid(), "fit type"),
            &mut self.fit_type,
            &(),
        ) {
            self.update_texture(encoder);
        };
    }
    pub fn show_normalization_control(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            combo_box(
                ui,
                (self.image_view.uuid(), "norm type"),
                &mut self.norm_type,
                &(),
            );
            if self.norm_type == NormType::StdDev {
                ui.add(
                    DragValue::new(&mut self.std_dev)
                        .range((0.)..=(9.))
                        .speed(0.01),
                );
            }
        });
    }
    pub fn show_line_dir_control(&mut self, ui: &mut Ui, encoder: &ImageEncoder) {
        let text = match self.line_dir {
            LineDir::Forward => "Forward",
            LineDir::Backward => "Backward",
        };
        if ui.button(text).clicked() {
            match self.line_dir {
                LineDir::Forward => self.line_dir = LineDir::Backward,

                LineDir::Backward => self.line_dir = LineDir::Forward,
            }
            self.write_and_update_texture(encoder);
        }
    }
    pub fn show_channel_control(&mut self, ui: &mut Ui) {
        let mut selection = self.channel_selected.as_ref().map(|s| s.as_str());
        if egui::ComboBox::new((self.image_view.uuid(), "combo_box"), "")
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
    pub fn update_texture(&self, encoder: &ImageEncoder) {
        self.image_view.write_texture(encoder, self.fit_type);
    }
    pub fn write_and_update_texture(&mut self, encoder: &ImageEncoder) {
        let new_data = match self.line_dir {
            LineDir::Forward => &self.forward_data,
            LineDir::Backward => &self.backward_data,
        };
        if self.image_view.size() != new_data.size {
            self.image_view = self.image_view.resized(encoder, new_data.size);
        }
        self.image_view
            .write_lines(encoder, .., |buf| buf.copy_from_slice(&new_data.data))
            .unwrap();
        self.update_texture(encoder);
    }
    pub fn clear_texture(&mut self, encoder: &ImageEncoder) {
        self.image_view.clear(encoder);
    }
}

impl ComboBoxType for LineDir {
    type Ctx = ();

    fn opt_atoms(&self, _ctx: &Self::Ctx) -> impl Into<WidgetText> {
        match self {
            LineDir::Forward => "Forward",
            LineDir::Backward => "Backward",
        }
    }

    fn options(_ctx: &Self::Ctx) -> impl Iterator<Item = Self> {
        [LineDir::Forward, LineDir::Backward].into_iter()
    }
}

#[derive(Clone)]
pub struct FrameData {
    pub size: [u32; 2],
    pub data: Arc<Box<[f32]>>,
}
impl Default for FrameData {
    fn default() -> Self {
        Self {
            size: [2, 2],
            data: Arc::new(vec![f32::NAN; 4].into_boxed_slice()),
        }
    }
}
