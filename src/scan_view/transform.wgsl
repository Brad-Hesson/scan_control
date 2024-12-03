@group(0) @binding(0)
var<uniform> size: vec2<u32>;

@group(0) @binding(1)
var<storage, read_write> data: array<f32>;

@compute @workgroup_size(16, 16)
fn row_sum(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= size) {
        return;
    }
    let row = global_id.y;
    let id = global_id.x;
    let len = size.x;
    var split = pow2_floor(len);
    if id < (len - split) {
        write_cell(id, row, read_cell(id, row) + read_cell(id + split, row));
    }
    loop {
        split /= 2u;
        if id >= split {
            return;
        }
        storageBarrier();
        write_cell(id, row, read_cell(id, row) + read_cell(id + split, row));
    }
}

@compute @workgroup_size(16, 16)
fn col_sum(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= size) {
        return;
    }
    let col = global_id.x;
    let id = global_id.y;
    let len = size.y;
    var split = pow2_floor(len);
    if id < (len - split) {
        write_cell(col, id, read_cell(col, id) + read_cell(col, id + split));
    }
    loop {
        split /= 2u;
        if id >= split {
            return;
        }
        storageBarrier();
        write_cell(col, id, read_cell(col, id) + read_cell(col, id + split));
    }
}

@compute @workgroup_size(16, 16)
fn sum(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= size) {
        return;
    }
    let id = global_id.y * size.x + global_id.x;
    let len = size.x * size.y;
    var num = pow2_floor(len);
    if id < (len - num) {
        data[id] = data[id] + data[id + num];
    }
    loop {
        num /= 2u;
        if id >= num {
            return;
        }
        storageBarrier();
        data[id] = data[id] + data[id + num];
    }
}

fn read_cell(x: u32, y: u32) -> f32 {
    return data[y * size.x + x];
}
fn write_cell(x: u32, y: u32, d: f32) {
    data[y * size.x + x] = d;
}

fn pow2_floor(n: u32) -> u32 {
    return u32(1u << (31u - countLeadingZeros(n - 1u)));
}