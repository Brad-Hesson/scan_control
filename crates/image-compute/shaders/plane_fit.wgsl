@group(0) @binding(0)
var<uniform> image_size: vec2<u32>;
@group(0) @binding(1)
var<storage, read> image_in: array<f32>;

@group(1) @binding(0)
var texture_out: texture_storage_2d<r32float, write>;
@group(1) @binding(1)
var<storage, read_write> planarize_out: array<f64>;
@group(1) @binding(2)
var<storage, read_write> normalize_out: NormalizeData;

@group(2) @binding(0)
var<storage, read_write> count: array<u32>;
@group(2) @binding(1)
var<storage, read_write> mins: array<f64>;
@group(2) @binding(2)
var<storage, read_write> maxs: array<f64>;
@group(2) @binding(3)
var<storage, read_write> std_devs: array<f64>;
@group(2) @binding(4)
var<storage, read_write> xz: array<f64>;
@group(2) @binding(5)
var<storage, read_write> yz: array<f64>;
@group(2) @binding(6)
var<storage, read_write> xx: array<f64>;
@group(2) @binding(7)
var<storage, read_write> yy: array<f64>;

struct NormalizeData {
    stddev: f64,
    min: f64,
    max: f64,
}

const WGS: u32 = 256u;
const WGS_SQUARE: u32 = 16u;

@compute @workgroup_size(WGS)
fn copy_image(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    if is_nan_f32(image_in[i]){
        planarize_out[i] = 0.;
        count[i] = 0u;
    } else {
        planarize_out[i] = f64(image_in[i]);
        count[i] = 1u;
    }
}

@compute @workgroup_size(WGS)
fn copy_image_transpose(@builtin(global_invocation_id) global_index: vec3<u32>) {
    let i = global_index.x;
    let i_t = index_transpose(i);
    if i >= image_len() { return; }

    if is_nan_f32(image_in[i]){
        planarize_out[i_t] = 0.;
        count[i_t] = 0u;
    } else {
        planarize_out[i_t] = f64(image_in[i]);
        count[i_t] = 1u;
    }
}

