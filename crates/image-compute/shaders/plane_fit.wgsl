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
var<storage, read_write> a: array<f64>;
@group(2) @binding(5)
var<storage, read_write> b: array<f64>;
@group(2) @binding(6)
var<storage, read_write> c: array<f64>;
@group(2) @binding(7)
var<storage, read_write> d: array<f64>;

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
    planarize_out[i] = f64(image_in[i]);
    if is_nan_f32(image_in[i]) {
        count[i] = 0u;
    }else{
        count[i] = 1u;
    }
}

@compute @workgroup_size(WGS)
fn copy_image_transpose(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if global_id.x >= image_len() { return; }
    let x = global_id.x % image_size.x;
    let y = global_id.x / image_size.x;
    planarize_out[x * image_size.y + y] = f64(image_in[y * image_size.x + x]);
    if is_nan_f32(image_in[y * image_size.x + x]) {
        count[x * image_size.y + y] = 0u;
    }else{
        count[x * image_size.y + y] = 1u;
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
        z_sum_wg[local_index] = replace_nan_f64(planarize_out[read_idx], 0.);
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
        planarize_out[read_idx] = 0.;
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
    let local_id = vec2(local_index % WGS_SQUARE, local_index / WGS_SQUARE);
    let col_read_idx = num_workgroups.y * local_id.y + workgroup_id.y;
    if col_read_idx < sz.y && global_id.x < sz.x {
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
        planarize_out[idx(sz, global_id.x, col_read_idx)] = 0.;
        count[idx(sz, global_id.x, col_read_idx)] = 0u;
    }
}

@compute @workgroup_size(WGS)
fn generate_sums_plane(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }
    let basis = calc_basis(i);
    a[i] = basis.x * basis.z;
    b[i] = basis.y * basis.z;
    c[i] = basis.x * basis.x;
    d[i] = basis.y * basis.y;
}

