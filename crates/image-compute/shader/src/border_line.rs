use spirv_std::{
    glam::{DVec3, Vec2, Vec3, Vec4},
    spirv,
};

use crate::scan_image::Mat3x3F64;

#[spirv(vertex)]
pub fn vs_main(
    #[spirv(location = 0)] vert: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] world2screen: &Mat3x3F64,
    #[spirv(uniform, descriptor_set = 1, binding = 0)] quad2world: &Mat3x3F64,
    #[spirv(position)] out_position: &mut Vec4,
) {
    let position =
        world2screen.mul_vec3(quad2world.mul_vec3(DVec3::new(vert.x as f64, vert.y as f64, 1.0)));
    *out_position = Vec4::new(position.x as f32, position.y as f32, 0.0, position.z as f32);
}

#[spirv(fragment)]
pub fn fs_main(
    #[spirv(uniform, descriptor_set = 1, binding = 1)] border_color: &Vec3,
    #[spirv(location = 0)] out_color: &mut Vec4,
) {
    *out_color = border_color.extend(1.0);
}
