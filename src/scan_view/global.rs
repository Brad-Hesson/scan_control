use super::{
    image::ImageResources,
    shaders::{self, copy_texture, scan_image, ColorMapTexture, TransformBuffer},
};
use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, ComputePipeline, Device, RenderPipeline, TextureFormat},
};
use egui::ahash::{HashMap, HashMapExt};
use glam::Affine2;
use uuid::Uuid;

pub(super) struct GlobalCallback {
    pub target_format: TextureFormat,
    pub screen_transform: Affine2,
    pub new_color_map: Option<Box<[egui::Color32; ColorMapTexture::SIZE]>>,
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
            .screen_transform_buf
            .set(queue, self.screen_transform);

        // If there is a new color map, write it to the texture
        if let Some(color_map) = &self.new_color_map {
            global_res.color_map_texture.set(queue, color_map);
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
    pub scan_image_pipeline: RenderPipeline,
    pub image_copy_pipeline: ComputePipeline,
    pub global_bind_group: scan_image::GlobalBindGroup,
    pub screen_transform_buf: TransformBuffer,
    pub color_map_texture: ColorMapTexture,
    pub images: HashMap<Uuid, ImageResources>,
}
impl GlobalResources {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let scan_image_pipeline = shaders::scan_image::create_main_pipeline(device, target_format);
        let image_copy_pipeline = copy_texture::create_main_pipeline(device);
        let screen_transform_buf = TransformBuffer::new(device);
        let color_map_texture = ColorMapTexture::new(device);
        let global_bind_group =
            scan_image::GlobalBindGroup::new(device, &screen_transform_buf, &color_map_texture);
        Self {
            scan_image_pipeline,
            global_bind_group,
            screen_transform_buf,
            image_copy_pipeline,
            color_map_texture,
            images: HashMap::new(),
        }
    }
}
