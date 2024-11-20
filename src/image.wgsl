struct VertexOutput {
    @location(0) color: vec3<f32>,
    @builtin(position) position: vec4<f32>,
};

var<private> verts: array<vec2<f32>, 4> = array(
    vec2(-1.0, -1.0),
    vec2(1.0, -1.0),
    vec2(-1.0, 1.0),
    vec2(1.0, 1.0),
);

var<private> colors: array<vec3<f32>, 4> = array(
    vec3<f32>(1.0, 0.0, 0.0),
    vec3<f32>(0.0, 1.0, 0.0),
    vec3<f32>(0.0, 0.0, 1.0),
    vec3<f32>(1.0, 0.0, 0.0),
);

@group(0)
@binding(0)
var<uniform> world2screen: mat4x4<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vert_index: u32) -> VertexOutput {
    var result: VertexOutput;
    result.position = world2screen * vec4(verts[vert_index], 0.0, 1.0);
    result.color = colors[vert_index];
    return result;
}

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(vertex.color, 1.0);
}