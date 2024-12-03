use std::time::Duration;

use egui_colorgradient::Gradient;
use glam::{Affine2, Vec2};
use itertools::iproduct;

use crate::scan_view::{ScanImage, ScanView};

pub struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    scan_view: ScanView,
    images: Vec<ScanImage>,
    time: f32,
    gradient: egui_colorgradient::Gradient,
    last_gradient: egui_colorgradient::Gradient,
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
            Affine2::from_scale_angle_translation(Vec2::ONE / 2., 0., Vec2::ZERO),
        );
        images.push(image);
        let gradient = Gradient::default();
        scan_view.set_color_map(
            gradient
                .linear_eval(ScanView::COLOR_MAP_SIZE, true)
                .try_into()
                .unwrap(),
        );
        Self {
            scan_view,
            images,
            time: 0.,
            last_gradient: gradient.clone(),
            gradient,
        }
    }
    fn update_gradient(&mut self) {
        if self.gradient != self.last_gradient {
            self.last_gradient = self.gradient.clone();
            self.scan_view.set_color_map(
                self.gradient
                    .linear_eval(ScanView::COLOR_MAP_SIZE, true)
                    .into_boxed_slice()
                    .try_into()
                    .unwrap(),
            );
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("The triangle is being painted using ");
                ui.hyperlink_to("glow", "https://github.com/grovesNL/glow");
                ui.label(" (OpenGL).");
            });
            egui_colorgradient::gradient_editor(ui, &mut self.gradient);
            self.update_gradient();
            self.scan_view.show(ui, |ctx| {
                let dt = ctx.ui.input(|is| is.unstable_dt);
                self.time += dt;
                for image in &mut self.images {
                    // image.transform.matrix2 = Mat2::from_angle(0.5 * dt) * image.transform.matrix2;
                    // image.set_image_data(4, Box::new([self.time % 3. - 1.]));
                    image.show(ctx);
                }
            });
        });
        ctx.request_repaint_after(Duration::from_millis(40));
    }
}
