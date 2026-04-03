use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use image_compute::image_compute::ImageComputePipeline;

mod border;
mod callbacks;
mod scan_image;
pub mod static_image;
mod view;
mod file_image;
mod gds_image;
mod scale_bar;

pub use border::BorderRectangle;
pub use scan_image::ScanViewImage;
pub use view::ScanView;
pub use file_image::FileImage;
pub use gds_image::GDSImage;
pub use scale_bar::ScaleBar;

#[derive(Clone)]
pub struct ImageEncoder {
    pipeline: Arc<ImageComputePipeline>,
    wgpu_state: RenderState,
}
impl ImageEncoder {
    pub fn new(wgpu_state: &RenderState) -> Self {
        let pipeline = Arc::new(ImageComputePipeline::new(&wgpu_state.device));
        Self {
            pipeline,
            wgpu_state: wgpu_state.clone(),
        }
    }
}
