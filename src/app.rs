use std::path::Path;

use crate::components::file_dialog::ViewportFileDialog;
use crate::scan_view::{ScanImage, ScanView};
use egui::{emath::OrderedFloat, Button, MenuBar, Ui};
use egui_file_dialog::FileDialog;
use eyre::{Context, Result};
use glam::{Affine2, Vec2};
use tracing::{error, info};

pub struct MyApp {
    scan_view: ScanView,
    images: Vec<ScanImage>,
    file_dialog: ViewportFileDialog,
    // gradient: egui_colorgradient::Gradient,
    // last_gradient: egui_colorgradient::Gradient,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        // let gradient = Gradient::default();
        // scan_view.set_color_map(
        //     gradient
        //         .linear_eval(ScanView::COLOR_MAP_SIZE, true)
        //         .try_into()
        //         .expect("must be a ScanView::COLOR_MAP_SIZE bug"),
        // );
        Self {
            scan_view: ScanView::new(wgpu),
            images: vec![],
            file_dialog: ViewportFileDialog::new(FileDialog::new().title("Import File")),
            // last_gradient: gradient.clone(),
            // gradient,
        }
    }
    // fn update_gradient(&mut self) {
    //     if self.gradient != self.last_gradient {
    //         self.last_gradient = self.gradient.clone();
    //         self.scan_view.set_color_map(
    //             self.gradient
    //                 .linear_eval(ScanView::COLOR_MAP_SIZE, true)
    //                 .try_into()
    //                 .expect("must be a ScanView::COLOR_MAP_SIZE bug"),
    //         );
    //     }
    // }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(path) = self.file_dialog.take_picked() {
            if let Err(e) = load_file(self, path).context("file load failed") {
                error!("{e:#}");
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            let is_fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fs));
        }
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            MenuBar::new().ui(ui, |ui| {
                file_menu_button(ui, ctx, self);
            });
        });
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            let tr = self.scan_view.world_transform.inverse();
            let (scale, _, translation) = tr.to_scale_angle_translation();
            ui.label(format!("scale: {scale}, translation: {translation}"));
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                // egui_colorgradient::gradient_editor(ui, &mut self.gradient);
                // self.update_gradient();
                self.scan_view.show(ui, |ctx| {
                    for image in &mut self.images {
                        image.show(ctx);
                    }
                });
            });
    }
}

fn file_menu_button(ui: &mut Ui, ctx: &egui::Context, app: &mut MyApp) {
    ui.menu_button("File", |ui| {
        if ui.add(Button::new("Import")).clicked() {
            app.file_dialog.pick_file();
        }
    });
    app.file_dialog.update(ctx);
}

fn load_file(app: &mut MyApp, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    info!("Trying to load image `{}`", path.display());
    let file = sxmfile::SXM::parse_file(&path)?;
    info!("Loaded image `{}`", path.display());
    let [width, _] = file.get_image_size()?;
    let mut data = file.data[0][0].clone();
    let max = data
        .iter()
        .copied()
        .map(OrderedFloat)
        .max()
        .unwrap()
        .into_inner();
    let min = data
        .iter()
        .copied()
        .map(OrderedFloat)
        .min()
        .unwrap()
        .into_inner();
    for d in &mut data {
        *d = (*d - min) / (max - min);
    }
    let scale = file.get_scan_range()?;
    let translation = file.get_scan_center()?;
    let transform = Affine2::from_scale_angle_translation(
        Vec2::from(scale) * 1e9,
        0.,
        Vec2::from(translation) * 1e9,
    );
    let new_image = ScanImage::new(width, data, transform);
    app.images.push(new_image);
    Ok(())
}
