

@fragment
fn blit_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let use_dest_viewport = blit.dest_viewport.z > 0.0 && blit.dest_viewport.w > 0.0;
    let use_source_viewport = blit.source_viewport.z > 0.0 && blit.source_viewport.w > 0.0;
    var source_origin = vec2<f32>(0.0);
    var source_size = tex_size;
    if use_source_viewport {
        source_origin = blit.source_viewport.xy;
        source_size = blit.source_viewport.zw;
    }
    let dest_pos = input.position.xy;
    var source_pos = source_origin + input.uv * source_size;
    var resolve_span = blit.resolve_span.xy;
    if use_dest_viewport {
        let viewport_max = blit.dest_viewport.xy + blit.dest_viewport.zw;
        if dest_pos.x < blit.dest_viewport.x || dest_pos.y < blit.dest_viewport.y ||
            dest_pos.x >= viewport_max.x || dest_pos.y >= viewport_max.y {
            discard;
        }
        let local_dest = dest_pos - blit.dest_viewport.xy;
        source_pos = vec2<f32>(
            source_origin.x + local_dest.x * source_size.x / blit.dest_viewport.z,
            source_origin.y + local_dest.y * source_size.y / blit.dest_viewport.w,
        );
    }
    let sampled =
        composite_sample(source_pos, tex_size, blit.sampling.x, resolve_span) * blit.alpha.x;
    if (blit.mask_enabled.x <= 0.5) {
        return sampled;
    }

    let world_pos = dest_pos;
    let center = blit.mask_rect.xy + blit.mask_rect.zw * 0.5;
    let half_size = blit.mask_rect.zw * 0.5;
    let local_pos = world_pos - center;
    let has_radii = (blit.mask_radii[0] > 0.0 || blit.mask_radii[1] > 0.0 ||
                     blit.mask_radii[2] > 0.0 || blit.mask_radii[3] > 0.0);
    var coverage: f32;
    if (has_radii) {
        let dist = sdf_rounded_rect(local_pos, half_size, blit.mask_radii);
        coverage = 1.0 - smoothstep(-0.5, 0.5, dist);
    } else {
        let cov_x = clamp(half_size.x + 0.5 - abs(local_pos.x), 0.0, 1.0);
        let cov_y = clamp(half_size.y + 0.5 - abs(local_pos.y), 0.0, 1.0);
        coverage = cov_x * cov_y;
    }

    if (coverage <= 0.001) {
        discard;
    }
    return sampled * coverage;
}
