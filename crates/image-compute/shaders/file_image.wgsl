// ------------ Vertex Shader ------------

@group(0) @binding(0)
var<uniform> world2screen: mat3x3<f64>;

@group(1) @binding(0)
var<uniform> quad2world: mat3x3<f64>;

@vertex
fn vs_main(@builtin(vertex_index) vert_index: u32) -> VertexOutput {
    var position = vec3(verts[vert_index], 1.0);
    var uv = uvs[vert_index];

    // apply the transforms
    position = world2screen * quad2world * position;

    var result: VertexOutput;
    result.position = vec4(f32(position.x), f32(position.y), 0, f32(position.z));
    result.uv = uv;
    return result;
}

// ------------ Fragment Shader ------------

@group(0) @binding(1)
var tex_sampler: sampler;

@group(0) @binding(2)
var color_map: texture_1d<f32>;

@group(1) @binding(1)
var image_tex: texture_2d<f32>;

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    if textureSample(color_map, tex_sampler, 0.).r > 2.0 {
        discard;
    }
    return textureSample(image_tex, tex_sampler, vertex.uv);
}

// ------------ Structs and Data ------------

struct VertexOutput {
    @location(0) uv: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

var<private> verts: array<vec2<f64>, 4> = array(
    vec2(-0.5, -0.5), // TL
    vec2(0.5, -0.5),  // TR
    vec2(-0.5, 0.5),  // BL
    vec2(0.5, 0.5),   // BR
);

var<private> uvs: array<vec2<f32>, 4> = array(
    vec2(0.0, 1.0), // TL
    vec2(1.0, 1.0), // TR
    vec2(0.0, 0.0), // BL
    vec2(1.0, 0.0), // BR
);