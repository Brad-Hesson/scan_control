#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::{glam::UVec3, spirv};

/// Output written by one invocation of [`hello_world`].
///
/// This type is shared with the host crate, so its layout must remain compatible
/// with the corresponding storage buffer element.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "spirv"), derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct HelloWorldOutput {
    pub value: u32,
}

impl HelloWorldOutput {
    const fn from_index(index: u32) -> Self {
        Self { value: index + 1 }
    }
}

/// Minimal compute shader used to verify the rust-gpu build pipeline.
///
/// Bind a writable [`HelloWorldOutput`] storage buffer at set 0, binding 0.
/// Each invocation writes a recognizable value into its corresponding output
/// element.
#[spirv(compute(threads(64)))]
pub fn hello_world(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 0)] output: &mut [HelloWorldOutput],
) {
    output[id.x as usize] = HelloWorldOutput::from_index(id.x);
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use super::HelloWorldOutput;

    #[test]
    fn hello_world_output_has_u32_layout() {
        assert_eq!(size_of::<HelloWorldOutput>(), size_of::<u32>());
        assert_eq!(align_of::<HelloWorldOutput>(), align_of::<u32>());

        let output = HelloWorldOutput { value: 7 };
        assert_eq!(bytemuck::bytes_of(&output), 7_u32.to_ne_bytes());
    }
}
