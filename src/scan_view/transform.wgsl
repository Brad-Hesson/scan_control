@group(0) @binding(0)
var<uniform> size_out: vec2<u32>;
@group(0) @binding(1)
var<storage, read_write> data_out: array<f32>;

@group(1) @binding(0)
var<uniform> size_a: vec2<u32>;
@group(1) @binding(1)
var<storage, read_write> data_a: array<f32>;

@group(2) @binding(0)
var<uniform> size_b: vec2<u32>;
@group(2) @binding(1)
var<storage, read_write> data_b: array<f32>;

@group(3) @binding(0)
var<uniform> iteration: u32;


@compute @workgroup_size(16, 16)
fn add(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = global_id.xy;
    write_cell_out(pos, read_cell_a(pos) + read_cell_b(pos));
}

@compute @workgroup_size(16, 16)
fn mul(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = global_id.xy;
    write_cell_out(pos, read_cell_a(pos) * read_cell_b(pos));
}

@compute @workgroup_size(16, 16)
fn div(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = global_id.xy;
    write_cell_out(pos, read_cell_a(pos) / read_cell_b(pos));
}

@compute @workgroup_size(16, 16)
fn copy(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = global_id.xy;
    write_cell_out(pos, read_cell_a(pos));
}

var<workgroup> broadcast_val: f32;

@compute @workgroup_size(8, 1)
fn row_broadcast(
    @builtin(global_invocation_id) global_id: vec3<u32>, @builtin(local_invocation_index) local_index: u32,
) {
    let pos = global_id.xy;
    if local_index == 0u {
        broadcast_val = read_cell_a(vec2(0u, pos.y));
    }
    workgroupBarrier();
    write_cell_out(pos, broadcast_val);
}

@compute @workgroup_size(1, 8)
fn col_broadcast(
    @builtin(global_invocation_id) global_id: vec3<u32>, @builtin(local_invocation_index) local_index: u32,
) {
    let pos = global_id.xy;
    if local_index == 0u {
        broadcast_val = read_cell_a(vec2(pos.x, 0u));
    }
    workgroupBarrier();
    write_cell_out(pos, broadcast_val);
}

@compute @workgroup_size(workgroup_size, 1)
fn row_sum(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    let stride = pow_u32(workgroup_size, iteration);
    let pos = vec2(stride, 1u) * global_id.xy;
    wg_array[local_index] = read_cell_out(pos);
    workgroup_sum(local_index);
    workgroupBarrier();
    write_cell_out(pos, wg_array[local_index]);
}

@compute @workgroup_size(1, workgroup_size)
fn col_sum(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    let stride = pow_u32(workgroup_size, iteration);
    let pos = vec2(1u, stride) * global_id.xy;
    wg_array[local_index] = read_cell_out(pos);
    workgroup_sum(local_index);
    workgroupBarrier();
    write_cell_out(pos, wg_array[local_index]);
}

const workgroup_size: u32 = 256;
var<workgroup> wg_array: array<f32, workgroup_size>;

// compute the sum of the data in the wg_array and place it in the 0th index
fn workgroup_sum(local_index: u32) {
    var split = pow2_floor(workgroup_size);
    workgroupBarrier();
    if local_index < (workgroup_size - split) {
        wg_array[local_index] += wg_array[local_index + split];
        wg_array[local_index + split] = 0.;
    }
    loop {
        split /= 2u;
        if split == 0 {
            return;
        }
        workgroupBarrier();
        if local_index < split {
            wg_array[local_index] += wg_array[local_index + split];
            wg_array[local_index + split] = 0.;
        }
    }
}

fn pow_u32(n: u32, exp: u32) -> u32 {
    var out = 1u;
    for (var i = 0u; i < exp; i++) {
        out *= n;
    }
    return out;
}

fn div_ceil(a: u32, b: u32) -> u32 {
    return (a + b - 1) / b;
}

fn pow2_floor(n: u32) -> u32 {
    return u32(1u << (31u - countLeadingZeros(n - 1u)));
}

fn read_cell_a(pos: vec2<u32>) -> f32 {
    return data_a[pos.x + size_a.x * pos.y];
}
fn read_cell_b(pos: vec2<u32>) -> f32 {
    return data_b[pos.x + size_b.x * pos.y];
}
fn write_cell_out(pos: vec2<u32>, val: f32) {
    data_out[pos.x + size_out.x * pos.y] = val;
}
fn read_cell_out(pos: vec2<u32>) -> f32 {
    return data_out[pos.x + size_out.x * pos.y];
}