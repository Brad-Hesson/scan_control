mod color_map;
mod storage;
mod transform;

pub use color_map::ColorMapTexture;
pub use storage::StorageBuffer;
pub use transform::TransformBuffer;

#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, packed)]
pub struct Unalign<T>(T);
impl<T> Unalign<T> {
    pub fn set(&mut self, val: T) {
        *self = Self(val);
    }
    pub fn get(&self) -> T
    where
        T: Copy,
    {
        let Self(val) = *self;
        val
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BufferOpError {
    #[error("the requested size of the buffer was zero")]
    BufferSizeZero,
}

#[track_caller]
const fn assert_gpu_buffer_align<T>() {
    assert!(
        align_of::<T>() <= 4,
        "Can only map storage buffers of type with 4 byte alignment or less"
    )
}
