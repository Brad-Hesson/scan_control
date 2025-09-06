use std::{
    fmt::Debug,
    marker::PhantomData,
    ops::RangeBounds,
    sync::{Arc, OnceLock},
};

use bytemuck::{AnyBitPattern, NoUninit};
use glam::{Affine2, Mat3, Mat4};
use wgpu::{
    Buffer, BufferAddress, BufferDescriptor, BufferUsages, Device, Extent3d, Queue, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

#[derive(Debug)]
pub struct StorageBuffer<T: Clone + NoUninit + AnyBitPattern> {
    pub inner: Buffer,
    pd: PhantomData<T>,
}
impl<T: Clone + NoUninit + AnyBitPattern> StorageBuffer<T> {
    pub fn new(
        device: &Device,
        label: Option<&str>,
        usage: BufferUsages,
        size: usize,
        init_fn: impl FnOnce(&mut [T]),
    ) -> Self {
        let inner = device.create_buffer(&BufferDescriptor {
            label,
            size: (size * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: true,
        });
        init_fn(bytemuck::cast_slice_mut(
            inner.get_mapped_range_mut(..).as_mut(),
        ));
        inner.unmap();
        Self {
            inner,
            pd: PhantomData,
        }
    }
    pub fn queue_write(&self, queue: &Queue, offset: usize, data: &[T]) {
        queue.write_buffer(
            &self.inner,
            offset as u64 * size_of::<T>() as u64,
            bytemuck::cast_slice(data),
        );
    }
    pub fn queue_download_with<W>(
        &self,
        device: &Device,
        queue: &Queue,
        range: impl RangeBounds<usize>,
        f: impl FnOnce(&[T]) -> W + Send + 'static,
    ) -> Arc<OnceLock<W>>
    where
        W: Sync + Send + Debug + 'static,
    {
        let buf = Arc::new(std::sync::OnceLock::new());
        let buf_clone = buf.clone();
        let range = rangebounds_map(range, |v| (*v * size_of::<T>()) as BufferAddress);
        wgpu::util::DownloadBuffer::read_buffer(
            device,
            queue,
            &self.inner.slice(range),
            move |db| {
                buf.set(f(bytemuck::cast_slice(&db.unwrap()))).unwrap();
            },
        );
        buf_clone
    }
    pub fn queue_download(
        &self,
        device: &Device,
        queue: &Queue,
        range: impl RangeBounds<usize>,
    ) -> Arc<OnceLock<Box<[T]>>>
    where
        T: Sync + Send + Debug,
    {
        self.queue_download_with(device, queue, range, |r| r.to_vec().into_boxed_slice())
    }
}

fn rangebounds_map<I, O>(
    range: impl RangeBounds<I>,
    mut f: impl FnMut(&I) -> O,
) -> impl RangeBounds<O> {
    (
        range.start_bound().map(&mut f),
        range.end_bound().map(&mut f),
    )
}

pub struct TransformBuffer(pub Buffer);
impl TransformBuffer {
    pub fn new(device: &Device) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("quad2world uniform"),
            size: std::mem::size_of::<glam::Mat4>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self(buffer)
    }
    pub fn set(&self, queue: &Queue, transform: Affine2) {
        let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(transform.matrix2));
        mat4.w_axis.x = transform.translation.x;
        mat4.w_axis.y = transform.translation.y;
        queue.write_buffer(&self.0, 0, bytemuck::bytes_of(mat4.as_ref()));
    }
}

pub struct ImageTexture(pub Texture);
impl ImageTexture {
    pub fn new(device: &Device, size: [u32; 2]) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
            view_formats: &[TextureFormat::R32Float],
        });
        Self(texture)
    }
}

pub struct ColorMapTexture(pub Texture);
impl ColorMapTexture {
    pub const SIZE: usize = 1024;
    pub fn new(device: &Device) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: Self::SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D1,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[TextureFormat::Rgba8UnormSrgb],
        });
        Self(texture)
    }
    pub fn set(&self, queue: &Queue, color_map: &[egui::Color32; Self::SIZE]) {
        queue.write_texture(
            self.0.as_image_copy(),
            bytemuck::cast_slice(color_map),
            eframe::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(Self::SIZE as u32 * std::mem::size_of::<u8>() as u32 * 4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: Self::SIZE as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}