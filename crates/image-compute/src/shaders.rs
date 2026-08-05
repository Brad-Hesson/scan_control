pub(crate) use image_compute_shader::HelloWorldOutput;

const _: () = assert!(
    std::mem::size_of::<HelloWorldOutput>() == std::mem::size_of::<u32>(),
    "hello_world output must remain one u32"
);

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

pub mod rust_gpu {
    #![allow(dead_code, non_snake_case, unused_imports)]
    include!(concat!(env!("OUT_DIR"), "/rust_gpu.rs"));
}