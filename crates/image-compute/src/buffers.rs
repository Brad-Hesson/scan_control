use std::{fmt::Debug, marker::PhantomData, num::NonZero, ops::RangeBounds};

use bytemuck::{AnyBitPattern, NoUninit};
use glam::{Affine2, Mat3, Mat4};
use wgpu::{
    Buffer, BufferAddress, BufferBinding, BufferDescriptor, BufferUsages, Device, Extent3d, Queue,
    Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

#[derive(Clone)]
#[repr(transparent)]
pub struct StorageBuffer<T: Clone + NoUninit + AnyBitPattern> {
    inner: Buffer,
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
    pub fn queue_write(
        &self,
        queue: &Queue,
        offset: usize,
        size: usize,
        callback: impl Fn(&mut [T]),
    ) -> Result<(), BufferOpError> {
        let mut view = queue
            .write_buffer_with(
                &self.inner,
                offset as u64 * size_of::<T>() as u64,
                NonZero::try_from(size as u64 * size_of::<T>() as u64)
                    .map_err(|_| BufferOpError::BufferSizeZero)?,
            )
            .expect("`Queue::write_buffer_with` failed");
        callback(bytemuck::cast_slice_mut(&mut *view));
        Ok(())
    }
    pub fn queue_download(
        &self,
        device: &Device,
        queue: &Queue,
        range: impl RangeBounds<usize>,
        callback: impl FnOnce(&[T]) + Send + 'static,
    ) -> Result<(), BufferOpError> {
        let range = rangebounds_map(range, |v| (*v * size_of::<T>()) as BufferAddress);
        if range_is_empty(&range) {
            return Err(BufferOpError::BufferSizeZero);
        }
        wgpu::util::DownloadBuffer::read_buffer(
            device,
            queue,
            &self.inner.slice(range),
            move |db| callback(bytemuck::cast_slice(&db.unwrap())),
        );
        Ok(())
    }
    pub fn as_entire_buffer_binding(&self) -> BufferBinding<'_> {
        self.inner.as_entire_buffer_binding()
    }
    pub fn buffer_ref(&self) -> &Buffer {
        &self.inner
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BufferOpError {
    #[error("the requested size of the buffer was zero")]
    BufferSizeZero,
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

fn range_is_empty(range: &impl RangeBounds<u64>) -> bool {
    let first = match range.start_bound() {
        std::ops::Bound::Included(n) => *n,
        std::ops::Bound::Excluded(n) => *n + 1,
        std::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        std::ops::Bound::Included(n) => *n + 1,
        std::ops::Bound::Excluded(n) => *n,
        std::ops::Bound::Unbounded => u64::MAX,
    };
    end <= first
}

#[derive(Clone)]
pub struct TransformBuffer(Buffer);
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
    pub fn write(&self, queue: &Queue, transform: Affine2) {
        let mut mat4 = Mat4::from_mat3(Mat3::from_mat2(transform.matrix2));
        mat4.w_axis.x = transform.translation.x;
        mat4.w_axis.y = transform.translation.y;
        queue.write_buffer(&self.0, 0, bytemuck::bytes_of(mat4.as_ref()));
    }
    pub fn as_entire_buffer_binding(&self) -> BufferBinding<'_> {
        self.0.as_entire_buffer_binding()
    }
}

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
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[TextureFormat::Rgba8UnormSrgb],
        });
        Self(texture)
    }
    pub fn write(&self, queue: &Queue, color_map: &[egui::Color32; SIZE]) {
        queue.write_texture(
            self.0.as_image_copy(),
            bytemuck::cast_slice(color_map),
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
