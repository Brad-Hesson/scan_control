
struct Metadata {
    width: u32,
    max: f32,
    min: f32,
    // mean: f32,
    // std_dev: f32,
}

// ------------ Vertex Shader ------------

@group(0) @binding(0)
var<uniform> met: Metadata;

@group(0) @binding(1)
var<storage> data: array<f32>;

@group(0) @binding(2)
var texture: texture_storage_2d<r32float, write>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let v = data[global_id.y * met.width + global_id.x];
    let out = (v - met.min) / met.max;
    textureStore(texture, global_id.xy, vec4(out, 0.0, 0.0, 0.0));
}