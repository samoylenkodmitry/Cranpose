@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(input_texture, input_sampler, input.uv);
    let position = input.uv * u[0].xy;
    let rgb = source.rgb / max(source.a, 0.0001);
    let lit = floating_contact_light(rgb, length(position - u[0].zw), u[1].y, u[1].x);
    return vec4<f32>(lit * source.a, source.a);
}
