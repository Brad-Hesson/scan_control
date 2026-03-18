use std::ops::RangeBounds;

use egui::{DragValue, Response, Ui, WidgetText};
use glam::Affine2;
use image_compute::{
    buffers::BufferOpError,
    image_compute::{FitData, FitType},
};
use itertools::{izip, Itertools};

use crate::{
    components::combo_box::{combo_box, ComboBoxType},
    scan_view::{
        static_image::{MetersFmt, NormType},
        ImageEncoder, ScanImage,
    },
};

pub struct LiveImage {
    image_data: ScanImage,
    pub transform: Affine2,
    pub norm_type: NormType,
    pub std_dev: f32,
    pub fit_type: FitType,
    pub channel: Channel,
    pub signal_names: Vec<String>,
    pub channel_opts: Vec<usize>,
    pub name: String,
}

impl LiveImage {
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        self.image_data.norm_type = self.norm_type.combined(self.std_dev);
        self.image_data.transform = self.transform;
        self.image_data.show(ui)
    }
    pub fn show_menu(&mut self, ui: &mut Ui, image_encoder: &mut ImageEncoder) {
        let vis = &mut ui.style_mut().visuals.widgets.inactive;
        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
        if combo_box(
            ui,
            (self.image_data.uuid(), "fit type"),
            &mut self.fit_type,
            &(),
        ) {
            self.update_texture(image_encoder);
        }
        ui.horizontal(|ui| {
            combo_box(
                ui,
                (self.image_data.uuid(), "norm type"),
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
        let channel_opts = self
            .channel_opts
            .iter()
            .map(|ch| (*ch, self.signal_names[*ch].clone()))
            .collect_vec();
        combo_box(
            ui,
            (self.image_data.uuid(), "channel"),
            &mut self.channel,
            &channel_opts,
        );
        let norm = self.image_data.norm_data.read();
        let fit = self.image_data.fit_data.read();
        if let (Some(norm), Some(fit)) = (norm.as_ref(), fit.as_ref()) {
            let mean = fit.mean();
            ui.label(format!("Max:     {:.2}", MetersFmt(norm.max)));
            ui.label(format!("Gap:    {:.2}", MetersFmt(norm.max - norm.min)));
            ui.label(format!("Min:     {:.2}", MetersFmt(norm.min)));
            ui.label(format!("Mean:    {:.2}", MetersFmt(mean)));
            ui.label(format!("Std Dev: {:.2}", MetersFmt(norm.stddev)));
            match fit {
                FitData::PlaneFitSubtract {
                    x_slope, y_slope, ..
                } => {
                    ui.label(format!("X Slope: {:.2}", MetersFmt(*x_slope)));
                    ui.label(format!("Y Slope: {:.2}", MetersFmt(*y_slope)));
                }
                FitData::MeanSubtract { .. } => {}
                FitData::LineMeanSubtract { means } => {
                    for m in means {
                        ui.label(format!("{:.2}", MetersFmt(*m)));
                    }
                }
                FitData::LineFitSubtract { means, slopes } => {
                    for (m, s) in izip!(means, slopes) {
                        ui.label(format!("{:.2}  {:.2}", MetersFmt(*m), MetersFmt(*s)));
                    }
                }
            }
        }
    }
    pub fn update_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.write_texture(image_encoder, self.fit_type);
    }
    pub fn clear_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.clear(image_encoder);
    }
    pub fn write_lines(
        &self,
        image_encoder: &ImageEncoder,
        lines: impl RangeBounds<u32>,
        callback: impl Fn(&mut [f32]),
    ) -> Result<(), BufferOpError> {
        self.image_data.write_lines(image_encoder, lines, callback)
    }
    pub fn size(&self) -> [u32; 2] {
        self.image_data.size()
    }
    pub fn resize(&mut self, image_encoder: &ImageEncoder, new_size: [u32; 2]) {
        self.image_data = ScanImage::new(
            image_encoder,
            new_size,
            self.transform,
            self.norm_type.combined(self.std_dev),
            |buf| buf.fill(f32::NAN),
        );
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Channel {
    None,
    Channel(usize),
}
impl Channel {
    pub fn is_some(&self) -> bool {
        *self != Channel::None
    }
}
impl ComboBoxType for Channel {
    type Ctx = Vec<(usize, String)>;

    fn opt_atoms(&self, channels: &Self::Ctx) -> impl Into<egui::WidgetText> {
        match self {
            Channel::None => WidgetText::Text("".into()),
            Channel::Channel(ch) => channels
                .iter()
                .find_map(|(name_ch, name)| (name_ch == ch).then(|| name.into()))
                .unwrap_or("".into()),
        }
    }

    fn options(channels: &Self::Ctx) -> impl Iterator<Item = Self> {
        channels.iter().map(|(ch, _)| Channel::Channel(*ch))
    }
}
