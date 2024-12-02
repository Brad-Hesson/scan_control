use std::sync::Arc;

use eframe::wgpu::{DeviceDescriptor, Features, PresentMode};

mod app;
mod scan_view;

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0])
            .with_icon(
                // NOTE: Adding an icon is optional
                eframe::icon_data::from_png_bytes(&include_bytes!("../assets/icon-256.png")[..])
                    .expect("Failed to load icon"),
            ),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            present_mode: PresentMode::Immediate,
            device_descriptor: Arc::new(|adapter| {
                dbg!(adapter.get_info().name);
                DeviceDescriptor {
                    label: Some("Scan Control Device Descriptor"),
                    required_features: Features::FLOAT32_FILTERABLE,
                    ..Default::default()
                }
            }),
            ..Default::default()
        },
        multisampling: 1,
        ..Default::default()
    };
    eframe::run_native(
        "Scan Control",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MyApp::new(cc)))),
    )
}
