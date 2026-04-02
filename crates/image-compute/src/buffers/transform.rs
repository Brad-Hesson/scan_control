use glam::{DMat3, DVec3};
use wgpu::{BufferBinding, BufferUsages, Device, Queue};

use crate::buffers::{StorageBuffer, Unalign};

#[derive(Clone)]
pub struct TransformBuffer(StorageBuffer<DVec3Pad>);
impl TransformBuffer {
    pub fn new(device: &Device) -> Self {
        let buffer = StorageBuffer::new_uninit(
            device,
            Some("quad2world uniform"),
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            3,
        );
        Self(buffer)
    }
    pub fn queue_write(&self, queue: &Queue, transform: impl Into<DMat3>) {
        let mat3 = transform.into();
        let mut buf = self.0.queue_write_with(queue, 0, 3).expect("size is known");
        buf[0].set(mat3.x_axis);
        buf[1].set(mat3.y_axis);
        buf[2].set(mat3.z_axis);
    }
    pub fn as_entire_buffer_binding(&self) -> BufferBinding<'_> {
        self.0.as_entire_buffer_binding()
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct DVec3Pad {
    vec: Unalign<DVec3>,
    _pad: [u8; size_of::<f64>()],
}
impl DVec3Pad {
    fn set(&mut self, vec: DVec3) {
        self.vec.set(vec);
    }
}
