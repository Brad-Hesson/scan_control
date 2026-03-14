@group(0) @binding(0)
var<uniform> image_size: vec2<u32>;
@group(0) @binding(1)
var<storage, read> image_in: array<f32>;

@group(1) @binding(0)
var texture_out: texture_storage_2d<r32float, write>;
@group(1) @binding(1)
var<storage, read_write> planarize_out: array<f32>;
@group(1) @binding(2)
var<storage, read_write> normalize_out: NormalizeData;

@group(2) @binding(0)
var<storage, read_write> count: array<u32>;
@group(2) @binding(1)
var<storage, read_write> mins: array<f32>;
@group(2) @binding(2)
var<storage, read_write> maxs: array<f32>;
@group(2) @binding(3)
var<storage, read_write> std_devs: array<f32>;
@group(2) @binding(4)
var<storage, read_write> xz: array<f32>;
@group(2) @binding(5)
var<storage, read_write> yz: array<f32>;
@group(2) @binding(6)
var<storage, read_write> xx: array<f32>;
@group(2) @binding(7)
var<storage, read_write> yy: array<f32>;

struct NormalizeData {
    stddev: f32,
    min: f32,
    max: f32,
}

const WGS: u32 = 256u;
const WGS_SQUARE: u32 = 16u;

@compute @workgroup_size(WGS)
fn copy_image(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    if is_nan_f32(image_in[i]) {
        planarize_out[i] = 0.;
        count[i] = 0u;
    } else {
        planarize_out[i] = image_in[i];
        count[i] = 1u;
    }
}

@compute @workgroup_size(WGS)
fn copy_image_transpose(@builtin(global_invocation_id) global_index: vec3<u32>) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let i_t = index_transpose(i);

    if is_nan_f32(image_in[i]) {
        planarize_out[i_t] = 0.;
        count[i_t] = 0u;
    } else {
        planarize_out[i_t] = image_in[i];
        count[i_t] = 1u;
    }
}

var<workgroup> z_sum_wg: array<f32, WGS>;
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
        if local_index >= stride { break; }
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
    let read_idx = idx(sz, global_id.x, col_read_idx);

    if col_read_idx < sz.y {
        z_sum_wg[local_index] = planarize_out[read_idx];
        z_count_wg[local_index] = count[read_idx];
    } else {
        z_sum_wg[local_index] = 0.;
        z_count_wg[local_index] = 0u;
        return;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_id.y >= stride { break; }
        workgroupBarrier();
        z_sum_wg[local_index] += z_sum_wg[local_index + stride * WGS_SQUARE];
        z_count_wg[local_index] += z_count_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_id.y == 0u {
        planarize_out[read_idx] = z_sum_wg[local_id.x];
        count[read_idx] = z_count_wg[local_id.x];
    } else {
        count[read_idx] = 0u;
    }
}

