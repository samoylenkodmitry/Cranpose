

@vertex
fn projective_blit_vs(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = (input.position.x / blit.viewport.x) * 2.0 - 1.0;
    let y = 1.0 - (input.position.y / blit.viewport.y) * 2.0;
    output.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    output.world_pos = input.position;
    return output;
}

fn projective_texel(origin: vec2<i32>, texel: vec2<i32>, last: vec2<i32>) -> vec4<f32> {
    return textureLoad(input_texture, origin + clamp(texel, vec2<i32>(0), last), 0);
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
    let in_region = blit.source_region.z > 0.0 && blit.source_region.w > 0.0;
    let extent = select(blit.source_size, blit.source_region.zw, in_region);
    if (source_x < 0.0 || source_y < 0.0 || source_x > extent.x || source_y > extent.y) {
        discard;
    }

    let origin = vec2<i32>(select(vec2<f32>(0.0), blit.source_region.xy, in_region));
    let last = vec2<i32>(extent) - vec2<i32>(1);
    let source_pos = vec2<f32>(source_x, source_y);
    if (blit.sampling.x > 0.5) {
        return projective_texel(origin, vec2<i32>(floor(source_pos)), last) * blit.alpha.x;
    }
    // Filtered here rather than by the sampler: the weights depend only on
    // the position within the surface, so a surface that shares a texture
    // composites exactly as one in a texture of its own, and a tap at its
    // edge reads its own texels as clamp-to-edge would.
    let texel_pos = clamp(source_pos, vec2<f32>(0.5), extent - vec2<f32>(0.5)) - vec2<f32>(0.5);
    let base = floor(texel_pos);
    let weight = texel_pos - base;
    let cell = vec2<i32>(base);
    let top = mix(
        projective_texel(origin, cell, last),
        projective_texel(origin, cell + vec2<i32>(1, 0), last),
        weight.x,
    );
    let bottom = mix(
        projective_texel(origin, cell + vec2<i32>(0, 1), last),
        projective_texel(origin, cell + vec2<i32>(1, 1), last),
        weight.x,
    );
    return mix(top, bottom, weight.y) * blit.alpha.x;
}