var<workgroup> z_sum_wg: array<f64, WGS>;
var<workgroup> z_count_wg: array<u32, WGS>;
@compute @workgroup_size(WGS)
fn reduce_image(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let read_idx = num_workgroups.x * local_index + workgroup_id.x;
    if read_idx < image_len() {
        z_sum_wg[local_index] = planarize_out[read_idx];
        z_count_wg[local_index] = count[read_idx];
    } else {
        z_sum_wg[local_index] = 0.;
        z_count_wg[local_index] = 0u;
        return;
    }
    var stride = WGS >> 1u;
    while stride > 0u {
        if local_index >= stride {break;}
        workgroupBarrier();
        z_sum_wg[local_index] += z_sum_wg[local_index + stride];
        z_count_wg[local_index] += z_count_wg[local_index + stride];
        stride >>= 1u;
    }
    if local_index == 0u {
        planarize_out[read_idx] = z_sum_wg[0];
        count[read_idx] = z_count_wg[0];
    } else {
        count[read_idx] = 0u;
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
    if global_id.x >= sz.x { return; }

    let local_id = vec2(local_index % WGS_SQUARE, local_index / WGS_SQUARE);
    let col_read_idx = num_workgroups.y * local_id.y + workgroup_id.y;

    if col_read_idx < sz.y {
        z_sum_wg[local_index] = planarize_out[idx(sz, global_id.x, col_read_idx)];
        z_count_wg[local_index] = count[idx(sz, global_id.x, col_read_idx)];
    } else {
        z_sum_wg[local_index] = 0.;
        z_count_wg[local_index] = 0u;
        return;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_id.y >= stride {break;}
        workgroupBarrier();
        z_sum_wg[local_index] += z_sum_wg[local_index + stride * WGS_SQUARE];
        z_count_wg[local_index] += z_count_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_id.y == 0u {
        planarize_out[idx(sz, global_id.x, col_read_idx)] = z_sum_wg[local_id.x];
        count[idx(sz, global_id.x, col_read_idx)] = z_count_wg[local_id.x];
    } else {
        count[idx(sz, global_id.x, col_read_idx)] = 0u;
    }
}

@compute @workgroup_size(WGS)
fn generate_sums_plane(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    let basis = calc_basis(i);
    if is_nan_f32(image_in[i]){
        xz[i] = 0.;
        yz[i] = 0.;
        xx[i] = 0.;
        yy[i] = 0.;
    } else {
        xz[i] = basis.x * basis.z;
        yz[i] = basis.y * basis.z;
        xx[i] = basis.x * basis.x;
        yy[i] = basis.y * basis.y;
    }
}

@compute @workgroup_size(WGS)
fn generate_sums_lines(@builtin(global_invocation_id) global_index: vec3<u32>) {
    let i = global_index.x;
    let i_t = index_transpose(i);
    if i >= image_len() { return; }

    let basis = calc_basis_lines(i);
    if is_nan_f32(image_in[i]){
        xz[i_t] = 0.;
        xx[i_t] = 0.;
    } else {
        xz[i_t] = basis.x * basis.z;
        xx[i_t] = basis.x * basis.x;
    }
}

var<workgroup> xz_sum_wg: array<f64, WGS>;
var<workgroup> yz_sum_wg: array<f64, WGS>;
var<workgroup> xx_sum_wg: array<f64, WGS>;
var<workgroup> yy_sum_wg: array<f64, WGS>;
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
        xx_sum_wg[local_index] = xx[read_idx];
        yy_sum_wg[local_index] = yy[read_idx];
    } else {
        xz_sum_wg[local_index] = 0.;
        yz_sum_wg[local_index] = 0.;
        xx_sum_wg[local_index] = 0.;
        yy_sum_wg[local_index] = 0.;
        return;
    }
    var stride = WGS >> 1u;
    while stride > 0u {
        if local_index >= stride { break; }
        workgroupBarrier();
        xz_sum_wg[local_index] += xz_sum_wg[local_index + stride];
        yz_sum_wg[local_index] += yz_sum_wg[local_index + stride];
        xx_sum_wg[local_index] += xx_sum_wg[local_index + stride];
        yy_sum_wg[local_index] += yy_sum_wg[local_index + stride];
        stride >>= 1u;
    }
    if local_index == 0u {
        xz[read_idx] = xz_sum_wg[0];
        yz[read_idx] = yz_sum_wg[0];
        xx[read_idx] = xx_sum_wg[0];
        yy[read_idx] = yy_sum_wg[0];
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
    if global_id.x >= sz.x { return; }

    let local_id = vec2(local_index % WGS_SQUARE, local_index / WGS_SQUARE);
    let col_read_idx = num_workgroups.y * local_id.y + workgroup_id.y;

    if col_read_idx < sz.y {
        xz_sum_wg[local_index] = xz[idx(sz, global_id.x, col_read_idx)];
        xx_sum_wg[local_index] = xx[idx(sz, global_id.x, col_read_idx)];
    } else {
        xz_sum_wg[local_index] = 0.;
        xx_sum_wg[local_index] = 0.;
        return;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_id.y >= stride {break;}
        workgroupBarrier();
        xz_sum_wg[local_index] += xz_sum_wg[local_index + stride * WGS_SQUARE];
        xx_sum_wg[local_index] += xx_sum_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_id.y == 0u {
        xz[idx(sz, global_id.x, col_read_idx)] = xz_sum_wg[local_id.x];
        xx[idx(sz, global_id.x, col_read_idx)] = xx_sum_wg[local_id.x];
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__mean_subtract(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    let basis = calc_basis(i);
    store_value(i, basis.z);

    if i == 0u{
        planarize_out[1] = f64(count[0]);
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__plane_fit(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32
) {
    let i = global_id.x;
    if i >= image_len() { return; }

    let basis = calc_basis(i);
    let x_slope = xz[0] / xx[0];
    let y_slope = select(yz[0] / yy[0], f64(0), yy[0] == 0.);
    let plane = x_slope * basis.x + y_slope * basis.y;
    store_value(i, basis.z - plane);

    if i == 0u {
        planarize_out[1] = f64(count[0]);
        planarize_out[2] = x_slope;
        planarize_out[3] = y_slope;
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__line_fit(
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let row = i / image_size.x;
    let col = i % image_size.x;

    let basis = calc_basis_lines(i);
    let x_slope = xz[row] / xx[row];
    let plane = x_slope * basis.x;
    store_value(i, basis.z - plane);
    
    if col == 0 {
        planarize_out[1u * image_size.y + row] = f64(count[row]);
        planarize_out[2u * image_size.y + row] = x_slope;
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__line_mean(
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let row = i / image_size.x;
    let col = i % image_size.x;

    let basis = calc_basis_lines(i);
    store_value(i, basis.z);

    if col == 0 {
        planarize_out[1u * image_size.y + row] = f64(count[row]);
    }
}

var<workgroup> min_wg: array<f64, WGS>;
var<workgroup> max_wg: array<f64, WGS>;
var<workgroup> std_dev_sum_wg: array<f64, WGS>;
@compute @workgroup_size(WGS)
fn reduce_normalizations(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>
) {
    let read_idx = num_workgroups.x * local_index + workgroup_id.x;
    if read_idx < image_len() {
        min_wg[local_index] = mins[read_idx];
        max_wg[local_index] = maxs[read_idx];
        std_dev_sum_wg[local_index] = std_devs[read_idx];
        z_count_wg[local_index] = count[read_idx];
    } else {
        min_wg[local_index] = f64_pos_infinity();
        max_wg[local_index] = f64_neg_infinity();
        std_dev_sum_wg[local_index] = 0.;
        z_count_wg[local_index] = 0u;
        return;
    }
    var stride = WGS >> 1u;
    while stride > 0u {
        if local_index >= stride { break; }
        workgroupBarrier();
        min_wg[local_index] = min(min_wg[local_index], min_wg[local_index + stride]);
        max_wg[local_index] = max(max_wg[local_index], max_wg[local_index + stride]);
        std_dev_sum_wg[local_index] += std_dev_sum_wg[local_index + stride];
        z_count_wg[local_index] += z_count_wg[local_index + stride];
        stride >>= 1u;
    }
    if local_index == 0u {
        mins[read_idx] = min_wg[0];
        maxs[read_idx] = max_wg[0];
        std_devs[read_idx] = std_dev_sum_wg[0];
        count[read_idx] = z_count_wg[0];
    }
    if global_id.x == 0u {
        normalize_out.min = mins[0];
        normalize_out.max = maxs[0];
        normalize_out.stddev = sqrt(std_devs[0] / f64(count[0]));
    }
}

@compute @workgroup_size(WGS)
fn clear_texture(
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let global_id = vec2(global_index.x % image_size.x, global_index.x / image_size.x);
    textureStore(texture_out, global_id, vec4(f32_nan(), 0.0, 0.0, 0.0));
    if i == 0u {
        normalize_out.min = f64_nan();
        normalize_out.max = f64_nan();
        normalize_out.stddev = f64_nan();
    }
}

// ------------ Helper Functions ------------

fn index_transpose(i: u32) -> u32{
    return (i % image_size.x) * image_size.y + i / image_size.x;
}

fn store_value(i: u32, value: f64){
    let global_coord = vec2(i % image_size.x, i / image_size.x);
    textureStore(texture_out, global_coord, vec4(f32(value), 0.0, 0.0, 0.0));
    if is_nan_f32(image_in[i]){
        mins[i] = f64_pos_infinity();
        maxs[i] = f64_neg_infinity();
        std_devs[i] = 0.;
    } else {
        mins[i] = value;
        maxs[i] = value;
        std_devs[i] = value * value;
    }
}

fn image_len() -> u32 {
    return image_size.x * image_size.y;
}

fn calc_basis(i: u32) -> vec3<f64> {
    if is_nan_f32(image_in[i]) {
        return vec3(
            f64_nan(),
            f64_nan(),
            f64_nan()
        );
    }
    let w = image_size.x;
    let h = image_size.y;
    let x = i % w;
    let y = i / w;
    return vec3(
        mean_center(w, x),
        mean_center(h, y),
        f64(image_in[i]) - planarize_out[0] / f64(count[0])
    );
}

fn calc_basis_lines(i: u32) -> vec3<f64> {
    if is_nan_f32(image_in[i]) {
        return vec3(
            f64_nan(),
            f64_nan(),
            f64_nan()
        );
    }
    let w = image_size.x;
    let x = i % w;
    let y = i / w;
    return vec3(
        mean_center(w, x),
        0,
        f64(image_in[i]) - planarize_out[y] / f64(count[y])
    );
}

fn mean_center(w: u32, x: u32) -> f64 {
    return f64(i32(x << 1u) - i32(w) + i32(1u)) / 2;
}

fn idx(sz: vec2<u32>, x: u32, y: u32) -> u32 {
    return x + y * sz.x;
}

fn make_u64(hi: u32, lo: u32) -> u64 {
    return (u64(hi) << 32u) | u64(lo);
}

fn f64_pos_infinity() -> f64 {
    let bits = make_u64(0x7FF00000u, 0x00000000u);
    return bitcast<f64>(bits);
}

fn f64_neg_infinity() -> f64 {
    let bits = make_u64(0xFFF00000u, 0x00000000u);
    return bitcast<f64>(bits);
}

fn f64_nan() -> f64 {
    let exp: u64 = u64((1u << 11u) - 1u);
    let mantissa: u64 = u64(1u) << 51u;
    let sign: u64 = u64(0u);
    let bits: u64 = (sign << 63u) | (exp << 52u) | mantissa;
    return bitcast<f64>(bits);
}

fn is_nan_f32(x: f32) -> bool {
    let bits: u32 = bitcast<u32>(x);
    let exp: u32 = (bits >> 23u) & 0xffu;
    let mantissa: u32 = bits & 0x007fffffu;
    return (exp == 0xffu) && (mantissa != 0u);
}

fn f32_nan() -> f32 {
    let exp: u32 = 0xffu;
    let mantissa: u32 = 1u << 22u;
    let sign: u32 = 0u;
    let bits: u32 = (sign << 31u) | (exp << 23u) | mantissa;
    return bitcast<f32>(bits);
}