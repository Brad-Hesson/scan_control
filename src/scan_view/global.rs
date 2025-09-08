use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, Device, TextureFormat},
};
use glam::Affine2;
use image_compute::scan_image::{ScanImageBuffers, ScanImagePipeline};

use crate::app::COLOR_MAP_SIZE;

pub(super) struct GlobalCallback {
    pub target_format: TextureFormat,
    pub screen_transform: Affine2,
    pub new_color_map: Option<Box<[egui::Color32; COLOR_MAP_SIZE]>>,
}
impl CallbackTrait for GlobalCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let global_res = callback_resources
            .entry::<GlobalResources>()
            .or_insert_with(|| GlobalResources::new(device, self.target_format));

        // Set the new screen transform
        global_res
            .scan_image_buffers
            .write_screen_transform(queue, self.screen_transform);

        // If there is a new color map, write it to the texture
        if let Some(color_map) = &self.new_color_map {
            global_res
                .scan_image_buffers
                .write_color_map(queue, color_map);
        }
        Vec::new()
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
    }
}

pub(super) struct GlobalResources {
    pub scan_image_pipeline: ScanImagePipeline,
    pub scan_image_buffers: ScanImageBuffers<COLOR_MAP_SIZE>,
}
impl GlobalResources {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let scan_image_pipeline = ScanImagePipeline::new(device, target_format);
        let scan_image_buffers = ScanImageBuffers::new(device);
        Self {
            scan_image_pipeline,
            scan_image_buffers,
        }
    }
}
