// Ink that breaks into a halftone of dots, and the dots that then leave one
// by one, each at its own time. It follows `noise.wgsl`.
//
// Uniforms: 0 = how far gone, from 0 (solid ink) to 1 (nothing left);
// 1..3 = the dots' cell across and down, in logical pixels.

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let gone = get_float(0u);
    let size = vec2<f32>(textureDimensions(input_texture));
    // The logical size the input stands for, from the reserved slots.
    let logical = select(size, u[63].xy, u[63].x > 0.0);
    let point = input.uv * logical;
    let cell = vec2<f32>(get_float(1u), get_float(2u));
    let index = floor(point / cell);
    let center = (index + vec2<f32>(0.5)) * cell;
    let own = textureSampleLevel(input_texture, input_sampler, input.uv, 0.0);
    let ink = textureSampleLevel(input_texture, input_sampler, center / logical, 0.0);
    // A dot holds as much ink as its cell did, and leaves at its own time.
    let leaves = 0.15 + 0.75 * hash(index);
    let stays = 1.0 - smoothstep(leaves - 0.1, leaves + 0.1, gone);
    let offset = (point - center) / cell;
    let radius = 0.55 * sqrt(ink.a) * stays;
    let edge = fwidth(length(offset)) + 1.0e-4;
    let dot_cover = 1.0 - smoothstep(radius - edge, radius + edge, length(offset));
    let dotted = vec4<f32>(ink.rgb / max(ink.a, 1.0e-4), 1.0) * dot_cover;
    let breaking = smoothstep(0.0, 0.18, gone);
    return mix(own, dotted, breaking);
}
