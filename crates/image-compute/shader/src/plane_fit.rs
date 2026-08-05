#![allow(non_snake_case)]

#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float as _;
use spirv_std::{
    arch::workgroup_memory_barrier_with_group_sync,
    glam::{UVec2, UVec3, Vec3},
    spirv,
};

use crate::scan_image::NormalizeData;

pub const WGS: u32 = 256;
pub const WGS_SQUARE: u32 = 16;
type R32FloatImage = spirv_std::Image!(2D, format = r32f, sampled = false);

#[inline]
fn image_len(size: UVec2) -> u32 {
    size.x * size.y
}
#[inline]
fn norm_pos(w: u32, x: u32) -> f32 {
    x as f32 / (w - 1) as f32
}
#[inline]
fn index_transpose(size: UVec2, i: u32) -> usize {
    ((i % size.x) * size.y + i / size.x) as usize
}
#[inline]
fn idx(size: UVec2, x: u32, y: u32) -> usize {
    (x + y * size.x) as usize
}
#[inline]
fn nan(seed: u32) -> f32 {
    f32::from_bits(0x7fc0_0000 | (seed & 1))
}
#[inline]
fn positive_infinity(seed: u32) -> f32 {
    let value = seed as f32;
    1.0 / (value - value)
}
#[inline]
fn negative_infinity(seed: u32) -> f32 {
    -positive_infinity(seed)
}

#[inline]
fn calc_basis(
    size: UVec2,
    i: u32,
    image: &[f32],
    planar: &[f32],
    xs: &[f32],
    ys: &[f32],
    counts: &[u32],
) -> Vec3 {
    let n = counts[0] as f32;
    Vec3::new(
        norm_pos(size.x, i % size.x) - xs[0] / n,
        norm_pos(size.y, i / size.x) - ys[0] / n,
        image[i as usize] - planar[0] / n,
    )
}

#[inline]
fn calc_basis_lines(
    size: UVec2,
    i: u32,
    image: &[f32],
    planar: &[f32],
    xs: &[f32],
    counts: &[u32],
) -> Vec3 {
    let row = (i / size.x) as usize;
    let n = counts[row] as f32;
    Vec3::new(
        norm_pos(size.x, i % size.x) - xs[row] / n,
        0.0,
        image[i as usize] - planar[row] / n,
    )
}

#[spirv(compute(threads(256)))]
pub fn copy_image(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] image_size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image_in: &[f32],
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planarize_out: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] x_sum: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 1)] y_sum: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] count: &mut [u32],
) {
    let i = id.x as usize;
    if id.x >= image_len(*image_size) {
        return;
    }
    if image_in[i].is_nan() {
        planarize_out[i] = 0.0;
        x_sum[i] = 0.0;
        y_sum[i] = 0.0;
        count[i] = 0;
    } else {
        planarize_out[i] = image_in[i];
        x_sum[i] = norm_pos(image_size.x, id.x % image_size.x);
        y_sum[i] = norm_pos(image_size.y, id.x / image_size.x);
        count[i] = 1;
    }
}

#[spirv(compute(threads(256)))]
pub fn copy_image_transpose(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] image_size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image_in: &[f32],
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planarize_out: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] x_sum: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] count: &mut [u32],
) {
    if id.x >= image_len(*image_size) {
        return;
    }
    let src = id.x as usize;
    let dst = index_transpose(*image_size, id.x);
    if image_in[src].is_nan() {
        planarize_out[dst] = 0.0;
        x_sum[dst] = 0.0;
        count[dst] = 0;
    } else {
        planarize_out[dst] = image_in[src];
        x_sum[dst] = norm_pos(image_size.x, id.x % image_size.x);
        count[dst] = 1;
    }
}

