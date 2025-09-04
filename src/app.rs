use std::time::Duration;

use egui::{emath::OrderedFloat, Button, Label, MenuBar, Ui};
use egui_file_dialog::FileDialog;
// use egui_colorgradient::Gradient;
use glam::{Affine2, Vec2};
use itertools::iproduct;
use tracing::{error, info};

use crate::scan_view::{ScanImage, ScanView};

pub struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    scan_view: ScanView,
    images: Vec<ScanImage>,
    file_picker: FileDialog,
    // gradient: egui_colorgradient::Gradient,
    // last_gradient: egui_colorgradient::Gradient,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        let mut scan_view = ScanView::new(wgpu);
        let mut images = vec![];
        let width = 512;
        let height = 512;
        let mut data = vec![0.; width * height];
        let mut row_sums = vec![0.; height];
        for (x, y) in iproduct!(0..width, 0..height) {
            let i = y * width + x;
            let row = y;
            let x = x as f32 / (width - 1) as f32 * 50.;
            let y = y as f32 / (height - 1) as f32 * 50.;
            let v = (x.sin() + y.sin()) / 4. + 0.5;
            data[i] = v;
            row_sums[row] += v / width as f32;
        }
        let image = ScanImage::new(
            width,
            data.into_boxed_slice(),
            Affine2::from_scale_angle_translation(Vec2::ONE * 100. / 2., 0., Vec2::ZERO),
        );
        // images.push(image);
        // let gradient = Gradient::default();
        // scan_view.set_color_map(
        //     gradient
        //         .linear_eval(ScanView::COLOR_MAP_SIZE, true)
        //         .try_into()
        //         .expect("must be a ScanView::COLOR_MAP_SIZE bug"),
        // );
        Self {
            scan_view,
            images,
            file_picker: FileDialog::new(), // last_gradient: gradient.clone(),
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
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            MenuBar::new().ui(ui, |ui| {
                file_menu_button(ui, self);
            });
        });
        self.file_picker.update(ctx);
        if let Some(path) = self.file_picker.take_picked() {
            'load_file: {
                info!("Trying to load image `{}`", path.display());
                let Ok(file) = sxmfile::SXM::parse_file(&path).inspect_err(|e| error!("{e}"))
                else {
                    break 'load_file;
                };
                info!("Loaded image `{}`", path.display());
                let Ok([width, _]) = file.get_image_size().inspect_err(|e| error!("{e}")) else {
                    break 'load_file;
                };
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
                let Ok(scale) = file.get_scan_range().inspect_err(|e| error!("{e}")) else {
                    break 'load_file;
                };
                let Ok(translation) = file.get_scan_center().inspect_err(|e| error!("{e}")) else {
                    break 'load_file;
                };
                let transform = Affine2::from_scale_angle_translation(
                    Vec2::from(scale) * 1e9,
                    0.,
                    Vec2::from(translation) * 1e9,
                );
                let new_image = ScanImage::new(width, data, transform);
                self.images.push(new_image);
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            // egui_colorgradient::gradient_editor(ui, &mut self.gradient);
            // self.update_gradient();
            self.scan_view.show(ui, |ctx| {
                for image in &mut self.images {
                    image.show(ctx);
                }
            });
        });
        egui::TopBottomPanel::bottom("menu_bar").show(ctx, |ui| {
            let tr = self.scan_view.world_transform.inverse();
            let (scale, _, translation) = tr.to_scale_angle_translation();
            ui.label(format!("scale: {scale}, translation: {translation}"));
        });
    }
}

fn file_menu_button(ui: &mut Ui, app: &mut MyApp) {
    ui.menu_button("File", |ui| {
        if ui.add(Button::new("Import")).clicked() {
            app.file_picker.pick_file();
        }
    });
}
