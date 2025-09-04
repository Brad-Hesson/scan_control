use eframe::wgpu::{DeviceDescriptor, PresentMode};
use eyre::Context as _;
use tracing::info;
use tracing_subscriber::EnvFilter;
use wgpu::{
    Adapter, Device, FeaturesWGPU, FeaturesWebGPU, Instance, PowerPreference, Queue,
    RequestAdapterOptions,
};

mod app;
mod components;
mod scan_view;

fn main() -> eframe::Result {
    // env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let (instance, adapter, device, queue) =
        init_wgpu().map_err(|e| eframe::Error::AppCreation(e.into()))?;

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
            present_mode: PresentMode::AutoNoVsync,
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::Existing(
                eframe::egui_wgpu::WgpuSetupExisting {
                    instance,
                    adapter,
                    device,
                    queue,
                },
            ),
            ..Default::default()
        },
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "Scan Control",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MyApp::new(cc)))),
    )
}

fn init_wgpu() -> eyre::Result<(Instance, Adapter, Device, Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = smol::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .context("Adapter request failed")?;
    info!("Backend: {}", adapter.get_info().backend.to_str());
    info!("Adapter: {}", adapter.get_info().name);
    info!(
        "Driver: {} {}",
        adapter.get_info().driver,
        adapter.get_info().driver_info
    );
    let (dev, queue) = smol::block_on(adapter.request_device(&DeviceDescriptor {
        required_features: wgpu::Features {
            features_wgpu: FeaturesWGPU::TIMESTAMP_QUERY_INSIDE_PASSES
                | FeaturesWGPU::SHADER_F64
                | FeaturesWGPU::SHADER_INT64,
            features_webgpu: FeaturesWebGPU::FLOAT32_FILTERABLE | FeaturesWebGPU::TIMESTAMP_QUERY,
        },
        ..Default::default()
    }))
    .context("Device request failed")?;
    Ok((instance, adapter, dev, queue))
}