#[spirv(compute(threads(256)))]
pub fn reduce_image(
    #[spirv(local_invocation_index)] local: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(num_workgroups)] groups: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] image_size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 1)] ys: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(workgroup)] z_wg: &mut [f32; 256],
    #[spirv(workgroup)] x_wg: &mut [f32; 256],
    #[spirv(workgroup)] y_wg: &mut [f32; 256],
    #[spirv(workgroup)] c_wg: &mut [u32; 256],
) {
    let read = groups.x * local + group.x;
    let l = local as usize;
    if read < image_len(*image_size) {
        let r = read as usize;
        z_wg[l] = planar[r];
        x_wg[l] = xs[r];
        y_wg[l] = ys[r];
        c_wg[l] = counts[r];
    } else {
        z_wg[l] = 0.0;
        x_wg[l] = 0.0;
        y_wg[l] = 0.0;
        c_wg[l] = 0;
    }
    let mut stride = 128;
    while stride > 0 {
        workgroup_memory_barrier_with_group_sync();
        if local < stride {
            let s = (local + stride) as usize;
            z_wg[l] += z_wg[s];
            x_wg[l] += x_wg[s];
            y_wg[l] += y_wg[s];
            c_wg[l] += c_wg[s];
        }
        stride >>= 1;
    }
    if read < image_len(*image_size) {
        let r = read as usize;
        if local == 0 {
            planar[r] = z_wg[0];
            xs[r] = x_wg[0];
            ys[r] = y_wg[0];
            counts[r] = c_wg[0];
        } else {
            planar[r] = 0.0;
            xs[r] = 0.0;
            ys[r] = 0.0;
            counts[r] = 0;
        }
    }
}

