use std::sync::Arc;

use eframe::egui_wgpu::RenderState;
use image_compute::image_compute::ImageComputePipeline;

mod border;
mod callbacks;
mod image;
mod view;

pub use border::BorderRectangle;
pub use image::ScanImage;
pub use view::ScanView;

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


fn v2(v: impl Into<mint::Vector2<f32>>) -> glam::Vec2 {
    v.into().into()
}
