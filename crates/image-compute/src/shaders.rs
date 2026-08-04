pub mod plane_fit {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/plane_fit.rs"));
}
pub mod scan_image {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/scan_image.rs"));
}
pub mod file_image {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/file_image.rs"));
}

pub mod border_line {
    #![allow(dead_code, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/border_line.rs"));
}
/// A minimal rust-gpu compute shader, compiled to SPIR-V by `build.rs`.
///
/// The entry point is `hello_world`. It writes the one-based invocation index
/// into binding 0. Dispatch no more invocations than the buffer has elements.
pub const HELLO_WORLD_SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello_world.spv"));
