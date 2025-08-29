@group(0) @binding(0)
var<uniform> image_size: vec2<u32>;
@group(0) @binding(1)
var<storage, read> image_in: array<f32>;

@group(1) @binding(0)
var<storage, read_write> image_out: array<f32>;
@group(1) @binding(1)
var<storage, read_write> meta_out: array<f64>;

@group(2) @binding(0)
var<storage, read_write> xz: array<f64>;
@group(2) @binding(1)
var<storage, read_write> yz: array<f64>;

const WGS: u32 = 256u;
const WGS_SQUARE: u32 = 16u;

@compute @workgroup_size(WGS)
fn copy_image(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }
    meta_out[i] = f64(image_in[i]);
}

@compute @workgroup_size(WGS)
fn copy_image_transpose(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if global_id.x >= image_len() { return; }
    let x = global_id.x % image_size.x;
    let y = global_id.x / image_size.x;
    meta_out[x * image_size.y + y] = f64(image_in[y * image_size.x + x]);
}

var<workgroup> z_sum_wg: array<f64, WGS>;
@compute @workgroup_size(WGS)
fn reduce_image(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let read_idx = num_workgroups.x * local_index + workgroup_id.x;
    if read_idx < image_len() {
        z_sum_wg[local_index] = meta_out[read_idx];
    } else {
        z_sum_wg[local_index] = 0.;
    }
    var stride = WGS >> 1u;
    while stride > 0u {
        if local_index >= stride {break;}
        workgroupBarrier();
        z_sum_wg[local_index] += z_sum_wg[local_index + stride];
        stride >>= 1u;
    }
    if local_index == 0u {
        meta_out[workgroup_id.x] = z_sum_wg[0];
    } else {
        meta_out[read_idx] = 0.;
    }
}

@compute @workgroup_size(WGS_SQUARE, WGS_SQUARE)
fn reduce_image_lines(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let sz = image_size.yx;
    let local_id = vec2(local_index % WGS_SQUARE, local_index / WGS_SQUARE);
    let col_read_idx = num_workgroups.y * local_id.y + workgroup_id.y;
    if col_read_idx < sz.y && global_id.x < sz.x {
        z_sum_wg[local_index] = meta_out[idx(sz, global_id.x, col_read_idx)];
    } else {
        z_sum_wg[local_index] = 0.;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_id.y >= stride {break;}
        workgroupBarrier();
        z_sum_wg[local_index] += z_sum_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_id.y == 0u {
        meta_out[idx(sz, global_id.x, workgroup_id.y)] = z_sum_wg[local_id.x];
    } else {
        meta_out[idx(sz, global_id.x, col_read_idx)] = 0.;
    }
}

@compute @workgroup_size(256)
fn generate_sums_plane(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }
    let basis = calc_basis(i);
    xz[i] = basis.x * basis.z;
    yz[i] = basis.y * basis.z;
}

@compute @workgroup_size(256)
fn generate_sums_lines(@builtin(global_invocation_id) global_index: vec3<u32>) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let global_id = vec2(global_index.x % image_size.x, global_index.x / image_size.x);
    let mean = meta_out[global_id.y];
    let val = (f64(image_in[i]) - mean) * mean_center(image_size.x, global_id.x);
    xz[idx(image_size.yx, global_id.y, global_id.x)] = val;
}

var<workgroup> xz_sum_wg: array<f64, WGS>;
var<workgroup> yz_sum_wg: array<f64, WGS>;
@compute @workgroup_size(WGS)
fn reduce_sums_plane(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let read_idx = num_workgroups.x * local_index + workgroup_id.x;
    if read_idx < image_len() {
        xz_sum_wg[local_index] = xz[read_idx];
        yz_sum_wg[local_index] = yz[read_idx];
    } else {
        xz_sum_wg[local_index] = 0.;
        yz_sum_wg[local_index] = 0.;
    }
    var stride = WGS >> 1u;
    while stride > 0u {
        if local_index >= stride { break; }
        workgroupBarrier();
        xz_sum_wg[local_index] += xz_sum_wg[local_index + stride];
        yz_sum_wg[local_index] += yz_sum_wg[local_index + stride];
        stride >>= 1u;
    }
    if local_index == 0u {
        xz[workgroup_id.x] = xz_sum_wg[0];
        yz[workgroup_id.x] = yz_sum_wg[0];
    } else {
        xz[read_idx] = 0.;
        yz[read_idx] = 0.;
    }
}

