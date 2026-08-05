use spirv_std::{
    Sampler,
    glam::{DVec2, DVec3, Vec2, Vec4},
    image::Image2d,
    spirv,
};

use crate::scan_image::Mat3x3F64;

const VERTS: [DVec2; 4] = [
    DVec2::new(-0.5, -0.5),
    DVec2::new(0.5, -0.5),
    DVec2::new(-0.5, 0.5),
    DVec2::new(0.5, 0.5),
];
const UVS: [Vec2; 4] = [
    Vec2::new(0.0, 1.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
];

#[spirv(vertex)]
pub fn vs_main(
    #[spirv(vertex_index)] vert_index: u32,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] world2screen: &Mat3x3F64,
    #[spirv(uniform, descriptor_set = 1, binding = 0)] quad2world: &Mat3x3F64,
    #[spirv(location = 0)] out_uv: &mut Vec2,
    #[spirv(position)] out_position: &mut Vec4,
) {
    let vertex = VERTS[vert_index as usize];
    let position = world2screen.mul_vec3(quad2world.mul_vec3(DVec3::new(vertex.x, vertex.y, 1.0)));
    *out_position = Vec4::new(position.x as f32, position.y as f32, 0.0, position.z as f32);
    *out_uv = UVS[vert_index as usize];
}

#[spirv(fragment)]
pub fn fs_main(
    #[spirv(location = 0)] uv: Vec2,
    #[spirv(descriptor_set = 0, binding = 1)] tex_sampler: &Sampler,
    #[spirv(descriptor_set = 1, binding = 1)] image_tex: &Image2d,
    #[spirv(location = 0)] out_color: &mut Vec4,
) {
    *out_color = image_tex.sample(*tex_sampler, uv);
}
