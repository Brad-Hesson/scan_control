const low_color: vec3<f32> = vec3(0.0, 0.0, 1.0);
const high_color: vec3<f32> = vec3(1.0, 0.0, 0.0);
const IMAGE_ALPHA: f32 = 1.;

// ------------ Vertex Shader ------------

@group(0) @binding(0)
var<uniform> world2screen: mat4x4<f32>;

@group(1) @binding(0)
var<uniform> quad2world: mat4x4<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vert_index: u32) -> VertexOutput {
    var position = vec4(verts[vert_index], 0.0, 1.0);
    var uv = uvs[vert_index];

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

@group(0) @binding(2)
var color_map: texture_1d<f32>;

@group(1) @binding(1)
var height_map: texture_2d<f32>;
@group(1) @binding(2)
var<uniform> normalize_data: NormalizeData;
@group(1) @binding(3)
var<uniform> normalize_control: NormalizeControl;

@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    // sample the height of this pixel from the height-map texture
    let raw = textureSample(height_map, tex_sampler, vertex.uv).r;
    
    // if the datapoint doesn't exist, discard the fragment
    if isNan(raw) {
        discard;
    }

    // normalize the datapoint based on NormalizeControl
    var height: f32;
    if normalize_control.max_min != 0u {
        height = (raw - normalize_data.min) / (normalize_data.max - normalize_data.min);
    } else {
        let factor = normalize_control.std_dev_mul * normalize_data.stddev * 3.0;
        height = (raw / factor) + 0.5;
    }

    // if the hight is out of range, draw the high or low overflow color
    if height < 0.0 {
        return vec4(low_color, IMAGE_ALPHA);
    }
    if height > 1.0 {
        return vec4(high_color, IMAGE_ALPHA);
    }

    // sample the color from the color-map and return it
    return vec4(textureSample(color_map, tex_sampler, height).rgb, IMAGE_ALPHA);
}

// ------------ Structs and Data ------------

struct NormalizeControl{
    max_min: u32,
    _pad: u32,
    std_dev_mul: f32
}

struct NormalizeData{
    stddev: f32,
    min: f32,
    max: f32,
}

struct VertexOutput {
    @location(0) uv: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

var<private> verts: array<vec2<f32>, 4> = array(
    vec2(-1.0, -1.0), // TL
    vec2(1.0, -1.0),  // TR
    vec2(-1.0, 1.0),  // BL
    vec2(1.0, 1.0),   // BR
);

var<private> uvs: array<vec2<f32>, 4> = array(
    vec2(0.0, 0.0), // TL
    vec2(1.0, 0.0), // TR
    vec2(0.0, 1.0), // BL
    vec2(1.0, 1.0), // BR
);

fn isNan(value: f32) -> bool {
    let bits = bitcast<u32>(value);
    let exp  = extractBits(bits, 23u, 8u);
    let mant = extractBits(bits, 0u, 23u);
    return exp == 0xFFu && mant != 0u;
}