use wgpu::{
    Device, Extent3d, Queue, Texture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor,
};

#[derive(Clone)]
pub struct ColorMapTexture<const SIZE: usize>(Texture);
impl<const SIZE: usize> ColorMapTexture<SIZE> {
    pub fn new(device: &Device) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D1,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self(texture)
    }
    pub fn write(&self, queue: &Queue, color_map: &[egui::Color32; SIZE]) {
        queue.write_texture(
            self.0.as_image_copy(),
            bytemuck::try_cast_slice(color_map).unwrap(),
            eframe::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE as u32 * std::mem::size_of::<u8>() as u32 * 4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
    pub fn create_view(&self) -> TextureView {
        self.0.create_view(&TextureViewDescriptor::default())
    }
}
