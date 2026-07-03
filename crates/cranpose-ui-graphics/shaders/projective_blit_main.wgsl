

@vertex
fn projective_blit_vs(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = (input.position.x / blit.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (input.position.y / blit.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.world_pos = input.position;
    return output;
}

@fragment
fn projective_blit_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let p = vec3<f32>(input.world_pos, 1.0);
    let denom = dot(blit.inverse_row2.xyz, p);
    if (abs(denom) <= 0.00001) {
        discard;
    }

    let source_x = dot(blit.inverse_row0.xyz, p) / denom;
    let source_y = dot(blit.inverse_row1.xyz, p) / denom;
    if (source_x < 0.0 || source_y < 0.0 || source_x > blit.source_size.x || source_y > blit.source_size.y) {
        discard;
    }

    let source_pos = vec2<f32>(source_x, source_y);
    return composite_sample(source_pos, blit.source_size, blit.sampling.x, vec2<f32>(0.0, 0.0))
        * blit.alpha.x;
}
