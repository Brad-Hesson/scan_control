use eframe::wgpu::{ComputePipeline, Device};

use super::shaders::{self, copy_texture, ImageBuffer, ImageTexture, MetadataBuffer};

pub struct CopyTextureResources {
    pub pipeline: ComputePipeline,
}
impl CopyTextureResources {
    pub fn new(device: &Device) -> Self {
        let pipeline = copy_texture::create_main_pipeline(device);
        Self { pipeline }
    }
}

pub struct CopyTextureBindGroup {
    pub metadata_buffer: MetadataBuffer,
    pub bind_group: copy_texture::BindGroup,
}
impl CopyTextureBindGroup {
    pub fn new(device: &Device, texture: &ImageTexture, buffer: &ImageBuffer) -> Self {
        let metadata_buffer = shaders::MetadataBuffer::new(device);
        let bind_group = copy_texture::BindGroup::new(device, &metadata_buffer, buffer, texture);
        Self {
            metadata_buffer,
            bind_group,
        }
    }
}
