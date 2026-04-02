// ------------ Vertex Shader ------------

@group(0) @binding(0)
var<uniform> world2screen: mat3x3<f64>;

@group(1) @binding(0)
var<uniform> quad2world: mat3x3<f64>;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var position = vec3(f64(input.vert.x), f64(input.vert.y), 1.0);

    // apply the transforms
    // position = quad2world * position;
    // position = position / position.z;
    // position = world2screen * position;
    position = world2screen * quad2world * position;

    var result: VertexOutput;
    result.position = vec4(f32(position.x), f32(position.y), 0, f32(position.z));
    return result;
}

// ------------ Fragment Shader ------------

@group(0) @binding(1)
var tex_sampler: sampler;

@group(0) @binding(2)
var color_map: texture_1d<f32>;

@group(1) @binding(1)
var<uniform> border_color: vec3<f32>;

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    if textureSample(color_map, tex_sampler, 0.).r > 2.0 {
        discard;
    }
    return vec4(border_color, 1.0);
}

// ------------ Structs and Data ------------

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

struct VertexInput{
    @location(0) vert: vec2<f32>,
}