#[spirv(compute(threads(16, 16)))]
pub fn reduce_image_lines(
    #[spirv(global_invocation_id)] global: UVec3,
    #[spirv(local_invocation_index)] local: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(num_workgroups)] groups: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] image_size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(workgroup)] z_wg: &mut [f32; 256],
    #[spirv(workgroup)] x_wg: &mut [f32; 256],
    #[spirv(workgroup)] c_wg: &mut [u32; 256],
) {
    let size = UVec2::new(image_size.y, image_size.x);
    let lx = local % 16;
    let ly = local / 16;
    let col = groups.y * ly + group.y;
    let read = idx(size, global.x, col);
    let l = local as usize;
    let valid = global.x < size.x && col < size.y;
    if valid {
        z_wg[l] = planar[read];
        x_wg[l] = xs[read];
        c_wg[l] = counts[read];
    } else {
        z_wg[l] = 0.0;
        x_wg[l] = 0.0;
        c_wg[l] = 0;
    }
    let mut stride = 8;
    while stride > 0 {
        workgroup_memory_barrier_with_group_sync();
        if ly < stride {
            let s = (local + stride * 16) as usize;
            z_wg[l] += z_wg[s];
            x_wg[l] += x_wg[s];
            c_wg[l] += c_wg[s];
        }
        stride >>= 1;
    }
    if valid {
        if ly == 0 {
            planar[read] = z_wg[lx as usize];
            xs[read] = x_wg[lx as usize];
            counts[read] = c_wg[lx as usize];
        } else {
            planar[read] = 0.0;
            xs[read] = 0.0;
            counts[read] = 0;
        }
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_sums_plane(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 1)] ys: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 1)] yz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 3)] yy: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let i = id.x as usize;
    let b = calc_basis(*size, id.x, image, planar, xs, ys, counts);
    if b.z.is_nan() {
        xz[i] = 0.0;
        yz[i] = 0.0;
        xx[i] = 0.0;
        yy[i] = 0.0
    } else {
        xz[i] = b.x * b.z;
        yz[i] = b.y * b.z;
        xx[i] = b.x * b.x;
        yy[i] = b.y * b.y;
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_sums_lines(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let i = index_transpose(*size, id.x);
    let b = calc_basis_lines(*size, id.x, image, planar, xs, counts);
    if b.z.is_nan() {
        xz[i] = 0.0;
        xx[i] = 0.0
    } else {
        xz[i] = b.x * b.z;
        xx[i] = b.x * b.x;
    }
}

#[spirv(compute(threads(256)))]
pub fn reduce_sums_plane(
    #[spirv(local_invocation_index)] local: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(num_workgroups)] groups: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 1)] yz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 3)] yy: &mut [f32],
    #[spirv(workgroup)] xz_wg: &mut [f32; 256],
    #[spirv(workgroup)] yz_wg: &mut [f32; 256],
    #[spirv(workgroup)] xx_wg: &mut [f32; 256],
    #[spirv(workgroup)] yy_wg: &mut [f32; 256],
) {
    let read = groups.x * local + group.x;
    let l = local as usize;
    let valid = read < image_len(*size);
    if valid {
        let r = read as usize;
        xz_wg[l] = xz[r];
        yz_wg[l] = yz[r];
        xx_wg[l] = xx[r];
        yy_wg[l] = yy[r];
    } else {
        xz_wg[l] = 0.0;
        yz_wg[l] = 0.0;
        xx_wg[l] = 0.0;
        yy_wg[l] = 0.0
    }
    let mut s = 128;
    while s > 0 {
        workgroup_memory_barrier_with_group_sync();
        if local < s {
            let j = (local + s) as usize;
            xz_wg[l] += xz_wg[j];
            yz_wg[l] += yz_wg[j];
            xx_wg[l] += xx_wg[j];
            yy_wg[l] += yy_wg[j];
        }
        s >>= 1;
    }
    if valid {
        let r = read as usize;
        if local == 0 {
            xz[r] = xz_wg[0];
            yz[r] = yz_wg[0];
            xx[r] = xx_wg[0];
            yy[r] = yy_wg[0]
        } else {
            xz[r] = 0.0;
            yz[r] = 0.0;
            xx[r] = 0.0;
            yy[r] = 0.0
        }
    }
}

#[spirv(compute(threads(16, 16)))]
pub fn reduce_sums_lines(
    #[spirv(global_invocation_id)] global: UVec3,
    #[spirv(local_invocation_index)] local: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(num_workgroups)] groups: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] image_size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
    #[spirv(workgroup)] xz_wg: &mut [f32; 256],
    #[spirv(workgroup)] xx_wg: &mut [f32; 256],
) {
    let size = UVec2::new(image_size.y, image_size.x);
    let row = local / 16;
    let col = local % 16;
    let src_col = groups.y * row + group.y;
    let read = idx(size, global.x, src_col);
    let l = local as usize;
    let valid = global.x < size.x && src_col < size.y;
    if valid {
        xz_wg[l] = xz[read];
        xx_wg[l] = xx[read]
    } else {
        xz_wg[l] = 0.0;
        xx_wg[l] = 0.0
    }
    let mut s = 8;
    while s > 0 {
        workgroup_memory_barrier_with_group_sync();
        if row < s {
            let j = (local + s * 16) as usize;
            xz_wg[l] += xz_wg[j];
            xx_wg[l] += xx_wg[j]
        }
        s >>= 1
    }
    if valid {
        if row == 0 {
            xz[read] = xz_wg[col as usize];
            xx[read] = xx_wg[col as usize]
        } else {
            xz[read] = 0.0;
            xx[read] = 0.0
        }
    }
}

#[inline]
fn store_value(
    i: u32,
    size: UVec2,
    value: f32,
    texture: &R32FloatImage,
    mins: &mut [f32],
    maxs: &mut [f32],
    devs: &mut [f32],
) {
    unsafe { texture.write(UVec2::new(i % size.x, i / size.x), value) }
    let n = i as usize;
    if value.is_nan() {
        mins[n] = positive_infinity(i);
        maxs[n] = negative_infinity(i);
        devs[n] = 0.0
    } else {
        mins[n] = value;
        maxs[n] = value;
        devs[n] = value * value
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_normalization__mean_subtract(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(descriptor_set = 1, binding = 0)] texture: &R32FloatImage,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 1)] ys: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 3)] mins: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 4)] maxs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 5)] devs: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let b = calc_basis(*size, id.x, image, planar, xs, ys, counts);
    store_value(id.x, *size, b.z, texture, mins, maxs, devs);
    if id.x == 0 {
        planar[1] = counts[0] as f32
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_normalization__plane_fit(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(descriptor_set = 1, binding = 0)] texture: &R32FloatImage,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 1)] ys: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 3)] mins: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 4)] maxs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 5)] devs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 1)] yz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 3)] yy: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let b = calc_basis(*size, id.x, image, planar, xs, ys, counts);
    let sx = xz[0] / xx[0];
    let sy = if yy[0] == 0.0 { 0.0 } else { yz[0] / yy[0] };
    store_value(
        id.x,
        *size,
        b.z - sx * b.x - sy * b.y,
        texture,
        mins,
        maxs,
        devs,
    );
    if id.x == 0 {
        planar[1] = counts[0] as f32;
        planar[2] = sx;
        planar[3] = sy
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_normalization__line_fit(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(descriptor_set = 1, binding = 0)] texture: &R32FloatImage,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 3)] mins: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 4)] maxs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 5)] devs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 0)] xz: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 3, binding = 2)] xx: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let row = (id.x / size.x) as usize;
    let col = id.x % size.x;
    let b = calc_basis_lines(*size, id.x, image, planar, xs, counts);
    let slope = xz[row] / xx[row];
    store_value(id.x, *size, b.z - slope * b.x, texture, mins, maxs, devs);
    if col == 0 {
        planar[size.y as usize + row] = counts[row] as f32;
        planar[2 * size.y as usize + row] = slope
    }
}

