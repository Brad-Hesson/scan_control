const border_width: f32 = 0.02;
const border_color: vec4<f32> = vec4(0.2, 1.0, 0.2, 1.0);

// ------------ Vertex Shader ------------

@group(0) @binding(0)
var<uniform> world2screen: mat4x4<f32>;

@group(1) @binding(0)
var<uniform> quad2world: mat4x4<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vert_index: u32) -> VertexOutput {
    var position = vec4(verts[vert_index], 0.0, 1.0);
    var uv = uvs[vert_index];
    let is_vertical = dot(quad2world * vec4(1., 0., 0., 0.), vec4(1., 0., 0., 0.)) > 0.;

    // if border is enabled, grow the quad and extend the uvs by 2*border width
    if is_vertical {
        position.x *= 1.0 + border_width * 2.;
        position.y *= 1.0 + border_width * 2.;
        uv *= 1.0 + border_width * 2.;
        uv -= border_width;
    }

    // apply the transforms
    position = world2screen * quad2world * position;

    var result: VertexOutput;
    result.position = position;
    result.uv = uv;
    return result;
}

// ------------ Fragment Shader ------------

@group(0) @binding(1)
var tex_sampler: sampler;

@group(1) @binding(1)
var texture: texture_2d<f32>;

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    // if the uv goes outside of the standard coords, it means we want to draw a border
    if vertex.uv.x > 1.0 || vertex.uv.x < 0.0 || vertex.uv.y > 1.0 || vertex.uv.y < 0.0 {
        return vec4(0.0, 1.0, 0.0, 1.0);
    }
    let val = textureSample(texture, tex_sampler, vertex.uv).r;
    if isNan(val) {
        discard;
    }
    return vec4(val, val, val, 0.5);
}

// ------------ Structs and Data ------------

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

fn isNan(value: f32) -> bool {
    return extractBits(bitcast<u32>(value), 23u, 8u) == 0xFF;
}