use std::{fmt::Display, hash::Hash};

use egui::{ComboBox, DragValue, Ui};
use image_compute::image_compute::{FitData, FitType, NormalizationType};
use itertools::izip;
use tracing::warn;

use crate::scan_view::{ImageEncoder, ScanImage};

pub trait ImageMenu {
    fn fit_type_mut(&mut self) -> &mut FitType;
    fn image_data_mut(&mut self) -> &mut ScanImage;
    fn norm_type_mut(&mut self) -> &mut NormType;
    fn std_dev_mut(&mut self) -> &mut f32;

    fn show_image_menu(&mut self, ui: &mut Ui, image_encoder: &mut ImageEncoder) {
        let vis = &mut ui.style_mut().visuals.widgets.inactive;
        vis.weak_bg_fill = vis.weak_bg_fill.gamma_multiply(0.5);
        if combo_box(
            ui,
            (self.image_data_mut().uuid(), "fit type"),
            FIT_TYPES,
            self.fit_type_mut(),
        ) {
            let fit_type = *self.fit_type_mut();
            self.image_data_mut().write_texture(image_encoder, fit_type);
        }
        ui.horizontal(|ui| {
            combo_box(
                ui,
                (self.image_data_mut().uuid(), "norm type"),
                NORM_TYPES,
                self.norm_type_mut(),
            );
            if *self.norm_type_mut() == NormType::StdDev {
                ui.add(
                    DragValue::new(self.std_dev_mut())
                        .range((0.)..=(5.))
                        .speed(0.01),
                );
            }
        });
        let image_data = self.image_data_mut();
        let norm = image_data.norm_data.read();
        let fit = image_data.fit_data.read();
        if let (Some(norm), Some(fit)) = (norm.as_ref(), fit.as_ref()) {
            let mean = fit.mean();
            ui.label(format!("Max:     {:.2}", MetersFmt(norm.max + mean)));
            ui.label(format!("Gap:    {:.2}", MetersFmt(norm.max - norm.min)));
            ui.label(format!("Min:     {:.2}", MetersFmt(norm.min + mean)));
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

const FIT_TYPES: &[(FitType, &'static str)] = &[
    (FitType::LineFitSubtract, "Line Fit"),
    (FitType::LineMeanSubtract, "Line Mean Subtract"),
    (FitType::PlaneFitSubtract, "Plane Fit"),
    (FitType::MeanSubtract, "Mean Subtract"),
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

fn combo_box<T: Eq + Copy>(
    ui: &mut Ui,
    id_salt: impl Hash,
    types: &[(T, &'static str)],
    data: &mut T,
) -> bool {
    ComboBox::new((id_salt, "combo_box"), "")
        .selected_text(types.iter().find(|(t, _)| *t == *data).unwrap().1)
        .show_ui(ui, |ui| {
            ui.set_min_height(82.);
            types
                .iter()
                .map(|(typ, name)| ui.selectable_value(data, *typ, *name))
                .any(|resp| resp.clicked())
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
