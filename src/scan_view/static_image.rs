use core::f32;
use std::{fmt::Display, hash::Hash, ops::RangeBounds};

use egui::{ComboBox, DragValue, Response, Ui};
use glam::Affine2;
use image_compute::{
    buffers::BufferOpError,
    image_compute::{FitData, FitType, NormalizationType},
};
use itertools::{izip, Itertools};
use tracing::warn;

use crate::scan_view::{ImageEncoder, ScanImage};

pub struct StaticImage {
    image_data: ScanImage,
    pub transform: Affine2,
    pub fit_type: FitType,
    pub norm_type: NormType,
    pub std_dev: f32,
    pub name: String,
    pub signal_names: Vec<String>,
    pub channels: Vec<usize>,
    pub channel: Option<usize>,
}
impl StaticImage {
    pub fn new(
        encoder: &ImageEncoder,
        size: [u32; 2],
        transform: Affine2,
        init_fn: impl FnOnce(&mut [f32]),
    ) -> Self {
        let image_data = ScanImage::new(
            encoder,
            size,
            transform,
            NormalizationType::FullScale,
            init_fn,
        );
        Self {
            image_data,
            fit_type: FitType::MeanSubtract,
            norm_type: NormType::FullScale,
            std_dev: 1.5,
            transform,
            name: String::new(),
            signal_names: vec![],
            channels: vec![],
            channel: None,
        }
    }
    pub fn size(&self) -> [u32; 2] {
        self.image_data.size()
    }
    pub fn write_lines(
        &self,
        image_encoder: &ImageEncoder,
        lines: impl RangeBounds<u32>,
        callback: impl Fn(&mut [f32]),
    ) -> Result<(), BufferOpError> {
        self.image_data.write_lines(image_encoder, lines, callback)
    }
    pub fn update_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.write_texture(image_encoder, self.fit_type);
    }
    pub fn clear_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.clear(image_encoder);
    }
    pub fn resize(&mut self, image_encoder: &ImageEncoder, new_size: [u32; 2]) {
        self.image_data = ScanImage::new(
            image_encoder,
            new_size,
            self.transform,
            norm_type(self.norm_type, self.std_dev),
            |buf| buf.fill(f32::NAN),
        );
    }
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        self.image_data.norm_type = norm_type(self.norm_type, self.std_dev);
        self.image_data.transform = self.transform;
        self.image_data.show(ui)
    }
    pub fn show_image_menu(&mut self, ui: &mut Ui, image_encoder: &mut ImageEncoder) {
        let vis = &mut ui.style_mut().visuals.widgets.inactive;
        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
        if combo_box(
            ui,
            (self.image_data.uuid(), "fit type"),
            FIT_TYPES,
            &mut self.fit_type,
        ) {
            self.update_texture(image_encoder);
        }
        ui.horizontal(|ui| {
            combo_box(
                ui,
                (self.image_data.uuid(), "norm type"),
                NORM_TYPES,
                &mut self.norm_type,
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
            .channels
            .iter()
            .map(|ch| (Some(*ch), self.signal_names[*ch].as_str()))
            .collect_vec();
        combo_box(
            ui,
            (self.image_data.uuid(), "channel"),
            channel_opts.as_slice(),
            &mut self.channel,
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
}

fn norm_type(norm_type: NormType, std_dev: f32) -> NormalizationType {
    match norm_type {
        NormType::FullScale => image_compute::image_compute::NormalizationType::FullScale,
        NormType::StdDev => image_compute::image_compute::NormalizationType::StdDev(std_dev),
    }
}

const FIT_TYPES: &[(FitType, &'static str)] = &[
    (FitType::MeanSubtract, "Raw"),
    (FitType::LineMeanSubtract, "Subtract Average"),
    (FitType::LineFitSubtract, "Subtract Linear Fit"),
    (FitType::PlaneFitSubtract, "Subtract Plane"),
];
const NORM_TYPES: &[(NormType, &'static str)] = &[
    (NormType::FullScale, "Full Scale"),
    (NormType::StdDev, "Std Dev"),
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NormType {
    FullScale,
    StdDev,
}

fn combo_box<'s, T: Eq + Copy>(
    ui: &mut Ui,
    id_salt: impl Hash,
    types: &[(T, &'s str)],
    data: &mut T,
) -> bool {
    ComboBox::new((id_salt, "combo_box"), "")
        .selected_text(
            types
                .iter()
                .find(|(t, _)| *t == *data)
                .map(|(_, name)| *name)
                .unwrap_or(""),
        )
        .show_ui(ui, |ui| {
            let clicked = types
                .iter()
                .map(|(typ, name)| ui.selectable_value(data, *typ, *name))
                .any(|resp| resp.clicked());
            clicked
        })
        .inner
        .is_some_and(|clicked| clicked)
}

#[repr(transparent)]
struct MetersFmt(f32);
impl Display for MetersFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mag = (self.0.abs().log10() / 3.).floor();
        let scaled = self.0 / (10f32).powf(mag * 3.);
        let suf = match mag as i32 {
            4 => Some("Tm"),
            3 => Some("Gm"),
            2 => Some("Mm"),
            1 => Some("km"),
            0 => Some("m"),
            -1 => Some("mm"),
            -2 => Some("μm"),
            -3 => Some("nm"),
            -4 => Some("pm"),
            -5 => Some("fm"),
            _ => None,
        };
        if let Some(suf) = suf {
            f32::fmt(&scaled, f)?;
            write!(f, " {}", suf)?;
        } else if self.0 == 0. {
            f32::fmt(&self.0, f)?;
            write!(f, " m")?;
        } else {
            warn!("unimplemented `MetersFmt` base for value: `{}`", self.0);
            f32::fmt(&self.0, f)?;
            write!(f, " m")?;
        }
        Ok(())
    }
}
