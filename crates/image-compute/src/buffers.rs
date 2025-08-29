use std::{
    fmt::Debug,
    marker::PhantomData,
    ops::RangeBounds,
    sync::{Arc, OnceLock},
};

use bytemuck::{AnyBitPattern, NoUninit};
use wgpu::{Buffer, BufferAddress, BufferDescriptor, BufferUsages, Device, Queue};

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
