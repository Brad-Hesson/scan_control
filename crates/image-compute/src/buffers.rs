use std::{
    fmt::Debug,
    marker::PhantomData,
    num::NonZero,
    ops::{Deref, DerefMut, RangeBounds},
};

use bytemuck::{AnyBitPattern, NoUninit};
use encase::ShaderSize as _;
use glam::{DMat3, Mat3};
use itertools::iproduct;
use wgpu::{
    Buffer, BufferAddress, BufferBinding, BufferDescriptor, BufferUsages, BufferViewMut, Device,
    Extent3d, Queue, QueueWriteBufferView, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, TryFromBytes, Unalign};

#[derive(Clone)]
#[repr(transparent)]
pub struct StorageBuffer<T> {
    inner: Buffer,
    pd: PhantomData<T>,
}
impl<T: FromBytes + IntoBytes + KnownLayout> StorageBuffer<T> {
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
        init_fn(<[T]>::mut_from_bytes(inner.get_mapped_range_mut(..).as_mut()).unwrap());
        inner.unmap();
        Self {
            inner,
            pd: PhantomData,
        }
    }
    pub fn new_init_handle(
        device: &Device,
        label: Option<&str>,
        usage: BufferUsages,
        size: usize,
    ) -> StorageBufferUninit<T> {
        let inner = device.create_buffer(&BufferDescriptor {
            label,
            size: (size * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: true,
        });
        StorageBufferUninit {
            buffer: Self {
                inner,
                pd: PhantomData,
            },
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
        callback(<[T]>::mut_from_bytes(&mut *view).unwrap());
        Ok(())
    }
    pub fn queue_write_with<'s>(
        &'s self,
        queue: &'s Queue,
        offset: usize,
        size: usize,
    ) -> Result<QueueWriteStorageBufferView<'s, T>, BufferOpError> {
        let view = queue
            .write_buffer_with(
                &self.inner,
                offset as u64 * size_of::<T>() as u64,
                NonZero::try_from(size as u64 * size_of::<T>() as u64)
                    .map_err(|_| BufferOpError::BufferSizeZero)?,
            )
            .expect("`Queue::write_buffer_with` failed");
        Ok(QueueWriteStorageBufferView {
            inner: view,
            phantom: PhantomData,
        })
    }
    pub fn queue_download(
        &self,
        device: &Device,
        queue: &Queue,
        range: impl RangeBounds<usize>,
        callback: impl FnOnce(&[T]) + Send + 'static,
    ) -> Result<(), BufferOpError>
    where
        T: Immutable,
    {
        let range = rangebounds_map(range, |v| (*v * size_of::<T>()) as BufferAddress);
        if range_is_empty(&range) {
            return Err(BufferOpError::BufferSizeZero);
        }
        wgpu::util::DownloadBuffer::read_buffer(
            device,
            queue,
            &self.inner.slice(range),
            move |db| callback(<[T]>::ref_from_bytes(&db.unwrap()).unwrap()),
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

pub struct StorageBufferUninit<T> {
    buffer: StorageBuffer<T>,
}
impl<T: FromBytes + IntoBytes + KnownLayout> StorageBufferUninit<T> {
    pub fn view_mut(&mut self) -> StorageBufferViewMut<'_, T> {
        StorageBufferViewMut {
            inner: self.buffer.inner.get_mapped_range_mut(..),
            phantom: PhantomData,
        }
    }
}
impl<T: FromBytes + IntoBytes + KnownLayout> StorageBufferUninit<T> {
    pub fn finish(self) -> StorageBuffer<T> {
        self.buffer.inner.unmap();
        self.buffer
    }
}
pub struct StorageBufferViewMut<'a, T> {
    inner: BufferViewMut<'a>,
    phantom: PhantomData<T>,
}
impl<'a, T: FromBytes + IntoBytes + KnownLayout + Immutable> Deref for StorageBufferViewMut<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        <[T]>::ref_from_bytes(self.inner.as_ref()).unwrap()
    }
}
impl<'a, T: FromBytes + IntoBytes + KnownLayout + Immutable> DerefMut
    for StorageBufferViewMut<'a, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        let bytes = self.inner.as_mut();
        dbg!(bytes.as_ptr());
        <[T]>::mut_from_bytes(bytes).unwrap()
    }
}
pub struct QueueWriteStorageBufferView<'a, T> {
    inner: QueueWriteBufferView<'a>,
    phantom: PhantomData<T>,
}
impl<'a, T: FromBytes + IntoBytes + KnownLayout + Immutable> Deref
    for QueueWriteStorageBufferView<'a, T>
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        <[T]>::ref_from_bytes(self.inner.as_ref()).unwrap()
    }
}
impl<'a, T: FromBytes + IntoBytes + KnownLayout + Immutable> DerefMut
    for QueueWriteStorageBufferView<'a, T>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        <[T]>::mut_from_bytes(self.inner.as_mut()).unwrap()
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
pub struct TransformBuffer(StorageBuffer<Unalign<f64>>);
impl TransformBuffer {
    pub fn new(device: &Device) -> Self {
        let buffer = StorageBuffer::new(
            device,
            Some("quad2world uniform"),
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            3 * 4,
            |_| {},
        );
        Self(buffer)
    }
    pub fn write(&self, queue: &Queue, transform: impl Into<DMat3>) {
        let mat3 = transform.into();
        let mut buf = self.0.queue_write_with(queue, 0, 3 * 4).unwrap();
        for (x,y) in iproduct!(0..3, 0..3){
            buf[x*4..][y].set(mat3.to_cols_array_2d()[x][y]);
        }
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