#[spirv(compute(threads(256)))]
pub fn generate_normalization__line_mean(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] image: &[f32],
    #[spirv(descriptor_set = 1, binding = 0)] texture: &R32FloatImage,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 1)] planar: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 0)] xs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 3)] mins: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 4)] maxs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 5)] devs: &mut [f32],
) {
    if id.x >= image_len(*size) {
        return;
    }
    let row = (id.x / size.x) as usize;
    let b = calc_basis_lines(*size, id.x, image, planar, xs, counts);
    store_value(id.x, *size, b.z, texture, mins, maxs, devs);
    if id.x % size.x == 0 {
        planar[size.y as usize + row] = counts[row] as f32
    }
}

#[spirv(compute(threads(256)))]
pub fn reduce_normalizations(
    #[spirv(global_invocation_id)] global: UVec3,
    #[spirv(local_invocation_index)] local: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(num_workgroups)] groups: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 2)] normalize: &mut NormalizeData,
    #[spirv(storage_buffer, descriptor_set = 2, binding = 2)] counts: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 3)] mins: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 4)] maxs: &mut [f32],
    #[spirv(storage_buffer, descriptor_set = 2, binding = 5)] devs: &mut [f32],
    #[spirv(workgroup)] min_wg: &mut [f32; 256],
    #[spirv(workgroup)] max_wg: &mut [f32; 256],
    #[spirv(workgroup)] dev_wg: &mut [f32; 256],
    #[spirv(workgroup)] count_wg: &mut [u32; 256],
) {
    let read = groups.x * local + group.x;
    let l = local as usize;
    let valid = read < image_len(*size);
    if valid {
        let r = read as usize;
        min_wg[l] = mins[r];
        max_wg[l] = maxs[r];
        dev_wg[l] = devs[r];
        count_wg[l] = counts[r]
    } else {
        min_wg[l] = positive_infinity(read);
        max_wg[l] = negative_infinity(read);
        dev_wg[l] = 0.0;
        count_wg[l] = 0
    }
    let mut s = 128;
    while s > 0 {
        workgroup_memory_barrier_with_group_sync();
        if local < s {
            let j = (local + s) as usize;
            min_wg[l] = min_wg[l].min(min_wg[j]);
            max_wg[l] = max_wg[l].max(max_wg[j]);
            dev_wg[l] += dev_wg[j];
            count_wg[l] += count_wg[j]
        }
        s >>= 1
    }
    if valid {
        let r = read as usize;
        if local == 0 {
            mins[r] = min_wg[0];
            maxs[r] = max_wg[0];
            devs[r] = dev_wg[0];
            counts[r] = count_wg[0]
        } else {
            mins[r] = positive_infinity(read);
            maxs[r] = negative_infinity(read);
            devs[r] = 0.0;
            counts[r] = 0
        }
    }
    if global.x == 0 {
        normalize.min = mins[0];
        normalize.max = maxs[0];
        normalize.stddev = (devs[0] / counts[0] as f32).sqrt()
    }
}

#[spirv(compute(threads(256)))]
pub fn clear_texture(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] size: &UVec2,
    #[spirv(descriptor_set = 1, binding = 0)] texture: &R32FloatImage,
    #[spirv(storage_buffer, descriptor_set = 1, binding = 2)] normalize: &mut NormalizeData,
) {
    if id.x >= image_len(*size) {
        return;
    }
    let nan = nan(id.x);
    unsafe { texture.write(UVec2::new(id.x % size.x, id.x / size.x), nan) }
    if id.x == 0 {
        normalize.min = nan;
        normalize.max = nan;
        normalize.stddev = nan
    }
}