@compute @workgroup_size(WGS)
fn generate_sums_lines(@builtin(global_invocation_id) global_index: vec3<u32>) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let basis = calc_basis_lines(i);
    let global_id = vec2(global_index.x % image_size.x, global_index.x / image_size.x);
    a[idx(image_size.yx, global_id.y, global_id.x)] = basis.x * basis.z;
    b[idx(image_size.yx, global_id.y, global_id.x)] = basis.x * basis.x;
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
        xz_sum_wg[local_index] = replace_nan_f64(c[read_idx], 0.);
        yz_sum_wg[local_index] = replace_nan_f64(d[read_idx], 0.);
        xx_sum_wg[local_index] = replace_nan_f64(a[read_idx], 0.);
        yy_sum_wg[local_index] = replace_nan_f64(b[read_idx], 0.);
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
        a[read_idx] = xz_sum_wg[0];
        b[read_idx] = yz_sum_wg[0];
        c[read_idx] = xx_sum_wg[0];
        d[read_idx] = yy_sum_wg[0];
    } else {
        a[read_idx] = 0.;
        b[read_idx] = 0.;
        c[read_idx] = 0.;
        d[read_idx] = 0.;
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
    if col_read_idx < sz.y && global_id.x < sz.x {
        xz_sum_wg[local_index] = replace_nan_f64(a[idx(sz, global_id.x, col_read_idx)], 0.);
        xx_sum_wg[local_index] = replace_nan_f64(b[idx(sz, global_id.x, col_read_idx)], 0.);
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
        a[idx(sz, global_id.x, col_read_idx)] = xz_sum_wg[local_id.x];
        b[idx(sz, global_id.x, col_read_idx)] = xx_sum_wg[local_id.x];
    } else {
        a[idx(sz, global_id.x, col_read_idx)] = 0.;
        b[idx(sz, global_id.x, col_read_idx)] = 0.;
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__mean_subtract(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let i = global_id.x;
    if i >= image_len() { return; }
    let basis = calc_basis(i);
    textureStore(texture_out, vec2(global_id.x % image_size.x, global_id.x / image_size.x), vec4(f32(basis.z), 0.0, 0.0, 0.0));
    mins[i] = basis.z;
    maxs[i] = basis.z;
    std_devs[i] = basis.z * basis.z;
    if i == 0u{
        planarize_out[1] = f64(count[0]);
    }
}

var<workgroup> x_slope: f64;
var<workgroup> y_slope: f64;
@compute @workgroup_size(WGS)
fn generate_normalization__plane_fit(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32
) {
    let i = global_id.x;
    if i >= image_len() { return; }
    if local_index == 0u {
        let s_xz = a[0];
        let s_yz = b[0];
        let s_xx = c[0];
        let s_yy = d[0];
        x_slope = s_xz / s_xx;
        if s_yy == 0 {
            y_slope = 0;
        } else {
            y_slope = s_yz / s_yy;
        }
    }
    workgroupBarrier();
    let basis = calc_basis(i);
    let plane = x_slope * basis.x + y_slope * basis.y;
    let value = basis.z - plane;
    textureStore(texture_out, vec2(global_id.x % image_size.x, global_id.x / image_size.x), vec4(f32(value), 0.0, 0.0, 0.0));
    mins[i] = value;
    maxs[i] = value;
    std_devs[i] = value * value;
    if i == 0u {
        planarize_out[1] = f64(count[0]);
        planarize_out[2] = x_slope;
        planarize_out[3] = y_slope;
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__line_fit(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let global_id = vec2(i % image_size.x, i / image_size.x);

    let s_xz = a[global_id.y];
    let s_xx = b[global_id.y];
    let x_slope = s_xz / s_xx;

    let basis = calc_basis_lines(i);
    let plane = x_slope * basis.x;
    let value = basis.z - plane;
    textureStore(texture_out, global_id, vec4(f32(value), 0.0, 0.0, 0.0));
    mins[i] = value;
    maxs[i] = value;
    std_devs[i] = value * value;
    if global_id.x == 0 {
        planarize_out[1u * image_size.y + global_id.y] = f64(count[global_id.y]);
        planarize_out[2u * image_size.y + global_id.y] = x_slope;
    }
}

@compute @workgroup_size(WGS)
fn generate_normalization__line_mean(
    @builtin(local_invocation_index) local_index: u32,
    @builtin(global_invocation_id) global_index: vec3<u32>
) {
    let i = global_index.x;
    if i >= image_len() { return; }
    let global_id = vec2(i % image_size.x, i / image_size.x);

    let basis = calc_basis_lines(i);
    let value = basis.z;
    textureStore(texture_out, global_id, vec4(f32(value), 0.0, 0.0, 0.0));
    mins[i] = value;
    maxs[i] = value;
    std_devs[i] = value * value;
    if global_id.x == 0 {
        planarize_out[1u * image_size.y + global_id.y] = f64(count[global_id.y]);
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
        min_wg[local_index] = replace_nan_f64(mins[read_idx], f64_pos_infinity());
        max_wg[local_index] = replace_nan_f64(maxs[read_idx], f64_neg_infinity());
        std_dev_sum_wg[local_index] = replace_nan_f64(std_devs[read_idx], 0.);
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
    } else {
        mins[read_idx] = f64_pos_infinity();
        maxs[read_idx] = f64_neg_infinity();
        std_devs[read_idx] = 0.;
        count[read_idx] = 0u;
    }
    if global_id.x == 0u {
        normalize_out.min = mins[0];
        normalize_out.max = maxs[0];
        normalize_out.stddev = sqrt(std_devs[0] / f64(count[0]));
    }
}

@compute @workgroup_size(WGS)
fn clear_texture(
    @builtin(local_invocation_index) local_index: u32,
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

fn make_u64(hi: u32, lo: u32) -> u64 {
    return (u64(hi) << 32u) | u64(lo);
}

// +Infinity: 0x7FF0_0000_0000_0000
fn f64_pos_infinity() -> f64 {
    let bits = make_u64(0x7FF00000u, 0x00000000u);
    return bitcast<f64>(bits);
}

// -Infinity: 0xFFF0_0000_0000_0000
fn f64_neg_infinity() -> f64 {
    let bits = make_u64(0xFFF00000u, 0x00000000u);
    return bitcast<f64>(bits);
}

fn f64_nan() -> f64 {
    // Exponent = all ones (0x7ff)
    let exp: u64 = u64((1u << 11u) - 1u);

    // Mantissa: just set the top bit of the mantissa (bit 51)
    // Build as u64 without u64 literals
    let mantissa: u64 = u64(1u) << 51u;

    // Sign = 0
    let sign: u64 = u64(0u);

    // Assemble: (sign << 63) | (exp << 52) | mantissa
    let bits: u64 = (sign << 63u) | (exp << 52u) | mantissa;

    return bitcast<f64>(bits);
}

fn is_nan_f64(x: f64) -> bool {
    let bits: u64 = bitcast<u64>(x);
    let exp: u64 = (bits >> 52u) & u64(0x7ffu);
    let mantissa: u64 = bits & ((u64(1u) << 52u) - u64(1u));
    return (exp == u64(0x7ffu)) && (mantissa != u64(0u));
}
fn is_nan_f32(x: f32) -> bool {
    let bits: u32 = bitcast<u32>(x);
    let exp: u32 = (bits >> 23u) & 0xffu;
    let mantissa: u32 = bits & 0x007fffffu;
    return (exp == 0xffu) && (mantissa != 0u);
}

fn replace_nan_f64(x: f64, replacement: f64) -> f64 {
    if is_nan_f64(x) {
        return replacement;
    } else {
        return x;
    }
}

fn f32_nan() -> f32 {
    // Exponent = all ones (0xFF)
    let exp: u32 = 0xffu;

    // Mantissa: set the top mantissa bit (bit 22)
    let mantissa: u32 = 1u << 22u;

    // Sign = 0
    let sign: u32 = 0u;

    // Assemble: (sign << 31) | (exp << 23) | mantissa
    let bits: u32 = (sign << 31u) | (exp << 23u) | mantissa;

    return bitcast<f32>(bits);
}