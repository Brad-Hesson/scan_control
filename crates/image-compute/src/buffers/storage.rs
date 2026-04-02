use std::{
    marker::PhantomData,
    num::NonZero,
    ops::{Deref, DerefMut, RangeBounds},
};

use bytemuck::{AnyBitPattern, NoUninit};
use wgpu::{
    Buffer, BufferAddress, BufferBinding, BufferDescriptor, BufferUsages, BufferViewMut, Device,
    Queue, QueueWriteBufferView,
};

use crate::buffers::{BufferOpError, assert_gpu_buffer_align};

#[derive(Clone)]
#[repr(transparent)]
pub struct StorageBuffer<T> {
    inner: Buffer,
    pd: PhantomData<T>,
}
impl<T: NoUninit + AnyBitPattern> StorageBuffer<T> {
    pub fn new_uninit(
        device: &Device,
        label: Option<&str>,
        usage: BufferUsages,
        size: usize,
    ) -> Self {
        let inner = device.create_buffer(&BufferDescriptor {
            label,
            size: (size * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: false,
        });
        Self {
            inner,
            pd: PhantomData,
        }
    }
    pub fn new(device: &Device, label: Option<&str>, usage: BufferUsages, init: &[T]) -> Self {
        const { assert_gpu_buffer_align::<T>() }
        let inner = device.create_buffer(&BufferDescriptor {
            label,
            size: (init.len() * std::mem::size_of::<T>()) as u64,
            usage,
            mapped_at_creation: true,
        });
        {
            let mut view = inner.get_mapped_range_mut(..);
            bytemuck::cast_slice_mut(view.as_mut()).copy_from_slice(init);
        }
        inner.unmap();
        Self {
            inner,
            pd: PhantomData,
        }
    }
    pub fn new_with(
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
    pub fn queue_write_with<'s>(
        &'s self,
        queue: &'s Queue,
        offset: usize,
        size: usize,
    ) -> Result<QueueWriteStorageBufferView<'s, T>, BufferOpError> {
        const { assert_gpu_buffer_align::<T>() }
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
    ) -> Result<(), BufferOpError> {
        const { assert_gpu_buffer_align::<T>() }
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
pub struct StorageBufferUninit<T> {
    buffer: StorageBuffer<T>,
}
impl<T> StorageBufferUninit<T> {
    pub fn view_mut(&mut self) -> StorageBufferViewMut<'_, T> {
        const { assert_gpu_buffer_align::<T>() }
        StorageBufferViewMut {
            inner: self.buffer.inner.get_mapped_range_mut(..),
            phantom: PhantomData,
        }
    }
    pub fn finish(self) -> StorageBuffer<T> {
        self.buffer.inner.unmap();
        self.buffer
    }
}

pub struct StorageBufferViewMut<'a, T> {
    inner: BufferViewMut<'a>,
    phantom: PhantomData<T>,
}
impl<'a, T: NoUninit + AnyBitPattern> Deref for StorageBufferViewMut<'a, T> {
    type Target = [T];

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        bytemuck::cast_slice(self.inner.as_ref())
    }
}
impl<'a, T: NoUninit + AnyBitPattern> DerefMut for StorageBufferViewMut<'a, T> {
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Self::Target {
        let bytes = self.inner.as_mut();
        bytemuck::cast_slice_mut(bytes)
    }
}
pub struct QueueWriteStorageBufferView<'a, T> {
    inner: QueueWriteBufferView<'a>,
    phantom: PhantomData<T>,
}
impl<'a, T: NoUninit + AnyBitPattern> Deref for QueueWriteStorageBufferView<'a, T> {
    type Target = [T];

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        bytemuck::cast_slice(self.inner.as_ref())
    }
}
impl<'a, T: NoUninit + AnyBitPattern> DerefMut for QueueWriteStorageBufferView<'a, T> {
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Self::Target {
        bytemuck::cast_slice_mut(self.inner.as_mut())
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