@compute @workgroup_size(WGS_SQUARE, WGS_SQUARE)
fn reduce_sums_lines(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let sz = image_size.yx;
    let local_id = vec2(local_index % WGS_SQUARE, local_index / WGS_SQUARE);
    let col_read_idx = num_workgroups.y * local_id.y + workgroup_id.y;
    if col_read_idx < sz.y && global_id.x < sz.x {
        xz_sum_wg[local_index] = xz[idx(sz, global_id.x, col_read_idx)];
    } else {
        xz_sum_wg[local_index] = 0.;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_id.y >= stride {break;}
        workgroupBarrier();
        xz_sum_wg[local_index] += xz_sum_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_id.y == 0u {
        xz[idx(sz, global_id.x, workgroup_id.y)] = xz_sum_wg[local_id.x];
    } else {
        xz[idx(sz, global_id.x, col_read_idx)] = 0.;
    }
}

var<workgroup> x_slope: f64;
var<workgroup> y_slope: f64;
@compute @workgroup_size(256)
fn subtract_plane(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(global_invocation_id) global_id: vec3<u32>
) {
    let i = global_id.x;
    if i >= image_len() { return; }
    if local_index == 0u {
        let sums = axis_sums();
        let s_xz = xz[0];
        let s_yz = yz[0];
        let s_xx = sums.x;
        let s_yy = sums.y;
        x_slope = s_xz / s_xx;
        y_slope = s_yz / s_yy;
    }
    workgroupBarrier();
    let basis = calc_basis(i);
    var plane = x_slope * basis.x + y_slope * basis.y;
    image_out[i] = f32(basis.z - plane);
    if i == 0u {
        meta_out[1] = x_slope;
        meta_out[2] = y_slope;
    }
}

@compute @workgroup_size(256)
fn subtract_lines(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let global_id = vec2(global_index.x % image_size.x, global_index.x / image_size.x);
    let mean = meta_out[global_id.y] / f64(image_size.x);
    let slope = xz[global_id.y] / axis_sum();
    image_out[i] = f32(f64(image_in[i]) - mean - slope * mean_center(image_size.x, global_id.x));
    if global_id.x == 0u {
        meta_out[image_size.y + global_id.y] = slope;
    }
}

fn image_len() -> u32 {
    return image_size.x * image_size.y;
}
fn calc_basis(i: u32) -> vec3<f64> {
    let w = image_size.x;
    let h = image_size.y;
    let x = i % w;
    let y = i / w;
    let count = f64(image_size.x * image_size.y);
    return vec3(
        mean_center(w, x),
        mean_center(h, y),
        f64(image_in[i]) - meta_out[0] / count
    );
}
fn mean_center(w: u32, x: u32) -> f64 {
    return f64(i32(x << 1u) - i32(w) + i32(1u)) / 2;
}
fn axis_sums() -> vec2<f64> {
    let w = f64(image_size.x);
    let h = f64(image_size.y);
    let tmp = h * w / f64(12);
    return vec2(
        tmp * (w * w - 1),
        tmp * (h * h - 1)
    );
}
fn axis_sum() -> f64 {
    let w = f64(image_size.x);
    let tmp = w / f64(12);
    return tmp * (w * w - 1);
}
fn idx(sz: vec2<u32>, x: u32, y: u32) -> u32 {
    return x + y * sz.x;
}