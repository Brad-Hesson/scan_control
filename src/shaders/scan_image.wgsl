struct VertexOutput {
    @location(0) uv: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

var<private> verts: array<vec2<f32>, 4> = array(
    vec2(-1.0, -1.0),
    vec2(1.0, -1.0),
    vec2(-1.0, 1.0),
    vec2(1.0, 1.0),
);

var<private> uvs: array<vec2<f32>, 4> = array(
    vec2(0.0, 0.0),
    vec2(1.0, 0.0),
    vec2(0.0, 1.0),
    vec2(1.0, 1.0),
);

@group(0) @binding(0)
var<uniform> world2screen: mat4x4<f32>;

@group(1) @binding(0)
var<uniform> quad2world: mat4x4<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vert_index: u32) -> VertexOutput {
    var result: VertexOutput;
    result.position = world2screen * quad2world * vec4(verts[vert_index], 0.0, 1.0);
    result.uv = uvs[vert_index];
    return result;
}

@group(1) @binding(1)
var texture: texture_2d<f32>;

@group(0) @binding(1)
var tex_sampler: sampler;

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    let val = textureSample(texture, tex_sampler, vertex.uv).r;
    if (isNan(val)){
        return vec4(1.0, 0.0, 0.0, 0.0);
    } else {
        return vec4(val, val, val, 1.0);
    }
}

fn isNan(value: f32) -> bool{
    return extractBits(bitcast<u32>(value), 23u, 8u) == 0xFF;
}