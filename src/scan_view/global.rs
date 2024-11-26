use super::{
    copy_texture::CopyTextureResources,
    image::ImageResources,
    shaders::{self, image_view, TransformBuffer},
};
use eframe::{
    egui_wgpu::{self, CallbackTrait},
    wgpu::{self, Device, Queue, RenderPipeline, TextureFormat},
};
use egui::ahash::{HashMap, HashMapExt};
use glam::Affine2;
use uuid::Uuid;

pub(super) struct GlobalCallback {
    pub target_format: TextureFormat,
    pub screen_transform: Affine2,
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
        global_res.set_screen_transform(queue, self.screen_transform);
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
    pub pipeline: RenderPipeline,
    pub global_bg: image_view::GlobalBindGroup,
    pub screen_transform_buf: TransformBuffer,
    pub images: HashMap<Uuid, ImageResources>,
    pub copy_texture: CopyTextureResources,
}
impl GlobalResources {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let pipeline = shaders::image_view::create_main_pipeline(device, target_format);
        let screen_transform_buf = TransformBuffer::new(device);
        let global_bg = image_view::GlobalBindGroup::new(device, &screen_transform_buf);
        Self {
            pipeline,
            global_bg,
            screen_transform_buf,
            images: HashMap::new(),
            copy_texture: CopyTextureResources::new(device),
        }
    }
    fn set_screen_transform(&self, queue: &Queue, transform: Affine2) {
        self.screen_transform_buf.set(queue, transform);
    }
}
