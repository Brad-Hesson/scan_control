use std::{f32::NAN, time::Duration};

use glam::{Affine2, Mat2, Vec2};
use itertools::iproduct;

use crate::scan_view::{ScanImage, ScanView};

pub struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    scan_view: ScanView,
    images: Vec<ScanImage>,
    time: f32,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let wgpu = cc.wgpu_render_state.as_ref().unwrap();
        let scan_view = ScanView::new(wgpu);
        let mut images = vec![];
        let scale = 100.;
        let height = 0.5;
        let mut data = vec![height; 5 * 5];
        data[0] = NAN;
        let image = ScanImage::new(
            5,
            data.into_boxed_slice(),
            Affine2::from_scale_angle_translation(Vec2::ONE * scale / 2., 0., Vec2::ZERO),
        );
        images.push(image);
        Self {
            scan_view,
            images,
            time: 0.,
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

            self.scan_view.show(ui, |ctx| {
                let dt = ctx.ui.input(|is| is.unstable_dt);
                self.time += dt;
                for image in &mut self.images {
                    image.transform.matrix2 = Mat2::from_angle(0.5 * dt) * image.transform.matrix2;
                    image.set_image_data(4, Box::new([self.time % 3. - 1.]));
                    image.show(ctx);
                }
            });
        });
        ctx.request_repaint_after_secs(0.040);
    }
}

#[test]
fn feature() {
    dbg!(unsafe{std::mem::transmute::<u32, f32>(0x3dcccccd)});
}