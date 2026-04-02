use core::f32;

use egui::{DragValue, Response, Ui, WidgetText};
use glam::{Affine2, DAffine2};
use image_compute::image_compute::{FitData, FitType};
use itertools::{izip, Itertools};
use nanonis_tcp::LineDir;

use crate::{
    components::combo_box::{combo_box, ComboBoxType},
    connection::{backing::BufferState, nanonis_connection::toggle_dir},
    scan_view::{
        static_image::{MetersFmt, NormType, StaticImage},
        ImageEncoder, ScanViewImage,
    },
};

pub struct LiveImage {
    image_view: ScanViewImage,
    pub buffers: BufferState,
    pub line_dir: LineDir,
    pub transform: DAffine2,
    pub norm_type: NormType,
    pub std_dev: f32,
    pub fit_type: FitType,
    pub channel: Channel,
    pub signal_names: Vec<String>,
    pub channel_opts: Vec<usize>,
    pub name: String,
}

impl LiveImage {
    pub fn new(encoder: &ImageEncoder, buffers: BufferState, transform: DAffine2) -> Self {
        let norm_type = NormType::FullScale;
        let std_dev = 0.;
        Self {
            image_view: ScanViewImage::new(
                encoder,
                [buffers.size[1] as u32, buffers.size[0] as u32],
                transform,
                norm_type.combined(std_dev),
                |buf| buf.fill(f32::NAN),
            ),
            buffers,
            transform,
            norm_type,
            std_dev,
            line_dir: LineDir::Forward,
            fit_type: FitType::MeanSubtract,
            channel: Channel::None,
            signal_names: Vec::new(),
            channel_opts: Vec::new(),
            name: String::new(),
        }
    }
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        self.image_view.norm_type = self.norm_type.combined(self.std_dev);
        self.image_view.transform = self.transform;
        self.image_view.show(ui)
    }
    pub fn show_menu(&mut self, ui: &mut Ui, image_encoder: &mut ImageEncoder) {
        let vis = &mut ui.style_mut().visuals.widgets.inactive;
        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
        if combo_box(
            ui,
            (self.image_view.uuid(), "fit type"),
            &mut self.fit_type,
            &(),
        ) {
            self.update_texture(image_encoder);
        }
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
        let channel_opts = self
            .channel_opts
            .iter()
            .map(|ch| (*ch, self.signal_names[*ch].clone()))
            .collect_vec();
        combo_box(
            ui,
            (self.image_view.uuid(), "channel"),
            &mut self.channel,
            &channel_opts,
        );
        if ui.button(self.line_dir.opt_atoms(&())).clicked() {
            toggle_dir(&mut self.line_dir);
            self.update_texture(image_encoder);
        }
        let norm = self.image_view.norm_data.read();
        let fit = self.image_view.fit_data.read();
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
        let src = match self.line_dir {
            LineDir::Forward => &self.buffers.buf_f,
            LineDir::Backward => &self.buffers.buf_b,
        };
        self.image_view
            .write_lines(image_encoder, .., |buf| buf.copy_from_slice(src))
            .unwrap();
        self.image_view.write_texture(image_encoder, self.fit_type);
    }
    pub fn clear_texture(&self, image_encoder: &ImageEncoder) {
        self.image_view.clear(image_encoder);
    }
    pub fn size(&self) -> [u32; 2] {
        self.image_view.size()
    }
    pub fn resize(&mut self, image_encoder: &ImageEncoder, new_size: [u32; 2]) {
        self.image_view = ScanViewImage::new(
            image_encoder,
            new_size,
            self.transform,
            self.norm_type.combined(self.std_dev),
            |buf| buf.fill(f32::NAN),
        );
    }
    pub fn stamp(&self, encoder: &ImageEncoder, buffers: BufferState) -> StaticImage {
        let channel = match self.channel {
            Channel::None => "None".into(),
            Channel::Channel(ch) => self
                .signal_names
                .get(ch)
                .map(String::from)
                .unwrap_or_default(),
        };
        let mut image = StaticImage::new(encoder, self.transform, channel, buffers);
        image.fit_type = self.fit_type;
        image.norm_type = self.norm_type;
        image.std_dev = self.std_dev;
        image.name = self.name.clone();
        image.line_dir = self.line_dir;
        image.update_texture(encoder);
        image
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
