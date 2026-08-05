use spirv_std::{
    Sampler,
    arch::kill,
    glam::{DVec2, DVec3, DVec4, Vec2, Vec3, Vec4},
    image::{Image1d, Image2d},
    spirv,
};

const LOW_COLOR: Vec3 = Vec3::new(0.0, 0.0, 1.0);
const HIGH_COLOR: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const IMAGE_ALPHA: f32 = 1.0;

// ------------ Structs and data ------------

/// Padded representation of a column-major `mat3x3<f64>`.
///
/// Each WGSL/SPIR-V `vec3<f64>` matrix column occupies 32 bytes in a
/// uniform buffer, so the padding component of each DVec4 is unused.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(target_arch = "spirv"), derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Mat3x3F64 {
    pub x_axis: DVec4,
    pub y_axis: DVec4,
    pub z_axis: DVec4,
}

impl Mat3x3F64 {
    #[inline]
    pub(crate) fn mul_vec3(&self, rhs: DVec3) -> DVec3 {
        self.x_axis.truncate() * rhs.x
            + self.y_axis.truncate() * rhs.y
            + self.z_axis.truncate() * rhs.z
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(target_arch = "spirv"), derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct NormalizeControl {
    pub max_min: u32,
    pub _pad: u32,
    pub std_dev_mul: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(target_arch = "spirv"), derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct NormalizeData {
    pub stddev: f32,
    pub min: f32,
    pub max: f32,
}

const _: () = {
    assert!(core::mem::size_of::<Mat3x3F64>() == 96);
    assert!(core::mem::offset_of!(Mat3x3F64, x_axis) == 0);
    assert!(core::mem::offset_of!(Mat3x3F64, y_axis) == 32);
    assert!(core::mem::offset_of!(Mat3x3F64, z_axis) == 64);
    assert!(core::mem::size_of::<NormalizeControl>() == 12);
    assert!(core::mem::offset_of!(NormalizeControl, max_min) == 0);
    assert!(core::mem::offset_of!(NormalizeControl, _pad) == 4);
    assert!(core::mem::offset_of!(NormalizeControl, std_dev_mul) == 8);
    assert!(core::mem::size_of::<NormalizeData>() == 12);
    assert!(core::mem::offset_of!(NormalizeData, stddev) == 0);
    assert!(core::mem::offset_of!(NormalizeData, min) == 4);
    assert!(core::mem::offset_of!(NormalizeData, max) == 8);
};

const VERTS: [DVec2; 4] = [
    DVec2::new(-0.5, -0.5), // TL
    DVec2::new(0.5, -0.5),  // TR
    DVec2::new(-0.5, 0.5),  // BL
    DVec2::new(0.5, 0.5),   // BR
];

const UVS: [Vec2; 4] = [
    Vec2::new(0.0, 0.0), // TL
    Vec2::new(1.0, 0.0), // TR
    Vec2::new(0.0, 1.0), // BL
    Vec2::new(1.0, 1.0), // BR
];

// ------------ Vertex shader ------------

#[spirv(vertex)]
pub fn vs_main(
    #[spirv(vertex_index)] vert_index: u32,

    #[spirv(uniform, descriptor_set = 0, binding = 0)] world2screen: &Mat3x3F64,

    #[spirv(uniform, descriptor_set = 1, binding = 0)] quad2world: &Mat3x3F64,

    #[spirv(location = 0)] out_uv: &mut Vec2,

    #[spirv(position)] out_position: &mut Vec4,
) {
    let index = vert_index as usize;

    let vertex = VERTS[index];
    let position = DVec3::new(vertex.x, vertex.y, 1.0);

    // Equivalent to:
    //
    // world2screen * quad2world * position
    let position = world2screen.mul_vec3(quad2world.mul_vec3(position));

    *out_position = Vec4::new(position.x as f32, position.y as f32, 0.0, position.z as f32);

    *out_uv = UVS[index];
}

// ------------ Fragment shader ------------

#[spirv(fragment)]
pub fn fs_main(
    #[spirv(location = 0)] uv: Vec2,

    #[spirv(descriptor_set = 0, binding = 1)] tex_sampler: &Sampler,

    #[spirv(descriptor_set = 0, binding = 2)] color_map: &Image1d,

    #[spirv(descriptor_set = 1, binding = 1)] height_map: &Image2d,

    #[spirv(uniform, descriptor_set = 1, binding = 2)] normalize_data: &NormalizeData,

    #[spirv(uniform, descriptor_set = 1, binding = 3)] normalize_control: &NormalizeControl,

    #[spirv(location = 0)] out_color: &mut Vec4,
) {
    // Sample the height of this pixel from the height-map texture.
    let raw = height_map.sample(*tex_sampler, uv).x;

    // If the datapoint doesn't exist, discard the fragment.
    if is_nan(raw) {
        kill();
    }

    // Normalize the datapoint based on NormalizeControl.
    let height = if normalize_control.max_min != 0 {
        (raw - normalize_data.min) / (normalize_data.max - normalize_data.min)
    } else {
        let factor = normalize_control.std_dev_mul * normalize_data.stddev * 3.0;

        (raw / factor) + 0.5
    };

    // If the height is out of range, draw the overflow color.
    if height < 0.0 {
        *out_color = LOW_COLOR.extend(IMAGE_ALPHA);
        return;
    }

    if height > 1.0 {
        *out_color = HIGH_COLOR.extend(IMAGE_ALPHA);
        return;
    }

    // Sample the color map.
    let color = color_map.sample(*tex_sampler, height);

    *out_color = color.truncate().extend(IMAGE_ALPHA);
}

#[inline]
fn is_nan(value: f32) -> bool {
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 0xff;
    let mantissa = bits & 0x007f_ffff;

    exponent == 0xff && mantissa != 0
}
