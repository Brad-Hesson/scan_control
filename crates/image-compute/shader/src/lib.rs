#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::{glam::UVec3, spirv};

fn hello_value(index: u32) -> u32 {
    index + 1
}

/// Minimal compute shader used to verify the rust-gpu build pipeline.
///
/// Bind a writable `u32` storage buffer at set 0, binding 0. Each invocation
/// writes a recognizable value into its corresponding output element.
#[spirv(compute(threads(64)))]
pub fn hello_world(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 0)] output: &mut [u32],
) {
    output[id.x as usize] = hello_value(id.x);
}