@compute @workgroup_size(WGS)
fn generate_sums_plane(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    let basis = calc_basis(i);
    if is_nan_f32(basis.z) {
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
    if i >= image_len() { return; }
    let i_t = index_transpose(i);

    let basis = calc_basis_lines(i);
    if is_nan_f32(basis.z) {
        xz[i_t] = 0.;
        xx[i_t] = 0.;
    } else {
        xz[i_t] = basis.x * basis.z;
        xx[i_t] = basis.x * basis.x;
    }
}

var<workgroup> xz_sum_wg: array<f32, WGS>;
var<workgroup> yz_sum_wg: array<f32, WGS>;
var<workgroup> xx_sum_wg: array<f32, WGS>;
var<workgroup> yy_sum_wg: array<f32, WGS>;
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

    let local_row = local_index / WGS_SQUARE;
    let local_col = local_index % WGS_SQUARE;
    let col_read_idx = num_workgroups.y * local_row + workgroup_id.y;
    let read_idx = idx(sz, global_id.x, col_read_idx);

    if col_read_idx < sz.y {
        xz_sum_wg[local_index] = xz[read_idx];
        xx_sum_wg[local_index] = xx[read_idx];
    } else {
        xz_sum_wg[local_index] = 0.;
        xx_sum_wg[local_index] = 0.;
        return;
    }
    var stride = WGS_SQUARE >> 1u;
    while stride > 0u {
        if local_row >= stride { break; }
        workgroupBarrier();
        xz_sum_wg[local_index] += xz_sum_wg[local_index + stride * WGS_SQUARE];
        xx_sum_wg[local_index] += xx_sum_wg[local_index + stride * WGS_SQUARE];
        stride >>= 1u;
    }
    if local_row == 0u {
        xz[read_idx] = xz_sum_wg[local_col];
        xx[read_idx] = xx_sum_wg[local_col];
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__mean_subtract(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }

    let basis = calc_basis(i);
    store_value(i, basis.z);

    if i == 0u {
        planarize_out[1] = f32(count[0]);
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
    let y_slope = select(yz[0] / yy[0], 0., yy[0] == 0.);
    let plane = x_slope * basis.x + y_slope * basis.y;
    store_value(i, basis.z - plane);

    if i == 0u {
        planarize_out[1] = f32(count[0]);
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
        planarize_out[1u * image_size.y + row] = f32(count[row]);
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
        planarize_out[1u * image_size.y + row] = f32(count[row]);
    }
}

var<workgroup> min_wg: array<f32, WGS>;
var<workgroup> max_wg: array<f32, WGS>;
var<workgroup> std_dev_sum_wg: array<f32, WGS>;
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
        min_wg[local_index] = f32_pos_inf();
        max_wg[local_index] = f32_neg_inf();
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
        normalize_out.stddev = sqrt(std_devs[0] / f32(count[0]));
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
        normalize_out.min = f32_nan();
        normalize_out.max = f32_nan();
        normalize_out.stddev = f32_nan();
    }
}

// ------------ Helper Functions ------------

fn index_transpose(i: u32) -> u32 {
    return (i % image_size.x) * image_size.y + i / image_size.x;
}

fn store_value(i: u32, value: f32) {
    let global_coord = vec2(i % image_size.x, i / image_size.x);
    textureStore(texture_out, global_coord, vec4(value, 0.0, 0.0, 0.0));
    if is_nan_f32(value) {
        mins[i] = f32_pos_inf();
        maxs[i] = f32_neg_inf();
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

fn calc_basis(i: u32) -> vec3<f32> {
    let width = image_size.x;
    let height = image_size.y;
    let row = i / width;
    let col = i % width;
    let mean = planarize_out[0] / f32(count[0]);
    return vec3(norm_pos(width, col), norm_pos(height, row), image_in[i] - mean);
}

fn calc_basis_lines(i: u32) -> vec3<f32> {
    let width = image_size.x;
    let row = i / width;
    let col = i % width;
    let mean = planarize_out[row] / f32(count[row]);
    return vec3(norm_pos(width, col), 0, image_in[i] - mean);
}

fn norm_pos(w: u32, x: u32) -> f32 {
    return f32(i32(x << 1u) - i32(w) + i32(1u)) / f32((w << 1u) - 2u);
}

fn idx(sz: vec2<u32>, x: u32, y: u32) -> u32 {
    return x + y * sz.x;
}

fn f32_pos_inf() -> f32 {
    return bitcast<f32>(0x7F800000u);
}

fn f32_neg_inf() -> f32 {
    return bitcast<f32>(0xFF800000u);
}

fn f32_nan() -> f32 {
    return bitcast<f32>(0x7FC00000u);
}

fn is_nan_f32(x: f32) -> bool {
    let bits = bitcast<u32>(x);
    let exp = extractBits(bits, 23u, 8u);
    let mant = extractBits(bits, 0u, 23u);
    return exp == 0xFFu && mant != 0u;
}