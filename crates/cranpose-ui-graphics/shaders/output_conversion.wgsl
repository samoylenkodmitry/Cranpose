struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    return VertexOutput(
        vec4<f32>(positions[index], 0.0, 1.0),
        uvs[index],
    );
}

@group(0) @binding(0) var source_texture: texture_2d<f32>;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source_texture));
    let position = clamp(vec2<i32>(floor(input.uv * size)), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
    let color = clamp(textureLoad(source_texture, position, 0), vec4<f32>(0.0), vec4<f32>(1.0));
    return floor(color * 255.0 + vec4<f32>(0.5)) / 255.0;
}
